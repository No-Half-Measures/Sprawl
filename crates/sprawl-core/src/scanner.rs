use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::BinaryHeap,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const CATEGORY_COUNT: usize = 8;

#[derive(Deserialize)]
pub struct Category {
    pub id: String,
    pub extensions: Vec<String>,
}

pub fn categories() -> &'static [Category] {
    static CATEGORIES: OnceLock<Vec<Category>> = OnceLock::new();
    CATEGORIES.get_or_init(|| {
        serde_json::from_str(include_str!("../../../src/categories.json")).unwrap_or_default()
    })
}

pub fn category(name: &str) -> u8 {
    static EXTENSIONS: OnceLock<Vec<(&str, u8)>> = OnceLock::new();
    let extensions = EXTENSIONS.get_or_init(|| {
        let mut extensions: Vec<_> = categories()
            .iter()
            .enumerate()
            .flat_map(|(index, c)| c.extensions.iter().map(move |e| (e.as_str(), index as u8)))
            .collect();
        extensions.sort_unstable_by_key(|(extension, _)| *extension);
        extensions
    });
    let extension = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    extensions
        .binary_search_by(|(candidate, _)| {
            candidate
                .bytes()
                .cmp(extension.bytes().map(|b| b.to_ascii_lowercase()))
        })
        .map_or((CATEGORY_COUNT - 1) as u8, |i| extensions[i].1)
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Dir,
    File,
    Other,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub kind: Kind,
    pub size: u64,
    pub allocated: u64,
    pub modified: u64,
    pub files: u64,
    pub folders: u64,
    pub category: Option<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub root_path: PathBuf,
    pub errors: u64,
    pub records: Vec<u64>,
    pub checkpoint: Option<JournalCheckpoint>,
    /// False for trees loaded from saved scans that predate allocated sizes.
    pub has_allocated: bool,
    pub has_modified: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalCheckpoint {
    pub journal_id: u64,
    pub next_usn: i64,
}

#[derive(Clone, Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    pub errors: u64,
    pub current_path: String,
}

pub(crate) struct Counters {
    files: AtomicU64,
    folders: AtomicU64,
    bytes: AtomicU64,
    errors: AtomicU64,
    last: Mutex<Instant>,
}

impl Counters {
    pub(crate) fn new() -> Self {
        Self {
            files: AtomicU64::new(0),
            folders: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            last: Mutex::new(Instant::now()),
        }
    }
    pub(crate) fn flush(&self, counts: &mut Progress, path: &Path, report: &impl Fn(Progress)) {
        for (counter, count) in [
            (&self.files, counts.files),
            (&self.folders, counts.folders),
            (&self.bytes, counts.bytes),
            (&self.errors, counts.errors),
        ] {
            if count > 0 {
                counter.fetch_add(count, Ordering::Relaxed);
            }
        }
        *counts = Progress::default();
        self.report(path, report);
    }
    pub(crate) fn snapshot(&self, path: &Path) -> Progress {
        Progress {
            files: self.files.load(Ordering::Relaxed),
            folders: self.folders.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            current_path: path.to_string_lossy().into_owned(),
        }
    }
    fn report(&self, path: &Path, report: &impl Fn(Progress)) {
        if let Ok(mut last) = self.last.try_lock()
            && last.elapsed() >= Duration::from_millis(100)
        {
            *last = Instant::now();
            report(self.snapshot(path));
        }
    }
}

