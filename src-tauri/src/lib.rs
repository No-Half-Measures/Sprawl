use sprawl_core::{
    Backend, BackendEvent, admin,
    backend::{AdminStatus, Deleted, Drive, LoadedScan},
    cache::SavedScan,
    changes, scanner, search, shell,
};
use std::path::PathBuf;
use tauri::{Emitter, Manager, State};

#[tauri::command]
fn list_drives(state: State<'_, Backend>) -> Vec<Drive> {
    let backend = state.inner().clone();
    backend.list_drives()
}

#[tauri::command]
fn admin_status(state: State<'_, Backend>) -> AdminStatus {
    let backend = state.inner().clone();
    backend.admin_status()
}

#[tauri::command]
fn set_context_menu(enabled: bool, state: State<'_, Backend>) -> Result<(), String> {
    let backend = state.inner().clone();
    backend.set_context_menu(enabled)
}

#[tauri::command]
fn take_launch_path(state: State<'_, Backend>) -> Option<String> {
    let backend = state.inner().clone();
    backend.take_launch_path()
}

#[tauri::command]
fn set_always_elevate(enabled: bool, state: State<'_, Backend>) -> Result<(), String> {
    let backend = state.inner().clone();
    backend.set_always_elevate(enabled)
}

#[tauri::command]
fn restart_as_admin(app: tauri::AppHandle, state: State<'_, Backend>) -> Result<(), String> {
    let backend = state.inner().clone();
    backend.restart_as_admin()?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
async fn list_saved_scans(state: State<'_, Backend>) -> Result<Vec<SavedScan>, String> {
    let backend = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || backend.list_saved_scans())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn load_saved_scan(
    path: String,
    scanned_at: u64,
    state: State<'_, Backend>,
) -> Result<LoadedScan, String> {
    let backend = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || backend.load_saved_scan(path, scanned_at))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn clear_saved_scans(state: State<'_, Backend>) -> Result<(), String> {
    let backend = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || backend.clear_saved_scans())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn start_scan(path: String, state: State<'_, Backend>) -> Result<u64, String> {
    let backend = state.inner().clone();
    backend.start_scan(path)
}

#[tauri::command]
fn cancel_scan(state: State<'_, Backend>) -> Result<(), String> {
    let backend = state.inner().clone();
    backend.cancel_scan()
}

#[tauri::command]
fn get_tree(
    node_id: Option<usize>,
    depth: usize,
    max_children: usize,
    state: State<'_, Backend>,
) -> Result<scanner::ViewNode, String> {
    let backend = state.inner().clone();
    backend.get_tree(node_id, depth, max_children)
}

#[tauri::command]
fn get_nodes(
    ids: Vec<usize>,
    state: State<'_, Backend>,
) -> Result<Vec<Option<scanner::ViewNode>>, String> {
    let backend = state.inner().clone();
    backend.get_nodes(ids)
}

#[tauri::command]
async fn delete_item(
    node_id: usize,
    permanent: bool,
    app: tauri::AppHandle,
    state: State<'_, Backend>,
) -> Result<Deleted, String> {
    let backend = state.inner().clone();
    #[cfg(windows)]
    let owner = app
        .get_webview_window("main")
        .ok_or("The main window is unavailable.")?
        .hwnd()
        .map_err(|e| e.to_string())?
        .0 as usize;
    #[cfg(not(windows))]
    let owner = 0;
    tauri::async_runtime::spawn_blocking(move || backend.delete_item(node_id, permanent, owner))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_largest_files(
    node_id: usize,
    limit: usize,
    state: State<'_, Backend>,
) -> Result<Vec<scanner::LargeFile>, String> {
    let backend = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || backend.get_largest_files(node_id, limit))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_type_breakdown(
    node_id: usize,
    state: State<'_, Backend>,
) -> Result<Vec<scanner::Breakdown>, String> {
    let backend = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || backend.get_type_breakdown(node_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_extension_breakdown(
    node_id: usize,
    state: State<'_, Backend>,
) -> Result<Vec<scanner::ExtensionTotal>, String> {
    let backend = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || backend.get_extension_breakdown(node_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_changes(
    node_id: usize,
    state: State<'_, Backend>,
) -> Result<Option<changes::Changes>, String> {
    let backend = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || backend.get_changes(node_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_files(
    node_id: usize,
    query: String,
    sort: String,
    descending: bool,
    offset: usize,
    limit: usize,
    state: State<'_, Backend>,
) -> Result<scanner::FilePage, String> {
    let backend = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        backend.get_files(node_id, query, sort, descending, offset, limit)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn search(
    node_id: usize,
    query: String,
    limit: usize,
    state: State<'_, Backend>,
) -> Result<search::Matches, String> {
    let backend = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || backend.search(node_id, query, limit))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn export_scan(
    node_id: usize,
    path: PathBuf,
    format: String,
    state: State<'_, Backend>,
) -> Result<u64, String> {
    let backend = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || backend.export_scan(node_id, path, format))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_trail(node_id: usize, state: State<'_, Backend>) -> Result<Vec<scanner::ViewNode>, String> {
    let backend = state.inner().clone();
    backend.get_trail(node_id)
}

#[tauri::command]
fn get_node_path(node_id: usize, state: State<'_, Backend>) -> Result<String, String> {
    let backend = state.inner().clone();
    backend.get_node_path(node_id)
}

#[tauri::command]
fn reveal_in_explorer(node_id: usize, state: State<'_, Backend>) -> Result<(), String> {
    let backend = state.inner().clone();
    tauri_plugin_opener::reveal_item_in_dir(backend.reveal_in_explorer(node_id)?)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_folder(node_id: usize, state: State<'_, Backend>) -> Result<(), String> {
    let backend = state.inner().clone();
    tauri_plugin_opener::open_path(backend.open_folder(node_id)?, None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    if let Err(error) = admin::wait_for_parent() {
        eprintln!("{error}");
        return;
    }
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
            if let Some(path) = shell::launch_path(args.into_iter().skip(1)) {
                app.state::<Backend>().request_scan(path);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let backend = app.state::<Backend>();
            #[cfg(windows)]
            if admin::elevated()
                && let Err(error) = shell::allow_scan_requests(&app.config().identifier)
            {
                eprintln!("{error}");
            }
            if cfg!(windows)
                && backend.admin_status().always_elevate
                && !admin::elevated()
                && !std::env::args_os().any(|arg| arg == "--elevated")
            {
                match backend.restart_as_admin() {
                    Ok(()) => app.handle().exit(0),
                    Err(_) => backend.mark_admin_declined(),
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            admin_status,
            set_context_menu,
            take_launch_path,
            set_always_elevate,
            restart_as_admin,
            list_drives,
            list_saved_scans,
            load_saved_scan,
            clear_saved_scans,
            start_scan,
            cancel_scan,
            get_tree,
            get_nodes,
            delete_item,
            get_largest_files,
            get_type_breakdown,
            get_extension_breakdown,
            get_files,
            get_changes,
            search,
            export_scan,
            get_trail,
            get_node_path,
            reveal_in_explorer,
            open_folder
        ])
        .build(tauri::generate_context!());
    let app = match result {
        Ok(app) => app,
        Err(error) => {
            eprintln!("Could not run Sprawl: {error}");
            return;
        }
    };
    let data_dir = match app.path().app_local_data_dir() {
        Ok(data_dir) => data_dir,
        Err(error) => {
            eprintln!("Could not run Sprawl: {error}");
            return;
        }
    };
    let handle = app.handle().clone();
    let backend = Backend::new(
        data_dir,
        shell::launch_path(std::env::args().skip(1)),
        move |event| match event {
            BackendEvent::ScanProgress(payload) => {
                let _ = handle.emit("scan-progress", payload);
            }
            BackendEvent::ScanComplete(payload) => {
                let _ = handle.emit("scan-complete", payload);
            }
            BackendEvent::ScanCancelled(payload) => {
                let _ = handle.emit("scan-cancelled", payload);
            }
            BackendEvent::ScanError(payload) => {
                let _ = handle.emit("scan-error", payload);
            }
            BackendEvent::SavedScansChanged => {
                let _ = handle.emit("saved-scans-changed", ());
            }
            BackendEvent::ScanRequested(path) => {
                let _ = handle.emit("scan-requested", path);
            }
        },
    );
    app.manage(backend);
    app.run(|_, _| {});
}
