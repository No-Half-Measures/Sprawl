use crate::{
    admin,
    cache::{self, SavedScan},
    changes, export, recycle, scanner, search, shell,
};
use scanner::{Kind, Tree};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Default)]
pub(crate) struct Session {
    pub(crate) generation: u64,
    pub(crate) cancel: Option<Arc<AtomicBool>>,
    pub(crate) tree: Option<Arc<Tree>>,
    pub(crate) saved: Option<SavedScan>,
    pub(crate) panels: Arc<RootPanels>,
}

#[derive(PartialEq, Eq)]
struct FileKey {
    node_id: usize,
    query: String,
    sort: scanner::FileSort,
    descending: bool,
}

#[derive(Default)]
pub(crate) struct RootPanels {
    pub(crate) largest: OnceLock<Result<Vec<scanner::LargeFile>, String>>,
    pub(crate) breakdown: OnceLock<Result<Vec<scanner::Breakdown>, String>>,
    extensions: OnceLock<Result<Vec<scanner::ExtensionTotal>, String>>,
    files: Mutex<Option<(FileKey, Arc<Vec<usize>>)>>,
    changes: OnceLock<Result<Option<changes::Comparison>, String>>,
}

impl Session {
    pub(crate) fn open_saved(
        &mut self,
        path: &str,
        scanned_at: u64,
    ) -> Result<Option<LoadedScan>, String> {
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
                has_allocated: tree.has_allocated,
                has_modified: tree.has_modified,
                saved: saved.clone(),
            }));
        }
        Ok(None)
    }
}
struct BackendState {
    session: Mutex<Session>,
    worker: Mutex<()>,
    deletes: Mutex<()>,
    admin_declined: AtomicBool,
    launch_path: Mutex<Option<String>>,
    launch_ready: AtomicBool,
    data_dir: PathBuf,
    events: Arc<dyn Fn(BackendEvent) + Send + Sync>,
}

fn tree(state: &BackendState) -> Result<Arc<Tree>, String> {
    state
        .session
        .lock()
        .map_err(|_| "Scan state is unavailable".to_string())?
        .tree
        .clone()
        .ok_or_else(|| "No completed scan. Choose a folder to begin.".into())
}

#[derive(Serialize)]
pub struct Drive {
    pub mount: String,
    pub label: String,
    pub total: u64,
    pub free: u64,
}

pub(crate) fn scan_metadata(tree: &Tree) -> SavedScan {
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
        recycled: 0,
        deleted: 0,
        errors: tree.errors,
        drive_free: drive.map(|d| d.free),
        drive_total: drive.map(|d| d.total),
    }
}

pub(crate) fn drive_for_path<'a>(path: &str, drives: &'a [Drive]) -> Option<&'a Drive> {
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

