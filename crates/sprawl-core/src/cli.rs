use crate::{
    admin,
    backend::{drive_for_path, list_drives, scan_metadata},
    cache::{self, SavedScan},
    changes::{Comparison, Removed},
    export,
    scanner::{self, Breakdown, Kind, Tree, ViewNode},
    search::Query,
};
use serde::Serialize;
use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const IDENTIFIER: &str = "com.sprawl.app";

const USAGE: &str = "\
Sprawl command line. Prints JSON reports for scripts and AI agents.

Usage:
  sprawl-cli scan <path> [--depth N] [--limit N]
      Scan a drive or folder, save the scan, and print a report.
  sprawl-cli show <path> [--depth N] [--limit N]
      Print a report for <path> from the newest saved scan that contains it.
  sprawl-cli find <path> <query> [--limit N]
      Search under <path> in the newest saved scan that contains it.
  sprawl-cli export <path> --output <file> [--format csv|json]
      Write every item under <path> from the newest saved scan to a file.
  sprawl-cli list
      List saved scans.
  sprawl-cli --version
      Print the version.

Options:
  --depth N   Folder levels to list below <path> (default 1, max 8)
  --limit N   Items per folder, extensions, largest files, grown files and search
              results (default 20)
  --output F  File to export to
  --format X  csv or json (default from the --output extension, else csv)

Search queries match names without case; every term must match:
  word          name contains word
  \"two words\"   name contains the phrase
  *.iso         wildcard match on the whole name (* and ?)
  size>1GB      logical size, with >, <, >= or <= and B, KB, MB, GB or TB
  type:video    file category: video, images, audio, documents, archives,
                code, apps or other
  kind:file     files only, or kind:dir for folders
  modified<2025-01-01  last modified UTC day, with >, <, >= or <=
                      (<= includes the whole day; > starts the next day)
  age>1y        age, with >, <, >= or <= and d, w, m (30 days), y (365 days)

Sizes are in bytes and times are Unix milliseconds. Saved scans are shared
with the Sprawl app. Run elevated to read whole NTFS drives from the master
file table, which is much faster.";

pub fn run() -> ExitCode {
    match execute(std::env::args_os().skip(1).collect()) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("sprawl-cli: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn execute(args: Vec<OsString>) -> Result<String, String> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Ok(USAGE.into());
    };
    let mut positional = vec![];
    let (mut depth, mut limit) = (1, 20);
    let (mut output, mut format) = (None, None);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(USAGE.into()),
            Some(flag @ ("--depth" | "--limit")) => {
                let value = args
                    .next()
                    .and_then(|v| v.to_str()?.parse().ok())
                    .ok_or_else(|| format!("{flag} needs a number"))?;
                if flag == "--depth" {
                    depth = value;
                } else {
                    limit = value;
                }
            }
            Some("--output") => {
                output = Some(PathBuf::from(args.next().ok_or("--output needs a file")?));
            }
            Some("--format") => {
                let value = args.next().ok_or("--format needs csv or json")?;
                format = Some(export::Format::parse(&value.to_string_lossy())?);
            }
            Some(flag) if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ => positional.push(arg),
        }
    }
    let mut positional = positional.into_iter();
    let path = positional.next().map(PathBuf::from);
    let rest: Vec<_> = positional
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let dir = scans_dir()?;
    match (command.to_str(), path, rest.is_empty()) {
        (Some("-h" | "--help" | "help"), _, _) => Ok(USAGE.into()),
        (Some("-V" | "--version"), None, _) => {
            Ok(concat!("sprawl-cli ", env!("CARGO_PKG_VERSION")).into())
        }
        (Some("list"), None, _) => json(&list(&dir)?),
        (Some("scan"), Some(path), true) => json(&scan(&dir, &path, depth, limit)?),
        (Some("show"), Some(path), true) => json(&show(&dir, &path, depth, limit)?),
        (Some("find"), Some(path), false) => json(&find(&dir, &path, &rest.join(" "), limit)?),
        (Some("export"), Some(path), true) => {
            let output = output.ok_or("export needs --output <file>")?;
            let format = format.unwrap_or_else(|| export::Format::for_path(&output));
            json(&export_scan(&dir, &path, &output, format)?)
        }
        _ => Err("invalid arguments. Run sprawl-cli --help for usage.".into()),
    }
}