fn scan_directory<'scope>(
    scope: &rayon::Scope<'scope>,
    nodes: &'scope Mutex<Vec<Node>>,
    counters: &'scope Counters,
    cancel: &'scope AtomicBool,
    report: &'scope (impl Fn(Progress) + Sync),
    parent: usize,
    directory: PathBuf,
) {
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let mut entries = Vec::new();
    let mut counts = Progress::default();
    let flush = |entries: &mut Vec<Node>, counts: &mut Progress| {
        counters.flush(counts, &directory, report);
        if cancel.load(Ordering::Relaxed) || entries.is_empty() {
            return;
        }
        let directories: Vec<_> = entries
            .iter()
            .enumerate()
            .filter(|(_, node)| node.kind == Kind::Dir)
            .map(|(offset, node)| (offset, directory.join(&node.name)))
            .collect();
        let start = {
            let mut nodes = nodes.lock().unwrap();
            let start = nodes.len();
            nodes[parent].children.extend(start..start + entries.len());
            nodes.append(entries);
            start
        };
        for (offset, path) in directories {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            scope.spawn(move |scope| {
                scan_directory(scope, nodes, counters, cancel, report, start + offset, path)
            });
        }
    };
    match list_directory(&directory) {
        Ok(reader) => {
            for (index, entry) in reader.enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                if index > 0 && index % 1024 == 0 {
                    flush(&mut entries, &mut counts);
                }
                let Entry {
                    name,
                    kind,
                    size,
                    allocated,
                    modified,
                } = match entry {
                    Ok(Some(entry)) => entry,
                    Ok(None) => continue,
                    Err(_) => {
                        counts.errors += 1;
                        continue;
                    }
                };
                if kind == Kind::File {
                    counts.files += 1;
                    counts.bytes += size;
                } else {
                    counts.folders += 1;
                }
                let category = (kind == Kind::File).then(|| category(&name));
                entries.push(Node {
                    name,
                    parent: Some(parent),
                    children: vec![],
                    kind,
                    size,
                    allocated,
                    modified,
                    files: u64::from(kind == Kind::File),
                    folders: 0,
                    category,
                });
            }
        }
        Err(_) => counts.errors += 1,
    }
    flush(&mut entries, &mut counts);
}

pub(crate) fn excluded(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

struct Entry {
    name: String,
    kind: Kind,
    size: u64,
    allocated: u64,
    modified: u64,
}

/// Lists a directory, yielding `None` for entries the scan skips, such as links.
/// Folders report zero for both sizes.
#[cfg(not(windows))]
fn list_directory(directory: &Path) -> io::Result<impl Iterator<Item = io::Result<Option<Entry>>>> {
    use std::os::unix::fs::MetadataExt;

    Ok(fs::read_dir(directory)?.map(|entry| {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        let kind = if excluded(&metadata) {
            return Ok(None);
        } else if metadata.is_dir() {
            Kind::Dir
        } else if metadata.is_file() {
            Kind::File
        } else {
            return Ok(None);
        };
        let file = kind == Kind::File;
        Ok(Some(Entry {
            name: entry.file_name().to_string_lossy().into_owned(),
            kind,
            size: if file { metadata.len() } else { 0 },
            allocated: if file { metadata.blocks() * 512 } else { 0 },
            modified: if file {
                metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_millis() as u64)
            } else {
                0
            },
        }))
    }))
}

/// Lists a directory in bulk with `FileFullDirectoryInfo`, which carries sizes,
/// attributes and reparse tags for every entry without opening each file.
#[cfg(windows)]
fn list_directory(directory: &Path) -> io::Result<Entries> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY,
    };

    Ok(Entries {
        directory: fs::OpenOptions::new()
            .access_mode(FILE_LIST_DIRECTORY)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(directory)?,
        buffer: vec![0; 8192],
        next: None,
        done: false,
    })
}

#[cfg(windows)]
struct Entries {
    directory: fs::File,
    buffer: Vec<u64>,
    next: Option<usize>,
    done: bool,
}

#[cfg(windows)]
impl Iterator for Entries {
    type Item = io::Result<Option<Entry>>;

