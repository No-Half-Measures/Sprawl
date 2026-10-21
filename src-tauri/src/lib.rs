mod admin;
mod cache;
mod changes;
mod mft;
mod scanner;

use cache::SavedScan;
use scanner::{Kind, Tree};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Emitter, Manager, State};

#[derive(Default)]
struct Session {
    generation: u64,
    cancel: Option<Arc<AtomicBool>>,
    tree: Option<Arc<Tree>>,
    saved: Option<SavedScan>,
    panels: Arc<RootPanels>,
}

#[derive(Default)]
struct RootPanels {
    largest: OnceLock<Result<Vec<scanner::LargeFile>, String>>,
    breakdown: OnceLock<Result<Vec<scanner::Breakdown>, String>>,
    changes: OnceLock<Result<Option<changes::Comparison>, String>>,
}

impl Session {
    fn open_saved(&mut self, path: &str, scanned_at: u64) -> Result<Option<LoadedScan>, String> {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.generation += 1;
        if let (Some(tree), Some(saved)) = (&self.tree, &self.saved)
            && cache::key(&saved.path) == cache::key(path)
            && saved.scanned_at == scanned_at
        {
            return Ok(Some(LoadedScan {
                root: tree.view(0, 0, 0)?,
                errors: tree.errors,
                saved: saved.clone(),
            }));
        }
        Ok(None)
    }
}
#[derive(Default)]
struct AppState {
    session: Mutex<Session>,
    worker: Mutex<()>,
    cache: Mutex<()>,
    admin_declined: AtomicBool,
}

fn tree(state: &AppState) -> Result<Arc<Tree>, String> {
    state
        .session
        .lock()
        .map_err(|_| "Scan state is unavailable".to_string())?
        .tree
        .clone()
        .ok_or_else(|| "No completed scan. Choose a folder to begin.".into())
}

#[derive(Serialize)]
struct Drive {
    mount: String,
    label: String,
    total: u64,
    free: u64,
}

#[tauri::command]
fn list_drives() -> Vec<Drive> {
    sysinfo::Disks::new_with_refreshed_list()
        .iter()
        .map(|d| Drive {
            mount: d.mount_point().to_string_lossy().into_owned(),
            label: d.name().to_string_lossy().into_owned(),
            total: d.total_space(),
            free: d.available_space(),
        })
        .collect()
}

fn scan_metadata(tree: &Tree) -> SavedScan {
    let path = tree.root_path.to_string_lossy().into_owned();
    let root = &tree.nodes[0];
    let drives = list_drives();
    let drive = drive_for_path(&path, &drives);
    SavedScan {
        version: cache::FORMAT_VERSION,
        path,
        scanned_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        size: root.size,
        files: root.files,
        folders: root.folders,
        errors: tree.errors,
        drive_free: drive.map(|d| d.free),
        drive_total: drive.map(|d| d.total),
    }
}

fn drive_for_path<'a>(path: &str, drives: &'a [Drive]) -> Option<&'a Drive> {
    let path = path.trim_end_matches(['\\', '/']).to_lowercase();
    drives
        .iter()
        .filter(|d| {
            let mount = d.mount.trim_end_matches(['\\', '/']).to_lowercase();
            path == mount
                || path
                    .strip_prefix(&mount)
                    .is_some_and(|s| s.starts_with(['\\', '/']))
        })
        .max_by_key(|d| d.mount.len())
}

fn scans_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|p| p.join("scans"))
        .map_err(|e| e.to_string())
}

fn settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|p| p.join("settings.json"))
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminStatus {
    supported: bool,
    elevated: bool,
    always_elevate: bool,
    declined: bool,
}

#[tauri::command]
fn admin_status(app: tauri::AppHandle, state: State<'_, AppState>) -> AdminStatus {
    let settings = settings_path(&app)
        .map(|path| admin::read_settings(&path))
        .unwrap_or_default();
    AdminStatus {
        supported: cfg!(windows),
        elevated: admin::elevated(),
        always_elevate: settings.always_elevate,
        declined: state.admin_declined.load(Ordering::Relaxed),
    }
}