fn json(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|e| e.to_string())
}

fn scans_dir() -> Result<PathBuf, String> {
    dirs::data_local_dir()
        .map(|dir| dir.join(IDENTIFIER).join("scans"))
        .ok_or_else(|| "could not find the local data folder".into())
}

fn lock(dir: &Path) -> Result<fs::File, String> {
    cache::lock(dir).map_err(|e| format!("saved scans are unavailable: {e}"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    #[serde(flatten)]
    scan: SavedScan,
    age_seconds: u64,
}

fn list(dir: &Path) -> Result<Vec<Saved>, String> {
    let _lock = lock(dir)?;
    let now = now();
    Ok(cache::list(dir)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|scan| Saved {
            age_seconds: now.saturating_sub(scan.scanned_at) / 1000,
            scan,
        })
        .collect())
}

pub(crate) fn scan(dir: &Path, path: &Path, depth: usize, limit: usize) -> Result<Report, String> {
    let path = canonical(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    #[cfg(windows)]
    let previous = scanner::drive_letter(&path).and_then(|_| {
        let _lock = lock(dir).ok()?;
        cache::load(dir, &path.to_string_lossy())
            .ok()
            .map(|(tree, _)| tree)
    });
    #[cfg(not(windows))]
    let previous = None;
    let tree = scanner::scan(&path, previous.as_ref(), &AtomicBool::new(false), |_| {})?
        .ok_or("the scan was cancelled")?;
    drop(previous);
    let meta = scan_metadata(&tree);
    let saved =
        lock(dir).and_then(|_lock| cache::save(dir, &tree, &meta).map_err(|e| e.to_string()));
    if let Err(error) = saved {
        eprintln!("sprawl-cli: could not save the scan: {error}");
    }
    let mut report = report(dir, &tree, &meta, 0, depth, limit)?;
    report.scan.elevated = Some(admin::elevated());
    Ok(report)
}

pub(crate) fn show(dir: &Path, path: &Path, depth: usize, limit: usize) -> Result<Report, String> {
    let (tree, meta, id) = open(dir, path)?;
    report(dir, &tree, &meta, id, depth, limit)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Found {
    pub path: String,
    pub query: String,
    pub total: u64,
    pub results: Vec<FoundItem>,
    pub scan: ScanInfo,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FoundItem {
    pub path: String,
    pub kind: Kind,
    pub size: u64,
    pub allocated: u64,
    pub modified: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

pub(crate) fn find(dir: &Path, path: &Path, query: &str, limit: usize) -> Result<Found, String> {
    let parsed: Query = query.parse()?;
    let (tree, meta, id) = open(dir, path)?;
    let base = tree.path(id)?;
    let matches = tree.search(id, &parsed, limit)?;
    Ok(Found {
        path: base.to_string_lossy().into_owned(),
        query: query.into(),
        total: matches.total,
        results: matches
            .results
            .into_iter()
            .map(|m| FoundItem {
                path: base.join(m.relative_path).to_string_lossy().into_owned(),
                kind: m.kind,
                size: m.size,
                allocated: m.allocated,
                modified: m.modified,
                category: m.category,
            })
            .collect(),
        scan: scan_info(&tree, &meta),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Exported {
    pub path: String,
    pub output: String,
    pub format: export::Format,
    pub items: u64,
    pub scan: ScanInfo,
}

pub(crate) fn export_scan(
    dir: &Path,
    path: &Path,
    output: &Path,
    format: export::Format,
) -> Result<Exported, String> {
    let (tree, meta, id) = open(dir, path)?;
    let output = std::path::absolute(output).map_err(|e| e.to_string())?;
    Ok(Exported {
        items: export::export(&tree, id, &output, format)?,
        path: tree.path(id)?.to_string_lossy().into_owned(),
        output: output.to_string_lossy().into_owned(),
        format,
        scan: scan_info(&tree, &meta),
    })
}

/// Loads the newest saved scan that contains `path` and finds its node.
fn open(dir: &Path, path: &Path) -> Result<(Tree, SavedScan, usize), String> {
    let target = canonical(path)
        .or_else(|_| std::path::absolute(path))
        .map_err(|e| e.to_string())?;
    let (tree, meta, parts) = {
        let _lock = lock(dir)?;
        let (saved, parts) = cache::list(dir)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find_map(|s| relative(&target, Path::new(&s.path)).map(|parts| (s, parts)))
            .ok_or_else(|| {
                format!(
                    "no saved scan contains {}. Run sprawl-cli scan first.",
                    target.display()
                )
            })?;
        let (tree, meta) = cache::load(dir, &saved.path)?;
        (tree, meta, parts)
    };
    let id = locate(&tree, &parts).ok_or_else(|| {
        format!(
            "{} isn't in the saved scan of {}. It may be newer than the scan.",
            target.display(),
            meta.path
        )
    })?;
    Ok((tree, meta, id))
}

fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    fs::canonicalize(path).map(|p| scanner::strip_verbatim_prefix(&p))
}

fn same(a: &OsStr, b: &OsStr) -> bool {
    a == b || a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

fn relative(target: &Path, root: &Path) -> Option<Vec<OsString>> {
    let mut target = target.components();
    for component in root.components() {
        if !same(component.as_os_str(), target.next()?.as_os_str()) {
            return None;
        }
    }
    Some(target.map(|c| c.as_os_str().to_owned()).collect())
}

fn locate(tree: &Tree, parts: &[OsString]) -> Option<usize> {
    let mut id = 0;
    for part in parts {
        let children = &tree.nodes[id].children;
        let name = |c: &&usize| OsStr::new(&tree.nodes[**c].name);
        id = *children
            .iter()
            .find(|c| name(c) == part)
            .or_else(|| children.iter().find(|c| same(name(c), part)))?;
    }
    Some(id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    pub path: String,
    pub kind: Kind,
    pub size: u64,
    pub allocated: u64,
    pub modified: u64,
    pub files: u64,
    pub folders: u64,
    pub scan: ScanInfo,
    /// Used drive space the scan couldn't account for, shown at drive roots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unaccounted: Option<u64>,
    pub children: Vec<Item>,
    pub largest_files: Vec<FileEntry>,
    pub types: Vec<Breakdown>,
    pub extensions: Vec<scanner::ExtensionTotal>,
    pub changes: Option<ChangeSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScanInfo {
    pub root: String,
    pub scanned_at: u64,
    pub age_seconds: u64,
    pub errors: u64,
    /// False for scans saved before allocated sizes existed; `allocated` then equals `size`.
    pub allocated_known: bool,
    pub has_modified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elevated: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Item {
    pub name: String,
    pub kind: Kind,
    pub size: u64,
    pub allocated: u64,
    pub modified: u64,
    pub files: u64,
    pub folders: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_change: Option<i64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub new: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Item>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileEntry {
    pub path: String,
    pub size: u64,
    pub allocated: u64,
    pub modified: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_size: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangeSummary {
    pub since: u64,
    pub size_change: Option<i64>,
    pub removed: Removed,
    pub grown_files: Vec<FileEntry>,
}

fn change(size: u64, previous: u64) -> i64 {
    size as i64 - previous as i64
}

fn item(node: ViewNode, comparison: Option<&Comparison>) -> Item {
    let previous = comparison.zip(node.id).map(|(c, id)| c.previous_size(id));
    Item {
        size_change: previous.flatten().map(|p| change(node.size, p)),
        new: matches!(previous, Some(None)),
        children: node
            .children
            .map(|children| children.into_iter().map(|c| item(c, comparison)).collect()),
        name: node.name,
        kind: node.kind,
        size: node.size,
        allocated: node.allocated,
        modified: node.modified,
        files: node.files,
        folders: node.folders,
    }
}

fn report(
    dir: &Path,
    tree: &Tree,
    meta: &SavedScan,
    id: usize,
    depth: usize,
    limit: usize,
) -> Result<Report, String> {
    let node = tree.node(id)?;
    let base = tree.path(id)?;
    let comparison = {
        let _lock = lock(dir)?;
        cache::baseline(dir, meta)?
    }
    .map(|(previous, baseline)| Comparison::new(tree, &previous, baseline.scanned_at));
    let changes = comparison
        .as_ref()
        .map(|c| -> Result<_, String> {
            let changes = c.view(tree, id)?;
            Ok(ChangeSummary {
                since: changes.since,
                size_change: changes.previous_size.map(|p| change(node.size, p)),
                removed: changes.removed,
                grown_files: changes
                    .grown
                    .into_iter()
                    .take(limit)
                    .map(|f| FileEntry {
                        path: base.join(f.relative_path).to_string_lossy().into_owned(),
                        size: f.size,
                        allocated: f.allocated,
                        modified: f.modified,
                        category: None,
                        previous_size: f.previous_size,
                    })
                    .collect(),
            })
        })
        .transpose()?;
    let mut types: Vec<_> = tree
        .breakdown(id)?
        .into_iter()
        .filter(|b| b.bytes > 0)
        .collect();
    types.sort_by_key(|b| std::cmp::Reverse(b.bytes));
    Ok(Report {
        path: base.to_string_lossy().into_owned(),
        kind: node.kind,
        size: node.size,
        allocated: node.allocated,
        modified: node.modified,
        files: node.files,
        folders: node.folders,
        scan: scan_info(tree, meta),
        unaccounted: (id == 0).then(|| unaccounted(tree, meta)).flatten(),
        children: item(tree.view(id, depth, limit)?, comparison.as_ref())
            .children
            .unwrap_or_default(),
        largest_files: tree
            .largest(id, limit)?
            .into_iter()
            .map(|f| FileEntry {
                path: base.join(f.relative_path).to_string_lossy().into_owned(),
                size: f.size,
                allocated: f.allocated,
                modified: f.modified,
                category: Some(f.category),
                previous_size: None,
            })
            .collect(),
        types,
        extensions: tree.extensions(id)?.into_iter().take(limit).collect(),
        changes,
    })
}

fn scan_info(tree: &Tree, meta: &SavedScan) -> ScanInfo {
    ScanInfo {
        root: meta.path.clone(),
        scanned_at: meta.scanned_at,
        age_seconds: now().saturating_sub(meta.scanned_at) / 1000,
        errors: meta.errors,
        allocated_known: tree.has_allocated,
        has_modified: tree.has_modified,
        elevated: None,
    }
}

fn unaccounted(tree: &Tree, meta: &SavedScan) -> Option<u64> {
    let drives = list_drives();
    let drive = drive_for_path(&meta.path, &drives)?;
    let trim = |p: &str| p.trim_end_matches(['\\', '/']).to_lowercase();
    if trim(&drive.mount) != trim(&meta.path) {
        return None;
    }
    let (total, free) = (meta.drive_total?, meta.drive_free?);
    let root = &tree.nodes[0];
    let found = if tree.has_allocated {
        root.allocated
    } else {
        root.size
    };
    let scanned = found + meta.recycled + meta.deleted;
    let gap = total.checked_sub(free)?.checked_sub(scanned)?;
    (gap >= (1 << 30).max(total / 100)).then_some(gap)
}