    fn next(&mut self) -> Option<Self::Item> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::ERROR_NO_MORE_FILES,
            Storage::FileSystem::{
                FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FULL_DIR_INFO,
                FileFullDirectoryInfo, GetFileInformationByHandleEx,
            },
        };

        loop {
            let offset = match self.next {
                Some(offset) => offset,
                None if self.done => return None,
                None => {
                    // SAFETY: the handle is an open directory and the buffer is writable
                    // for its full length.
                    let ok = unsafe {
                        GetFileInformationByHandleEx(
                            self.directory.as_raw_handle(),
                            FileFullDirectoryInfo,
                            self.buffer.as_mut_ptr().cast(),
                            (self.buffer.len() * 8) as u32,
                        )
                    };
                    if ok == 0 {
                        self.done = true;
                        let error = io::Error::last_os_error();
                        return (error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32))
                            .then_some(Err(error));
                    }
                    0
                }
            };
            // SAFETY: the call above filled the buffer with 8-byte aligned entries chained
            // by NextEntryOffset, each followed by its FileNameLength-byte name.
            let (info, name) = unsafe {
                let info = self
                    .buffer
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<FILE_FULL_DIR_INFO>();
                let name = std::slice::from_raw_parts(
                    (&raw const (*info).FileName).cast::<u16>(),
                    (*info).FileNameLength as usize / 2,
                );
                (&*info, name)
            };
            self.next = (info.NextEntryOffset != 0).then(|| offset + info.NextEntryOffset as usize);
            if name == [u16::from(b'.')] || name == [u16::from(b'.'); 2] {
                continue;
            }
            // Same test as std's is_symlink, so cloud placeholders and dedup files stay.
            // For reparse points, EaSize holds the reparse tag.
            if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
                && info.EaSize & 0x20000000 != 0
            {
                return Some(Ok(None));
            }
            let file = info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0;
            return Some(Ok(Some(Entry {
                name: String::from_utf16_lossy(name),
                kind: if file { Kind::File } else { Kind::Dir },
                size: if file { info.EndOfFile as u64 } else { 0 },
                allocated: if file { info.AllocationSize as u64 } else { 0 },
                modified: if file && info.LastWriteTime > 0 {
                    filetime_ms(info.LastWriteTime as u64)
                } else {
                    0
                },
            })));
        }
    }
}

pub fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};

        let mut components = path.components();
        if let Some(Component::Prefix(prefix)) = components.next() {
            let mut normal = match prefix.kind() {
                Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:\\", char::from(drive))),
                Prefix::VerbatimUNC(server, share) => {
                    let mut root = PathBuf::from(r"\\");
                    root.push(server);
                    root.push(share);
                    root
                }
                _ => return path.to_owned(),
            };
            for component in components {
                if component != Component::RootDir {
                    normal.push(component.as_os_str());
                }
            }
            return normal;
        }
    }
    path.to_owned()
}

pub fn scan(
    path: &Path,
    previous: Option<&Tree>,
    cancel: &AtomicBool,
    report: impl Fn(Progress) + Sync,
) -> Result<Option<Tree>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    if !metadata.is_dir() || excluded(&metadata) {
        return Err("Choose a regular folder or drive, not a link or junction.".into());
    }
    let root_path = strip_verbatim_prefix(&fs::canonicalize(path).map_err(|e| e.to_string())?);
    #[cfg(windows)]
    if drive_letter(&root_path).is_some() {
        if let Some(previous) = previous
            && previous
                .root_path
                .to_string_lossy()
                .eq_ignore_ascii_case(&root_path.to_string_lossy())
            && let Ok(result) = crate::mft::rescan_volume(&root_path, previous, cancel, &report)
        {
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            return Ok(result.map(|mut tree| {
                tree.nodes[0].name = root_name(path);
                tree
            }));
        }
        let result = crate::mft::scan_volume(&root_path, cancel, &report);
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        match result {
            Ok(Some(mut tree)) => {
                tree.nodes[0].name = root_name(path);
                return Ok(Some(tree));
            }
            Ok(None) => return Ok(None),
            Err(_) => {}
        }
    }
    #[cfg(not(windows))]
    let _ = previous;
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    Ok(scan_walk(path, root_path, cancel, report))
}

#[cfg(windows)]
pub(crate) fn drive_letter(path: &Path) -> Option<u8> {
    use std::path::{Component, Prefix};

    let mut components = path.components();
    match (components.next(), components.next(), components.next()) {
        (Some(Component::Prefix(prefix)), Some(Component::RootDir), None) => match prefix.kind() {
            Prefix::Disk(letter) => Some(letter),
            _ => None,
        },
        _ => None,
    }
}

