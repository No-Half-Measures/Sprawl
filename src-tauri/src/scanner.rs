use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::BinaryHeap,
    fs,
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
        serde_json::from_str(include_str!("../../src/categories.json")).unwrap_or_default()
    })
}

pub fn category(name: &str) -> String {
    let extension = Path::new(name)
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    categories()
        .iter()
        .find(|c| c.extensions.contains(&extension))
        .map_or("other", |c| c.id.as_str())
        .into()
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Dir,
    File,
    Other,
}

#[derive(Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub kind: Kind,
    pub size: u64,
    pub files: u64,
    pub folders: u64,
    pub category: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub root_path: PathBuf,
    pub errors: u64,
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

struct Counters {
    files: AtomicU64,
    folders: AtomicU64,
    bytes: AtomicU64,
    errors: AtomicU64,
    last: Mutex<Instant>,
}

impl Counters {
    fn snapshot(&self, path: &Path) -> Progress {
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

fn excluded(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
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
    cancel: &AtomicBool,
    report: impl Fn(Progress) + Sync,
) -> Result<Option<Tree>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    if !metadata.is_dir() || excluded(&metadata) {
        return Err("Choose a regular folder or drive, not a link or junction.".into());
    }
    let root_path = strip_verbatim_prefix(&fs::canonicalize(path).map_err(|e| e.to_string())?);
    let counters = Counters {
        files: AtomicU64::new(0),
        folders: AtomicU64::new(0),
        bytes: AtomicU64::new(0),
        errors: AtomicU64::new(0),
        last: Mutex::new(Instant::now()),
    };
    let mut tree = Tree {
        nodes: vec![Node {
            name: path.file_name().map_or_else(
                || path.display().to_string(),
                |s| s.to_string_lossy().into_owned(),
            ),
            parent: None,
            children: vec![],
            kind: Kind::Dir,
            size: 0,
            files: 0,
            folders: 0,
            category: None,
        }],
        root_path: root_path.clone(),
        errors: 0,
    };
    let mut frontier = vec![(0, root_path)];
    while !frontier.is_empty() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        // Bound temporary directory listings even when a level contains millions of folders.
        let batch_len = frontier.len().min(256);
        let batch = frontier.split_off(frontier.len() - batch_len);
        let listings: Vec<_> = batch
            .par_iter()
            .map(|(parent, directory)| {
                let mut entries = Vec::new();
                match fs::read_dir(directory) {
                    Ok(reader) => {
                        for entry in reader {
                            if cancel.load(Ordering::Relaxed) {
                                break;
                            }
                            let entry = match entry {
                                Ok(e) => e,
                                Err(_) => {
                                    counters.errors.fetch_add(1, Ordering::Relaxed);
                                    continue;
                                }
                            };
                            #[cfg(windows)]
                            let metadata = entry.metadata();
                            #[cfg(not(windows))]
                            let metadata = fs::symlink_metadata(entry.path());
                            let metadata = match metadata {
                                Ok(m) => m,
                                Err(_) => {
                                    counters.errors.fetch_add(1, Ordering::Relaxed);
                                    continue;
                                }
                            };
                            if excluded(&metadata) {
                                continue;
                            }
                            let kind = if metadata.is_dir() {
                                Kind::Dir
                            } else if metadata.is_file() {
                                Kind::File
                            } else {
                                continue;
                            };
                            let size = if kind == Kind::File {
                                metadata.len()
                            } else {
                                0
                            };
                            if kind == Kind::File {
                                counters.files.fetch_add(1, Ordering::Relaxed);
                                counters.bytes.fetch_add(size, Ordering::Relaxed);
                            } else {
                                counters.folders.fetch_add(1, Ordering::Relaxed);
                            }
                            counters.report(directory, &report);
                            entries.push((
                                entry.file_name().to_string_lossy().into_owned(),
                                kind,
                                size,
                            ));
                        }
                    }
                    Err(_) => {
                        counters.errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
                counters.report(directory, &report);
                (*parent, directory, entries)
            })
            .collect();
        for (parent, directory, entries) in listings {
            for (name, kind, size) in entries {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                let id = tree.nodes.len();
                if kind == Kind::Dir {
                    frontier.push((id, directory.join(&name)));
                }
                let cat = (kind == Kind::File).then(|| category(&name));
                tree.nodes.push(Node {
                    name,
                    parent: Some(parent),
                    children: vec![],
                    kind,
                    size,
                    files: u64::from(kind == Kind::File),
                    folders: 0,
                    category: cat,
                });
                tree.nodes[parent].children.push(id);
            }
        }
    }
    let mut category_bytes = vec![[0_u64; CATEGORY_COUNT]; tree.nodes.len()];
    for id in (0..tree.nodes.len()).rev() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if tree.nodes[id].kind == Kind::File {
            if let Some(index) = categories()
                .iter()
                .position(|c| Some(&c.id) == tree.nodes[id].category.as_ref())
            {
                category_bytes[id][index] = tree.nodes[id].size;
            }
        } else {
            let dominant = category_bytes[id]
                .iter()
                .enumerate()
                .max_by_key(|(_, bytes)| *bytes)
                .map(|(index, _)| index);
            tree.nodes[id].category = Some(
                dominant
                    .and_then(|index| categories().get(index))
                    .map_or("other", |c| c.id.as_str())
                    .into(),
            );
        }
        if let Some(parent) = tree.nodes[id].parent {
            let (size, files, folders) = (
                tree.nodes[id].size,
                tree.nodes[id].files,
                tree.nodes[id].folders + u64::from(tree.nodes[id].kind == Kind::Dir),
            );
            tree.nodes[parent].size += size;
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
    tree.errors = counters.errors.load(Ordering::Relaxed);
    report(counters.snapshot(path));
    Ok(Some(tree))
}

#[derive(Clone, Serialize, Debug)]
pub struct ViewNode {
    pub id: Option<usize>,
    pub name: String,
    pub kind: Kind,
    pub size: u64,
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
        self.nodes
            .get(id)
            .ok_or_else(|| "This item is no longer available. Scan again.".into())
    }
    pub fn path(&self, id: usize) -> Result<PathBuf, String> {
        let mut parts = vec![];
        let mut current = id;
        while let Some(parent) = self.node(current)?.parent {
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
            files: node.files,
            folders: node.folders,
            category: node.category.clone(),
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
        let mut heap = BinaryHeap::new();
        self.visit(id, |i, n| {
            if n.kind == Kind::File && limit > 0 {
                heap.push(Reverse((n.size, i)));
                if heap.len() > limit {
                    heap.pop();
                }
            }
        })?;
        let base = self.path(id)?;
        let mut result = vec![];
        for Reverse((size, i)) in heap {
            let n = &self.nodes[i];
            result.push(LargeFile {
                id: i,
                name: n.name.clone(),
                relative_path: self
                    .path(i)?
                    .strip_prefix(&base)
                    .unwrap_or(Path::new(&n.name))
                    .to_string_lossy()
                    .into_owned(),
                size,
                category: n.category.clone().unwrap_or_else(|| "other".into()),
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
                && let Some(c) = &n.category
                && let Some(row) = result.iter_mut().find(|r| &r.category == c)
            {
                row.bytes += n.size;
                row.files += 1;
            }
        })?;
        Ok(result)
    }
}