#[tauri::command]
fn set_always_elevate(enabled: bool, app: tauri::AppHandle) -> Result<(), String> {
    admin::write_settings(
        &settings_path(&app)?,
        &admin::Settings {
            always_elevate: enabled,
        },
    )
    .map_err(|e| format!("Could not save administrator preference: {e}"))
}

#[tauri::command]
fn restart_as_admin(app: tauri::AppHandle) -> Result<(), String> {
    admin::relaunch()?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
async fn list_saved_scans(app: tauri::AppHandle) -> Result<Vec<SavedScan>, String> {
    let dir = scans_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _cache = state
            .cache
            .lock()
            .map_err(|_| "Saved scans are unavailable".to_string())?;
        cache::list(&dir).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct LoadedScan {
    root: scanner::ViewNode,
    errors: u64,
    saved: SavedScan,
}

#[tauri::command]
async fn load_saved_scan(
    path: String,
    scanned_at: u64,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<LoadedScan, String> {
    let generation = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Scan state is unavailable".to_string())?;
        if let Some(loaded) = session.open_saved(&path, scanned_at)? {
            return Ok(loaded);
        }
        session.generation
    };
    let dir = scans_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (tree, saved) = {
            let _cache = state
                .cache
                .lock()
                .map_err(|_| "Saved scans are unavailable".to_string())?;
            cache::load(&dir, &path)?
        };
        let loaded = LoadedScan {
            root: tree.view(0, 0, 0)?,
            errors: tree.errors,
            saved,
        };
        let tree = Arc::new(tree);
        let previous = {
            let mut session = state
                .session
                .lock()
                .map_err(|_| "Scan state is unavailable".to_string())?;
            if session.generation != generation {
                return Err("Another scan has started. Open the saved scan again.".into());
            }
            session.saved = Some(loaded.saved.clone());
            session.panels = Arc::default();
            session.tree.replace(tree)
        };
        std::thread::spawn(move || drop(previous));
        Ok(loaded)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn clear_saved_scans(app: tauri::AppHandle) -> Result<(), String> {
    let dir = scans_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _cache = state
            .cache
            .lock()
            .map_err(|_| "Saved scans are unavailable".to_string())?;
        cache::clear(&dir).map_err(|e| e.to_string())?;
        state
            .session
            .lock()
            .map_err(|_| "Scan state is unavailable".to_string())?
            .panels = Arc::default();
        let _ = app.emit("saved-scans-changed", ());
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanEvent<T: Serialize + Clone> {
    scan_id: u64,
    #[serde(flatten)]
    data: T,
}

#[derive(Clone, Serialize)]
struct ScanError {
    message: String,
}
#[derive(Clone, Serialize)]
struct Cancelled {}

#[derive(Clone, Serialize)]
struct Completed {
    root: scanner::ViewNode,
    errors: u64,
}

#[tauri::command]
fn start_scan(
    path: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<u64, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    let (generation, previous) = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Scan state is unavailable".to_string())?;
        if let Some(old) = session.cancel.take() {
            old.store(true, Ordering::Relaxed);
        }
        session.generation += 1;
        let previous = session.tree.take();
        session.saved = None;
        session.panels = Arc::default();
        session.cancel = Some(cancel.clone());
        (session.generation, previous)
    };
    std::thread::spawn(move || {
        drop(previous);
        let state = app.state::<AppState>();
        let _worker = match state.worker.lock() {
            Ok(worker) => worker,
            Err(_) => {
                let _ = app.emit(
                    "scan-error",
                    ScanEvent {
                        scan_id: generation,
                        data: ScanError {
                            message: "Scanner state is unavailable".into(),
                        },
                    },
                );
                return;
            }
        };
        if cancel.load(Ordering::Relaxed) {
            if let Ok(mut session) = state.session.lock()
                && session.generation == generation
            {
                session.cancel = None;
                let _ = app.emit(
                    "scan-cancelled",
                    ScanEvent {
                        scan_id: generation,
                        data: Cancelled {},
                    },
                );
            }
            return;
        }
        let result = scanner::scan(&PathBuf::from(path), &cancel, |progress| {
            if !cancel.load(Ordering::Relaxed) {
                let _ = app.emit(
                    "scan-progress",
                    ScanEvent {
                        scan_id: generation,
                        data: progress,
                    },
                );
            }
        });
        let Ok(mut session) = state.session.lock() else {
            let _ = app.emit(
                "scan-error",
                ScanEvent {
                    scan_id: generation,
                    data: ScanError {
                        message: "Scan state is unavailable".into(),
                    },
                },
            );
            return;
        };
        if session.generation != generation {
            return;
        }
        session.cancel = None;
        if cancel.load(Ordering::Relaxed) {
            let _ = app.emit(
                "scan-cancelled",
                ScanEvent {
                    scan_id: generation,
                    data: Cancelled {},
                },
            );
            return;
        }
        match result {
            Ok(Some(tree)) => match tree.view(0, 0, 0) {
                Ok(root) => {
                    let completed = Completed {
                        root,
                        errors: tree.errors,
                    };
                    let tree = Arc::new(tree);
                    let meta = scan_metadata(&tree);
                    session.saved = Some(meta.clone());
                    session.tree = Some(tree.clone());
                    let _ = app.emit(
                        "scan-complete",
                        ScanEvent {
                            scan_id: generation,
                            data: completed,
                        },
                    );
                    drop(session);
                    let app = app.clone();
                    std::thread::spawn(move || {
                        let result = (|| {
                            let dir = scans_dir(&app)?;
                            let state = app.state::<AppState>();
                            let _cache = state
                                .cache
                                .lock()
                                .map_err(|_| "Saved scans are unavailable".to_string())?;
                            cache::save(&dir, &tree, &meta).map_err(|e| e.to_string())
                        })();
                        match result {
                            Ok(()) => {
                                let _ = app.emit("saved-scans-changed", ());
                            }
                            Err(e) => eprintln!("Could not save scan: {e}"),
                        }
                    });
                }
                Err(error) => {
                    let _ = app.emit(
                        "scan-error",
                        ScanEvent {
                            scan_id: generation,
                            data: ScanError { message: error },
                        },
                    );
                }
            },
            Ok(None) => {
                let _ = app.emit(
                    "scan-cancelled",
                    ScanEvent {
                        scan_id: generation,
                        data: Cancelled {},
                    },
                );
            }
            Err(error) => {
                let _ = app.emit(
                    "scan-error",
                    ScanEvent {
                        scan_id: generation,
                        data: ScanError { message: error },
                    },
                );
            }
        }
    });
    Ok(generation)
}

#[tauri::command]
fn cancel_scan(state: State<'_, AppState>) -> Result<(), String> {
    let session = state
        .session
        .lock()
        .map_err(|_| "Scan state is unavailable".to_string())?;
    if let Some(cancel) = &session.cancel {
        cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
fn get_tree(
    node_id: Option<usize>,
    depth: usize,
    max_children: usize,
    state: State<'_, AppState>,
) -> Result<scanner::ViewNode, String> {
    tree(&state)?.view(node_id.unwrap_or(0), depth, max_children)
}
#[tauri::command]
async fn get_largest_files(
    node_id: usize,
    limit: usize,
    state: State<'_, AppState>,
) -> Result<Vec<scanner::LargeFile>, String> {
    let (tree, panels) = {
        let session = state
            .session
            .lock()
            .map_err(|_| "Scan state is unavailable".to_string())?;
        (
            session
                .tree
                .clone()
                .ok_or("No completed scan. Choose a folder to begin.")?,
            session.panels.clone(),
        )
    };
    tauri::async_runtime::spawn_blocking(move || {
        if node_id == 0 && limit == 50 {
            panels.largest.get_or_init(|| tree.largest(0, 50)).clone()
        } else {
            tree.largest(node_id, limit)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn get_type_breakdown(
    node_id: usize,
    state: State<'_, AppState>,
) -> Result<Vec<scanner::Breakdown>, String> {
    let (tree, panels) = {
        let session = state
            .session
            .lock()
            .map_err(|_| "Scan state is unavailable".to_string())?;
        (
            session
                .tree
                .clone()
                .ok_or("No completed scan. Choose a folder to begin.")?,
            session.panels.clone(),
        )
    };
    tauri::async_runtime::spawn_blocking(move || {
        if node_id == 0 {
            panels.breakdown.get_or_init(|| tree.breakdown(0)).clone()
        } else {
            tree.breakdown(node_id)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn get_changes(
    node_id: usize,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<changes::Changes>, String> {
    let (tree, saved, panels) = {
        let session = state
            .session
            .lock()
            .map_err(|_| "Scan state is unavailable".to_string())?;
        (
            session
                .tree
                .clone()
                .ok_or("No completed scan. Choose a folder to begin.")?,
            session.saved.clone(),
            session.panels.clone(),
        )
    };
    let dir = scans_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        tree.node(node_id)?;
        let comparison = panels.changes.get_or_init(|| {
            let Some(saved) = saved else { return Ok(None) };
            let baseline = {
                let state = app.state::<AppState>();
                let _cache = state
                    .cache
                    .lock()
                    .map_err(|_| "Saved scans are unavailable".to_string())?;
                cache::baseline(&dir, &saved)?
            };
            Ok(baseline.map(|(previous, meta)| {
                changes::Comparison::new(&tree, &previous, meta.scanned_at)
            }))
        });
        match comparison {
            Ok(Some(comparison)) => comparison.view(&tree, node_id).map(Some),
            Ok(None) => Ok(None),
            Err(error) => Err(error.clone()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn get_node_path(node_id: usize, state: State<'_, AppState>) -> Result<String, String> {
    Ok(tree(&state)?.path(node_id)?.to_string_lossy().into_owned())
}
#[tauri::command]
fn reveal_in_explorer(node_id: usize, state: State<'_, AppState>) -> Result<(), String> {
    let path = tree(&state)?.path(node_id)?;
    if !path.exists() {
        return Err("This item no longer exists. Rescan to refresh it.".into());
    }
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| e.to_string())
}
#[tauri::command]
fn open_folder(node_id: usize, state: State<'_, AppState>) -> Result<(), String> {
    let tree = tree(&state)?;
    if tree.node(node_id)?.kind != Kind::Dir {
        return Err("Choose a folder to open.".into());
    }
    let path = tree.path(node_id)?;
    if !std::fs::symlink_metadata(&path)
        .map_err(|_| "This folder has changed. Rescan to refresh it.".to_string())?
        .is_dir()
    {
        return Err("This folder has changed. Rescan to refresh it.".into());
    }
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            if cfg!(windows)
                && settings_path(app.handle())
                    .map(|path| admin::read_settings(&path).always_elevate)
                    .unwrap_or(false)
                && !admin::elevated()
                && !std::env::args_os().any(|arg| arg == "--elevated")
            {
                match admin::relaunch() {
                    Ok(()) => app.handle().exit(0),
                    Err(_) => app
                        .state::<AppState>()
                        .admin_declined
                        .store(true, Ordering::Relaxed),
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            admin_status,
            set_always_elevate,
            restart_as_admin,
            list_drives,
            list_saved_scans,
            load_saved_scan,
            clear_saved_scans,
            start_scan,
            cancel_scan,
            get_tree,
            get_largest_files,
            get_type_breakdown,
            get_changes,
            get_node_path,
            reveal_in_explorer,
            open_folder
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("Could not run DiskMapper: {error}");
    }
}

#[cfg(test)]
mod tests;