fn root_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |s| s.to_string_lossy().into_owned(),
    )
}

fn scan_walk(
    path: &Path,
    root_path: PathBuf,
    cancel: &AtomicBool,
    report: impl Fn(Progress) + Sync,
) -> Option<Tree> {
    let counters = Counters::new();
    let mut tree = Tree {
        nodes: vec![Node {
            name: root_name(path),
            parent: None,
            children: vec![],
            kind: Kind::Dir,
            size: 0,
            allocated: 0,
            modified: 0,
            files: 0,
            folders: 0,
            category: None,
        }],
        root_path: root_path.clone(),
        errors: 0,
        records: vec![],
        checkpoint: None,
        has_allocated: true,
        has_modified: true,
    };
    let nodes = Mutex::new(std::mem::take(&mut tree.nodes));
    rayon::scope(|scope| {
        scan_directory(scope, &nodes, &counters, cancel, &report, 0, root_path);
    });
    tree.nodes = nodes.into_inner().unwrap();
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    if !finish(&mut tree, cancel) {
        return None;
    }
    tree.errors = counters.errors.load(Ordering::Relaxed);
    report(counters.snapshot(path));
    Some(tree)
}

pub(crate) fn finish(tree: &mut Tree, cancel: &AtomicBool) -> bool {
    let mut category_bytes = vec![[0_u64; CATEGORY_COUNT]; tree.nodes.len()];
    for id in (0..tree.nodes.len()).rev() {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        if tree.nodes[id].kind == Kind::File {
            if let Some(index) = tree.nodes[id].category {
                category_bytes[id][usize::from(index)] = tree.nodes[id].size;
            }
        } else {
            tree.nodes[id].category = category_bytes[id]
                .iter()
                .enumerate()
                .max_by_key(|(_, bytes)| *bytes)
                .map(|(index, _)| index as u8);
        }
        if let Some(parent) = tree.nodes[id].parent {
            let (size, allocated, files, folders) = (
                tree.nodes[id].size,
                tree.nodes[id].allocated,
                tree.nodes[id].files,
                tree.nodes[id].folders + u64::from(tree.nodes[id].kind == Kind::Dir),
            );
            tree.nodes[parent].size += size;
            tree.nodes[parent].allocated += allocated;
            tree.nodes[parent].modified = tree.nodes[parent].modified.max(tree.nodes[id].modified);
            tree.nodes[parent].files += files;
            tree.nodes[parent].folders += folders;
            for (index, bytes) in category_bytes[id].into_iter().enumerate() {
                category_bytes[parent][index] += bytes;
            }
        }
        let mut children = std::mem::take(&mut tree.nodes[id].children);
        children.sort_unstable_by(|a, b| {
            tree.nodes[*b]
                .size
                .cmp(&tree.nodes[*a].size)
                .then_with(|| tree.nodes[*a].name.cmp(&tree.nodes[*b].name))
        });
        tree.nodes[id].children = children;
    }
    drop(category_bytes);
    if cancel.load(Ordering::Relaxed) {
        return false;
    }
    true
}