fn lock_cache(dir: &Path) -> Result<std::fs::File, String> {
    cache::lock(dir).map_err(|e| format!("Saved scans are unavailable: {e}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminStatus {
    pub supported: bool,
    pub elevated: bool,
    pub always_elevate: bool,
    pub declined: bool,
    pub context_menu_enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedScan {
    pub root: scanner::ViewNode,
    pub errors: u64,
    pub has_allocated: bool,
    pub has_modified: bool,
    pub saved: SavedScan,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEvent<T: Serialize + Clone> {
    pub scan_id: u64,
    #[serde(flatten)]
    pub data: T,
}

#[derive(Clone, Serialize)]
pub struct ScanError {
    pub message: String,
}
#[derive(Clone, Serialize)]
pub struct Cancelled {}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Completed {
    pub root: scanner::ViewNode,
    pub errors: u64,
    pub has_allocated: bool,
    pub has_modified: bool,
}

#[derive(Serialize)]
pub struct Deleted {
    pub root: scanner::ViewNode,
    pub recycled: u64,
    pub deleted: u64,
}

pub(crate) fn list_drives() -> Vec<Drive> {
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

#[derive(Clone)]
pub enum BackendEvent {
    ScanProgress(ScanEvent<scanner::Progress>),
    ScanComplete(ScanEvent<Completed>),
    ScanCancelled(ScanEvent<Cancelled>),
    ScanError(ScanEvent<ScanError>),
    SavedScansChanged,
    ScanRequested(String),
}
#[derive(Clone)]
pub struct Backend {
    inner: Arc<BackendState>,
}
impl Backend {
    pub fn new(
        data_dir: PathBuf,
        launch_path: Option<String>,
        events: impl Fn(BackendEvent) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(BackendState {
                data_dir,
                launch_path: Mutex::new(launch_path),
                events: Arc::new(events),
                session: Mutex::default(),
                worker: Mutex::new(()),
                deletes: Mutex::new(()),
                admin_declined: AtomicBool::new(false),
                launch_ready: AtomicBool::new(false),
            }),
        }
    }
    fn emit(&self, event: BackendEvent) {
        (self.inner.events)(event);
    }
    fn scans_dir(&self) -> PathBuf {
        self.inner.data_dir.join("scans")
    }
    fn settings_path(&self) -> PathBuf {
        self.inner.data_dir.join("settings.json")
    }
    pub fn request_scan(&self, path: String) {
        if let Ok(mut pending) = self.inner.launch_path.lock() {
            if self.inner.launch_ready.load(Ordering::Relaxed) {
                self.emit(BackendEvent::ScanRequested(path));
            } else {
                *pending = Some(path);
            }
        }
    }
    pub fn mark_admin_declined(&self) {
        self.inner.admin_declined.store(true, Ordering::Relaxed);
    }
    pub fn list_drives(&self) -> Vec<Drive> {
        list_drives()
    }

    pub fn admin_status(&self) -> AdminStatus {
        let backend = self;
        let state = &self.inner;
        let settings = admin::read_settings(&backend.settings_path());
        AdminStatus {
            supported: cfg!(windows),
            elevated: admin::elevated(),
            always_elevate: settings.always_elevate,
            declined: state.admin_declined.load(Ordering::Relaxed),
            context_menu_enabled: shell::context_menu_enabled(),
        }
    }

    pub fn set_context_menu(&self, enabled: bool) -> Result<(), String> {
        shell::set_context_menu(enabled)
    }

    pub fn take_launch_path(&self) -> Option<String> {
        let state = &self.inner;
        let mut path = state.launch_path.lock().ok()?;
        state.launch_ready.store(true, Ordering::Relaxed);
        path.take()
    }

    pub fn set_always_elevate(&self, enabled: bool) -> Result<(), String> {
        let backend = self;
        admin::write_settings(
            &backend.settings_path(),
            &admin::Settings {
                always_elevate: enabled,
            },
        )
        .map_err(|e| format!("Could not save administrator preference: {e}"))
    }

    pub fn restart_as_admin(&self) -> Result<(), String> {
        admin::relaunch()
    }

    pub fn list_saved_scans(&self) -> Result<Vec<SavedScan>, String> {
        let backend = self;
        let dir = backend.scans_dir();

        let _cache = lock_cache(&dir)?;
        cache::list(&dir).map_err(|e| e.to_string())
    }

    pub fn load_saved_scan(&self, path: String, scanned_at: u64) -> Result<LoadedScan, String> {
        let backend = self;
        let state = &self.inner;
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
        let dir = backend.scans_dir();
        let (tree, saved) = {
            let _cache = lock_cache(&dir)?;
            cache::load(&dir, &path)?
        };
        let loaded = LoadedScan {
            root: tree.view(0, 0, 0)?,
            errors: tree.errors,
            has_allocated: tree.has_allocated,
            has_modified: tree.has_modified,
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
    }

    pub fn clear_saved_scans(&self) -> Result<(), String> {
        let backend = self;
        let dir = backend.scans_dir();
        let state = &backend.inner;
        let _cache = lock_cache(&dir)?;
        cache::clear(&dir).map_err(|e| e.to_string())?;
        state
            .session
            .lock()
            .map_err(|_| "Scan state is unavailable".to_string())?
            .panels = Arc::default();
        backend.emit(BackendEvent::SavedScansChanged);
        Ok(())
    }

    pub fn start_scan(&self, path: String) -> Result<u64, String> {
        let backend = self;
        let state = &self.inner;
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
        let backend = backend.clone();
        std::thread::spawn(move || {
            drop(previous);
            let state = &backend.inner;
            let _worker = match state.worker.lock() {
                Ok(worker) => worker,
                Err(_) => {
                    backend.emit(BackendEvent::ScanError(ScanEvent {
                        scan_id: generation,
                        data: ScanError {
                            message: "Scanner state is unavailable".into(),
                        },
                    }));
                    return;
                }
            };
            if cancel.load(Ordering::Relaxed) {
                if let Ok(mut session) = state.session.lock()
                    && session.generation == generation
                {
                    session.cancel = None;
                    backend.emit(BackendEvent::ScanCancelled(ScanEvent {
                        scan_id: generation,
                        data: Cancelled {},
                    }));
                }
                return;
            }
            let path = PathBuf::from(path);
            let previous = {
                #[cfg(windows)]
                {
                    scanner::drive_letter(&scanner::strip_verbatim_prefix(&path)).and_then(|_| {
                        let dir = backend.scans_dir();
                        let _cache = cache::lock(&dir).ok()?;
                        cache::load(&dir, &path.to_string_lossy())
                            .ok()
                            .map(|(tree, _)| tree)
                    })
                }
                #[cfg(not(windows))]
                {
                    None
                }
            };
            let result = scanner::scan(&path, previous.as_ref(), &cancel, |progress| {
                if !cancel.load(Ordering::Relaxed) {
                    backend.emit(BackendEvent::ScanProgress(ScanEvent {
                        scan_id: generation,
                        data: progress,
                    }));
                }
            });
            let Ok(mut session) = state.session.lock() else {
                backend.emit(BackendEvent::ScanError(ScanEvent {
                    scan_id: generation,
                    data: ScanError {
                        message: "Scan state is unavailable".into(),
                    },
                }));
                return;
            };
            if session.generation != generation {
                return;
            }
            session.cancel = None;
            if cancel.load(Ordering::Relaxed) {
                backend.emit(BackendEvent::ScanCancelled(ScanEvent {
                    scan_id: generation,
                    data: Cancelled {},
                }));
                return;
            }
            match result {
                Ok(Some(tree)) => match tree.view(0, 0, 0) {
                    Ok(root) => {
                        let completed = Completed {
                            root,
                            errors: tree.errors,
                            has_allocated: tree.has_allocated,
                            has_modified: tree.has_modified,
                        };
                        let tree = Arc::new(tree);
                        let meta = scan_metadata(&tree);
                        session.saved = Some(meta.clone());
                        session.tree = Some(tree.clone());
                        backend.emit(BackendEvent::ScanComplete(ScanEvent {
                            scan_id: generation,
                            data: completed,
                        }));
                        drop(session);
                        let backend = backend.clone();
                        std::thread::spawn(move || {
                            let result = (|| {
                                let dir = backend.scans_dir();
                                let _cache = lock_cache(&dir)?;
                                cache::save(&dir, &tree, &meta).map_err(|e| e.to_string())
                            })();
                            match result {
                                Ok(()) => {
                                    backend.emit(BackendEvent::SavedScansChanged);
                                }
                                Err(e) => eprintln!("Could not save scan: {e}"),
                            }
                        });
                    }
                    Err(error) => {
                        backend.emit(BackendEvent::ScanError(ScanEvent {
                            scan_id: generation,
                            data: ScanError { message: error },
                        }));
                    }
                },
                Ok(None) => {
                    backend.emit(BackendEvent::ScanCancelled(ScanEvent {
                        scan_id: generation,
                        data: Cancelled {},
                    }));
                }
                Err(error) => {
                    backend.emit(BackendEvent::ScanError(ScanEvent {
                        scan_id: generation,
                        data: ScanError { message: error },
                    }));
                }
            }
        });
        Ok(generation)
    }

    pub fn cancel_scan(&self) -> Result<(), String> {
        let state = &self.inner;
        let session = state
            .session
            .lock()
            .map_err(|_| "Scan state is unavailable".to_string())?;
        if let Some(cancel) = &session.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    pub fn get_tree(
        &self,
        node_id: Option<usize>,
        depth: usize,
        max_children: usize,
    ) -> Result<scanner::ViewNode, String> {
        let state = &self.inner;
        tree(state)?.view(node_id.unwrap_or(0), depth, max_children)
    }

    pub fn get_nodes(&self, ids: Vec<usize>) -> Result<Vec<Option<scanner::ViewNode>>, String> {
        let state = &self.inner;
        let tree = tree(state)?;
        Ok(ids.into_iter().map(|id| tree.view(id, 0, 0).ok()).collect())
    }

    pub fn delete_item(
        &self,
        node_id: usize,
        permanent: bool,
        owner: usize,
    ) -> Result<Deleted, String> {
        let backend = self;
        let state = &backend.inner;
        let _delete = state
            .deletes
            .lock()
            .map_err(|_| "Delete state is unavailable".to_string())?;
        let (tree, generation, path, mut meta) = {
            let session = state
                .session
                .lock()
                .map_err(|_| "Scan state is unavailable".to_string())?;
            let tree = session
                .tree
                .clone()
                .ok_or("No completed scan. Choose a folder to begin.")?;
            let node = tree.node(node_id)?;
            if node_id == 0 {
                return Err("The scan root can't be deleted.".into());
            }
            let path = tree.path(node_id)?;
            recycle::check(&path, node.kind)?;
            let meta = session
                .saved
                .clone()
                .ok_or("Scan metadata is unavailable.")?;
            (tree, session.generation, path, meta)
        };
        let size = tree.nodes[node_id].size;
        recycle::delete_item(&path, &tree.nodes[node_id].name, owner, permanent)?;
        let mut updated = (*tree).clone();
        updated.remove(node_id)?;
        let updated = Arc::new(updated);
        let root = updated.view(0, 0, 0)?;
        meta.size = root.size;
        meta.files = root.files;
        meta.folders = root.folders;
        if permanent {
            meta.deleted += size;
        } else {
            meta.recycled += size;
        }
        let result = Deleted {
            root,
            recycled: meta.recycled,
            deleted: meta.deleted,
        };
        let panels = {
            let mut session = state
                .session
                .lock()
                .map_err(|_| "Scan state is unavailable".to_string())?;
            if session.generation != generation
                || !session
                    .tree
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &tree))
            {
                return Ok(result);
            }
            session.tree = Some(updated.clone());
            session.saved = Some(meta.clone());
            std::mem::take(&mut session.panels)
        };
        drop(panels);
        let backend = backend.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let dir = backend.scans_dir();
                let _cache = lock_cache(&dir)?;
                cache::save(&dir, &updated, &meta).map_err(|e| e.to_string())
            })();
            match result {
                Ok(()) => {
                    backend.emit(BackendEvent::SavedScansChanged);
                }
                Err(e) => eprintln!("Could not save scan: {e}"),
            }
        });
        Ok(result)
    }

    pub fn get_largest_files(
        &self,
        node_id: usize,
        limit: usize,
    ) -> Result<Vec<scanner::LargeFile>, String> {
        let state = &self.inner;
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

        if node_id == 0 && limit == 50 {
            panels.largest.get_or_init(|| tree.largest(0, 50)).clone()
        } else {
            tree.largest(node_id, limit)
        }
    }

    pub fn get_type_breakdown(&self, node_id: usize) -> Result<Vec<scanner::Breakdown>, String> {
        let state = &self.inner;
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

        if node_id == 0 {
            panels.breakdown.get_or_init(|| tree.breakdown(0)).clone()
        } else {
            tree.breakdown(node_id)
        }
    }

    pub fn get_extension_breakdown(
        &self,
        node_id: usize,
    ) -> Result<Vec<scanner::ExtensionTotal>, String> {
        let state = &self.inner;
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

        if node_id == 0 {
            panels.extensions.get_or_init(|| tree.extensions(0)).clone()
        } else {
            tree.extensions(node_id)
        }
    }

    pub fn get_changes(&self, node_id: usize) -> Result<Option<changes::Changes>, String> {
        let backend = self;
        let state = &self.inner;
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
        let dir = backend.scans_dir();

        tree.node(node_id)?;
        let comparison = panels.changes.get_or_init(|| {
            let Some(saved) = saved else { return Ok(None) };
            let baseline = {
                let _cache = lock_cache(&dir)?;
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
    }

    pub fn get_files(
        &self,
        node_id: usize,
        query: String,
        sort: String,
        descending: bool,
        offset: usize,
        limit: usize,
    ) -> Result<scanner::FilePage, String> {
        let state = &self.inner;
        let parsed: search::Query = query.parse()?;
        let sort = scanner::FileSort::parse(&sort)?;
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

        tree.node(node_id)?;
        let key = FileKey {
            node_id,
            query,
            sort,
            descending,
        };
        let cached = panels
            .files
            .lock()
            .map_err(|_| "File list is unavailable".to_string())?
            .as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, ids)| ids.clone());
        let ids = match cached {
            Some(ids) => ids,
            None => {
                let ids = Arc::new(tree.files(node_id, &parsed, sort, descending)?);
                *panels
                    .files
                    .lock()
                    .map_err(|_| "File list is unavailable".to_string())? =
                    Some((key, ids.clone()));
                ids
            }
        };
        Ok(tree.file_page(node_id, &ids, offset, limit))
    }

    pub fn search(
        &self,
        node_id: usize,
        query: String,
        limit: usize,
    ) -> Result<search::Matches, String> {
        let state = &self.inner;
        let query: search::Query = query.parse()?;
        let tree = tree(state)?;
        tree.search(node_id, &query, limit.min(1000))
    }

    pub fn export_scan(
        &self,
        node_id: usize,
        path: PathBuf,
        format: String,
    ) -> Result<u64, String> {
        let state = &self.inner;
        let format = export::Format::parse(&format)?;
        let tree = tree(state)?;
        export::export(&tree, node_id, &path, format)
    }

    pub fn get_trail(&self, node_id: usize) -> Result<Vec<scanner::ViewNode>, String> {
        let state = &self.inner;
        let tree = tree(state)?;
        tree.node(node_id)?;
        let mut ids = vec![node_id];
        while let Some(parent) = tree.nodes[ids[ids.len() - 1]].parent {
            ids.push(parent);
        }
        ids.into_iter()
            .rev()
            .map(|id| tree.view(id, 0, 0))
            .collect()
    }

    pub fn get_node_path(&self, node_id: usize) -> Result<String, String> {
        let state = &self.inner;
        Ok(tree(state)?.path(node_id)?.to_string_lossy().into_owned())
    }

    pub fn reveal_in_explorer(&self, node_id: usize) -> Result<PathBuf, String> {
        let state = &self.inner;
        let path = tree(state)?.path(node_id)?;
        if !path.exists() {
            return Err("This item no longer exists. Rescan to refresh it.".into());
        }
        Ok(path)
    }

    pub fn open_folder(&self, node_id: usize) -> Result<PathBuf, String> {
        let state = &self.inner;
        let tree = tree(state)?;
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
        Ok(path)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