#[derive(Clone, Serialize, Debug)]
pub struct ViewNode {
    pub id: Option<usize>,
    pub name: String,
    pub kind: Kind,
    pub size: u64,
    pub allocated: u64,
    pub modified: u64,
    pub files: u64,
    pub folders: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<ViewNode>>,
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LargeFile {
    pub id: usize,
    pub name: String,
    pub relative_path: String,
    pub size: u64,
    pub allocated: u64,
    pub modified: u64,
    pub category: String,
}

#[derive(Clone, Serialize, Debug)]
pub struct Breakdown {
    pub category: String,
    pub bytes: u64,
    pub files: u64,
}

impl Tree {
    pub fn node(&self, id: usize) -> Result<&Node, String> {
        let node = self
            .nodes
            .get(id)
            .ok_or_else(|| "This item is no longer available. Scan again.".to_string())?;
        let mut current = id;
        while let Some(parent) = self.nodes[current].parent {
            current = parent;
        }
        if current != 0 {
            return Err("This item was deleted.".into());
        }
        Ok(node)
    }
    pub fn remove(&mut self, id: usize) -> Result<(), String> {
        let node = self.node(id)?;
        let Some(mut parent) = node.parent else {
            return Err("The scan root can't be deleted.".into());
        };
        let (size, allocated, files, folders) = (
            node.size,
            node.allocated,
            node.files,
            node.folders + u64::from(node.kind == Kind::Dir),
        );
        self.nodes[parent].children.retain(|&child| child != id);
        self.nodes[id].parent = None;
        loop {
            self.nodes[parent].size -= size;
            self.nodes[parent].allocated -= allocated;
            self.nodes[parent].modified = self.nodes[parent]
                .children
                .iter()
                .map(|&i| self.nodes[i].modified)
                .max()
                .unwrap_or(0);
            self.nodes[parent].files -= files;
            self.nodes[parent].folders -= folders;
            let Some(ancestor) = self.nodes[parent].parent else {
                break;
            };
            let mut children = std::mem::take(&mut self.nodes[ancestor].children);
            let position = children.iter().position(|&child| child == parent).unwrap();
            children.remove(position);
            let position = children.partition_point(|&child| {
                self.nodes[child].size > self.nodes[parent].size
                    || (self.nodes[child].size == self.nodes[parent].size
                        && self.nodes[child].name < self.nodes[parent].name)
            });
            children.insert(position, parent);
            self.nodes[ancestor].children = children;
            parent = ancestor;
        }
        Ok(())
    }
    pub fn path(&self, id: usize) -> Result<PathBuf, String> {
        self.node(id)?;
        let mut parts = vec![];
        let mut current = id;
        while let Some(parent) = self.nodes[current].parent {
            parts.push(&self.nodes[current].name);
            current = parent;
        }
        let mut path = self.root_path.clone();
        for name in parts.into_iter().rev() {
            path.push(name);
        }
        Ok(path)
    }
    pub fn view(&self, id: usize, depth: usize, max_children: usize) -> Result<ViewNode, String> {
        self.node(id)?;
        Ok(self.view_inner(id, depth.min(8), max_children.min(500), &mut 8000))
    }
    fn view_inner(
        &self,
        id: usize,
        depth: usize,
        max_children: usize,
        budget: &mut usize,
    ) -> ViewNode {
        *budget = budget.saturating_sub(1);
        let node = &self.nodes[id];
        let children = if node.kind == Kind::Dir && depth > 0 {
            let mut children = vec![];
            let mut used = 0;
            for child in node.children.iter().take(max_children) {
                if *budget <= 1 {
                    break;
                }
                children.push(self.view_inner(*child, depth - 1, max_children, budget));
                used += 1;
            }
            if used < node.children.len() {
                let rest = &node.children[used..];
                children.push(ViewNode {
                    id: None,
                    name: format!("{} smaller items", rest.len()),
                    kind: Kind::Other,
                    size: rest.iter().map(|i| self.nodes[*i].size).sum(),
                    allocated: rest.iter().map(|i| self.nodes[*i].allocated).sum(),
                    modified: rest
                        .iter()
                        .map(|i| self.nodes[*i].modified)
                        .max()
                        .unwrap_or(0),
                    files: rest.iter().map(|i| self.nodes[*i].files).sum(),
                    folders: rest
                        .iter()
                        .map(|i| {
                            self.nodes[*i].folders + u64::from(self.nodes[*i].kind == Kind::Dir)
                        })
                        .sum(),
                    category: Some("other".into()),
                    children: None,
                });
                *budget = budget.saturating_sub(1);
            }
            Some(children)
        } else {
            None
        };
        ViewNode {
            id: Some(id),
            name: node.name.clone(),
            kind: node.kind,
            size: node.size,
            allocated: node.allocated,
            modified: node.modified,
            files: node.files,
            folders: node.folders,
            category: node
                .category
                .map(|index| categories()[usize::from(index)].id.clone()),
            children,
        }
    }
    fn visit(&self, id: usize, mut visitor: impl FnMut(usize, &Node)) -> Result<(), String> {
        self.node(id)?;
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            let node = &self.nodes[id];
            visitor(id, node);
            stack.extend(&node.children);
        }
        Ok(())
    }
    pub fn largest(&self, id: usize, limit: usize) -> Result<Vec<LargeFile>, String> {
        let limit = limit.min(1000);
        let mut heap: BinaryHeap<(Reverse<u64>, String, usize)> = BinaryHeap::new();
        let base = self.path(id)?;
        let mut stack = vec![id];
        while let Some(i) = stack.pop() {
            let n = &self.nodes[i];
            stack.extend(&n.children);
            if n.kind == Kind::File
                && limit > 0
                && heap
                    .peek()
                    .is_none_or(|(Reverse(size), _, _)| heap.len() < limit || n.size >= *size)
            {
                let relative_path = self
                    .path(i)?
                    .strip_prefix(&base)
                    .unwrap_or(Path::new(&n.name))
                    .to_string_lossy()
                    .into_owned();
                heap.push((Reverse(n.size), relative_path, i));
                if heap.len() > limit {
                    heap.pop();
                }
            }
        }
        let mut result = vec![];
        for (Reverse(size), relative_path, i) in heap {
            let n = &self.nodes[i];
            result.push(LargeFile {
                id: i,
                name: n.name.clone(),
                relative_path,
                size,
                allocated: n.allocated,
                modified: n.modified,
                category: n
                    .category
                    .map_or("other", |index| {
                        categories()[usize::from(index)].id.as_str()
                    })
                    .into(),
            });
        }
        result.sort_unstable_by(|a, b| {
            b.size
                .cmp(&a.size)
                .then_with(|| a.relative_path.cmp(&b.relative_path))
        });
        Ok(result)
    }
    pub fn breakdown(&self, id: usize) -> Result<Vec<Breakdown>, String> {
        let mut result: Vec<_> = categories()
            .iter()
            .map(|c| Breakdown {
                category: c.id.clone(),
                bytes: 0,
                files: 0,
            })
            .collect();
        self.visit(id, |_, n| {
            if n.kind == Kind::File
                && let Some(index) = n.category
                && let Some(row) = result.get_mut(usize::from(index))
            {
                row.bytes += n.size;
                row.files += 1;
            }
        })?;
        Ok(result)
    }
}

pub(crate) fn filetime_ms(ticks: u64) -> u64 {
    ticks.saturating_sub(116_444_736_000_000_000) / 10_000
}

#[derive(Clone, Debug, Serialize)]
pub struct ExtensionTotal {
    pub extension: String,
    pub category: String,
    pub bytes: u64,
    pub allocated: u64,
    pub files: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileSort {
    Size,
    Name,
    Modified,
}
impl FileSort {
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "size" => Ok(Self::Size),
            "name" => Ok(Self::Name),
            "modified" => Ok(Self::Modified),
            _ => Err(format!(
                "Unknown file sort \"{text}\". Use size, name or modified."
            )),
        }
    }
}

#[derive(Serialize)]
pub struct FilePage {
    pub total: usize,
    pub files: Vec<LargeFile>,
}

impl Tree {
    pub fn extensions(&self, id: usize) -> Result<Vec<ExtensionTotal>, String> {
        let mut totals = std::collections::HashMap::<String, ExtensionTotal>::new();
        self.visit(id, |_, node| {
            if node.kind != Kind::File {
                return;
            }
            let extension = Path::new(&node.name)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_lowercase();
            let row = totals
                .entry(extension.clone())
                .or_insert_with(|| ExtensionTotal {
                    category: categories()[usize::from(category(&node.name))].id.clone(),
                    extension,
                    bytes: 0,
                    allocated: 0,
                    files: 0,
                });
            row.bytes += node.size;
            row.allocated += node.allocated;
            row.files += 1;
        })?;
        let mut rows: Vec<_> = totals.into_values().collect();
        rows.sort_unstable_by(|a, b| {
            b.bytes
                .cmp(&a.bytes)
                .then_with(|| a.extension.cmp(&b.extension))
        });
        Ok(rows)
    }

    pub fn files(
        &self,
        id: usize,
        query: &crate::search::Query,
        sort: FileSort,
        descending: bool,
    ) -> Result<Vec<usize>, String> {
        let mut rows = vec![];
        let mut lower = String::new();
        self.visit(id, |i, node| {
            if node.kind == Kind::File && query.matches(node, &mut lower) {
                rows.push(i);
            }
        })?;
        let names: Vec<_> = if sort == FileSort::Name {
            rows.iter()
                .map(|&i| self.nodes[i].name.to_lowercase())
                .collect()
        } else {
            vec![]
        };
        let mut order: Vec<_> = (0..rows.len()).collect();
        order.sort_unstable_by(|&a, &b| {
            let na = &self.nodes[rows[a]];
            let nb = &self.nodes[rows[b]];
            let primary = match sort {
                FileSort::Size => na.size.cmp(&nb.size),
                FileSort::Name => names[a].cmp(&names[b]),
                FileSort::Modified => na.modified.cmp(&nb.modified),
            };
            let primary = if descending {
                primary.reverse()
            } else {
                primary
            };
            primary
                .then_with(|| {
                    if sort == FileSort::Modified {
                        nb.size.cmp(&na.size)
                    } else {
                        std::cmp::Ordering::Equal
                    }
                })
                .then_with(|| na.name.cmp(&nb.name))
                .then_with(|| rows[a].cmp(&rows[b]))
        });
        Ok(order.into_iter().map(|i| rows[i]).collect())
    }

    pub fn file_page(&self, base: usize, ids: &[usize], offset: usize, limit: usize) -> FilePage {
        let files = ids
            .iter()
            .skip(offset)
            .take(limit.min(1000))
            .filter_map(|&id| {
                let node = self.node(id).ok()?;
                (node.kind == Kind::File).then(|| LargeFile {
                    id,
                    name: node.name.clone(),
                    relative_path: self.relative_path(base, id),
                    size: node.size,
                    allocated: node.allocated,
                    modified: node.modified,
                    category: categories()
                        [usize::from(node.category.unwrap_or(category(&node.name)))]
                    .id
                    .clone(),
                })
            })
            .collect();
        FilePage {
            total: ids.len(),
            files,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn only_local_drive_roots_use_mft() {
        assert_eq!(drive_letter(Path::new(r"C:\")), Some(b'C'));
        for path in [
            r"C:\Windows",
            r"\\server\share\",
            r"\server\share\",
            "relative",
            "C:",
            r"\",
            r"\\?\C:\",
        ] {
            assert_eq!(drive_letter(Path::new(path)), None, "{path}");
        }
        assert_eq!(root_name(Path::new(r"c:/")), "c:/");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Scans a whole drive then replays its journal; run elevated with --nocapture"]
    fn incremental_matches_full_scan() {
        let root_path =
            PathBuf::from(std::env::var_os("SPRAWL_MFT_DRIVE").unwrap_or_else(|| r"C:\".into()));
        let cancel = AtomicBool::new(false);
        let start = Instant::now();
        let full = crate::mft::scan_volume(&root_path, &cancel, &|_| {})
            .unwrap_or_else(|error| panic!("MFT scan failed; run elevated: {error}"))
            .expect("Full scan cancelled");
        let full_time = start.elapsed();
        assert!(
            full.checkpoint.is_some(),
            "An active NTFS journal is required"
        );
        let start = Instant::now();
        let incremental = crate::mft::rescan_volume(&root_path, &full, &cancel, &|_| {})
            .unwrap_or_else(|error| panic!("Incremental scan unavailable: {error}"))
            .expect("Incremental scan cancelled");
        let incremental_time = start.elapsed();
        for (label, tree, elapsed) in [
            ("Full", &full, full_time),
            ("Incremental", &incremental, incremental_time),
        ] {
            let root = &tree.nodes[0];
            eprintln!(
                "{label}: {elapsed:?}, files={}, folders={}, size={}",
                root.files, root.folders, root.size
            );
        }
        let before = &full.nodes[0];
        let after = &incremental.nodes[0];
        for (expected, actual, tolerance) in [
            (before.files, after.files, 100),
            (before.folders, after.folders, 20),
            (before.size, after.size, 128 * 1024 * 1024),
        ] {
            assert!(
                expected.abs_diff(actual) <= (expected / 100).max(tolerance),
                "Full={expected}, incremental={actual}"
            );
        }
        assert_eq!(incremental.records.len(), incremental.nodes.len());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Scans a whole drive twice; run elevated with --nocapture"]
    fn mft_matches_directory_scan() {
        let path =
            PathBuf::from(std::env::var_os("SPRAWL_MFT_DRIVE").unwrap_or_else(|| r"C:\".into()));
        let root_path = strip_verbatim_prefix(&fs::canonicalize(&path).unwrap());
        assert!(
            drive_letter(&root_path).is_some(),
            "SPRAWL_MFT_DRIVE must be a local drive root"
        );
        let cancel = AtomicBool::new(false);
        let start = Instant::now();
        let mft = crate::mft::scan_volume(&root_path, &cancel, &|_| {})
            .unwrap_or_else(|error| panic!("MFT scan failed; run elevated: {error}"))
            .expect("MFT scan was cancelled");
        let mft_time = start.elapsed();
        let start = Instant::now();
        let walk = scan_walk(&path, root_path, &cancel, |_| {}).unwrap();
        let walk_time = start.elapsed();
        for (label, tree, elapsed) in [("MFT", &mft, mft_time), ("Directory", &walk, walk_time)] {
            let root = &tree.nodes[0];
            eprintln!(
                "{label}: {elapsed:?}, files={}, folders={}, size={}, allocated={}, errors={}",
                root.files, root.folders, root.size, root.allocated, tree.errors
            );
        }
        eprintln!(
            "MFT - directory: files={}, folders={}, size={}, allocated={}, errors={}",
            i128::from(mft.nodes[0].files) - i128::from(walk.nodes[0].files),
            i128::from(mft.nodes[0].folders) - i128::from(walk.nodes[0].folders),
            i128::from(mft.nodes[0].size) - i128::from(walk.nodes[0].size),
            i128::from(mft.nodes[0].allocated) - i128::from(walk.nodes[0].allocated),
            i128::from(mft.errors) - i128::from(walk.errors)
        );
    }

    #[test]
    fn progress_can_cancel_a_large_directory_before_assembly() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..1100 {
            fs::write(dir.path().join(format!("file{i}")), [0; 3]).unwrap();
        }
        let nodes = Mutex::new(vec![Node {
            name: "root".into(),
            parent: None,
            children: vec![],
            kind: Kind::Dir,
            size: 0,
            allocated: 0,
            modified: 0,
            files: 0,
            folders: 0,
            category: None,
        }]);
        let counters = Counters {
            files: AtomicU64::new(0),
            folders: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            last: Mutex::new(Instant::now() - Duration::from_millis(100)),
        };
        let cancel = AtomicBool::new(false);
        let report = |progress: Progress| {
            assert_eq!((progress.files, progress.bytes), (1024, 3072));
            assert_eq!(progress.current_path, dir.path().to_string_lossy());
            cancel.store(true, Ordering::Relaxed);
        };
        rayon::scope(|scope| {
            scan_directory(
                scope,
                &nodes,
                &counters,
                &cancel,
                &report,
                0,
                dir.path().into(),
            );
        });
        assert!(cancel.load(Ordering::Relaxed));
        assert_eq!(counters.files.load(Ordering::Relaxed), 1024);
        assert_eq!(nodes.into_inner().unwrap().len(), 1);
    }
}
