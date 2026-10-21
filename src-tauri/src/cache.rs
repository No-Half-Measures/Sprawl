use crate::scanner::{Kind, Node, Tree, categories};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    cmp::Reverse,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Write},
    path::Path,
};

const MAGIC: &[u8; 8] = b"DISKMAP\0";
pub const FORMAT_VERSION: u32 = 2;
const OPEN_ERROR: &str = "This saved scan can't be opened. Scan again to refresh it.";

#[derive(Serialize, Deserialize)]
struct DiskNode<'a> {
    name: Cow<'a, str>,
    parent: Option<usize>,
    children: Cow<'a, [usize]>,
    kind: Kind,
    size: u64,
    files: u64,
    folders: u64,
    category: Option<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedScan {
    pub version: u32,
    pub path: String,
    pub scanned_at: u64,
    pub size: u64,
    pub files: u64,
    pub folders: u64,
    pub errors: u64,
    pub drive_free: Option<u64>,
    pub drive_total: Option<u64>,
}

pub fn key(path: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in path.trim_end_matches(['\\', '/']).to_lowercase().bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn header(reader: &mut impl Read) -> io::Result<()> {
    let mut magic = [0; 8];
    let mut version = [0; 4];
    reader.read_exact(&mut magic)?;
    reader.read_exact(&mut version)?;
    if &magic != MAGIC || u32::from_le_bytes(version) != FORMAT_VERSION {
        return Err(io::Error::new(io::ErrorKind::InvalidData, OPEN_ERROR));
    }
    Ok(())
}

fn remove(dir: &Path, key: &str) {
    for extension in ["json", "tree"] {
        let _ = fs::remove_file(dir.join(format!("{key}.{extension}")));
    }
}

pub fn list(dir: &Path) -> io::Result<Vec<SavedScan>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let paths: Vec<_> = entries
        .map(|e| e.map(|e| e.path()))
        .collect::<io::Result<_>>()?;
    let mut saved = vec![];
    for path in &paths {
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let meta = File::open(path).and_then(|f| {
            serde_json::from_reader::<_, SavedScan>(BufReader::new(f)).map_err(io::Error::other)
        });
        match meta {
            Ok(meta)
                if meta.version == FORMAT_VERSION
                    && key(&meta.path) == stem
                    && File::open(path.with_extension("tree"))
                        .and_then(|mut f| header(&mut f))
                        .is_ok() =>
            {
                saved.push(meta)
            }
            _ => remove(dir, &stem),
        }
    }
    for path in paths {
        if path.extension().is_some_and(|e| e == "tmp")
            || (path.extension().is_some_and(|e| e == "tree")
                && !path.with_extension("json").is_file())
        {
            let _ = fs::remove_file(path);
        }
    }
    saved.sort_by_key(|s| Reverse(s.scanned_at));
    Ok(saved)
}

pub fn save(dir: &Path, tree: &Tree, meta: &SavedScan) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let saved = list(dir)?;
    let key = key(&meta.path);
    if saved
        .iter()
        .any(|s| self::key(&s.path) == key && s.scanned_at > meta.scanned_at)
    {
        return Ok(());
    }
    let temp = dir.join(format!("{key}.tree.tmp"));
    let mut writer = BufWriter::new(File::create(&temp)?);
    writer.write_all(MAGIC)?;
    writer.write_all(&FORMAT_VERSION.to_le_bytes())?;
    let mut encoder = zstd::stream::write::Encoder::new(writer, 3)?;
    encoder.include_checksum(true)?;
    let mut writer = BufWriter::new(encoder);
    bincode::serde::encode_into_std_write(
        (tree.nodes.len(), &tree.root_path, tree.errors),
        &mut writer,
        bincode::config::standard(),
    )
    .map_err(io::Error::other)?;
    for node in &tree.nodes {
        if node
            .category
            .is_some_and(|index| usize::from(index) >= categories().len())
        {
            return Err(io::Error::new(io::ErrorKind::InvalidData, OPEN_ERROR));
        }
        bincode::serde::encode_into_std_write(
            DiskNode {
                name: Cow::Borrowed(&node.name),
                parent: node.parent,
                children: Cow::Borrowed(&node.children),
                kind: node.kind,
                size: node.size,
                files: node.files,
                folders: node.folders,
                category: node.category,
            },
            &mut writer,
            bincode::config::standard(),
        )
        .map_err(io::Error::other)?;
    }
    let mut writer = writer.into_inner().map_err(|e| e.into_error())?.finish()?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    drop(writer);
    let meta_path = dir.join(format!("{key}.json"));
    match fs::remove_file(&meta_path) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e),
    }
    fs::rename(temp, dir.join(format!("{key}.tree")))?;
    let temp = dir.join(format!("{key}.json.tmp"));
    let mut writer = BufWriter::new(File::create(&temp)?);
    serde_json::to_writer(&mut writer, meta).map_err(io::Error::other)?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    drop(writer);
    fs::rename(temp, meta_path)?;
    for old in list(dir)?.iter().skip(10) {
        remove(dir, &self::key(&old.path));
    }
    Ok(())
}

pub fn load(dir: &Path, path: &str) -> Result<(Tree, SavedScan), String> {
    let key = key(path);
    let result = (|| -> Result<_, Box<dyn std::error::Error>> {
        let meta: SavedScan =
            serde_json::from_reader(BufReader::new(File::open(dir.join(format!("{key}.json")))?))?;
        if meta.version != FORMAT_VERSION || self::key(&meta.path) != key {
            return Err(OPEN_ERROR.into());
        }
        let mut reader = BufReader::new(File::open(dir.join(format!("{key}.tree")))?);
        header(&mut reader)?;
        let bytes = zstd::stream::decode_all(reader)?;
        let tree = decode(&bytes)?;
        if !valid(&tree) || self::key(&tree.root_path.to_string_lossy()) != key {
            return Err(OPEN_ERROR.into());
        }
        let root = &tree.nodes[0];
        if (root.size, root.files, root.folders, tree.errors)
            != (meta.size, meta.files, meta.folders, meta.errors)
        {
            return Err(OPEN_ERROR.into());
        }
        Ok((tree, meta))
    })();
    result.map_err(|_| {
        remove(dir, &key);
        OPEN_ERROR.into()
    })
}

fn decode(mut bytes: &[u8]) -> Result<Tree, bincode::error::DecodeError> {
    fn read<T: serde::de::DeserializeOwned>(
        bytes: &mut &[u8],
    ) -> Result<T, bincode::error::DecodeError> {
        let (value, used) = bincode::serde::decode_from_slice(bytes, bincode::config::standard())?;
        *bytes = &bytes[used..];
        Ok(value)
    }
    let (count, root_path, errors): (usize, _, _) = read(&mut bytes)?;
    if count > bytes.len() {
        return Err(bincode::error::DecodeError::Other(OPEN_ERROR));
    }
    let mut nodes = Vec::with_capacity(count);
    for _ in 0..count {
        let node: DiskNode = read(&mut bytes)?;
        if node
            .category
            .is_some_and(|index| usize::from(index) >= categories().len())
        {
            return Err(bincode::error::DecodeError::Other(OPEN_ERROR));
        }
        nodes.push(Node {
            name: node.name.into_owned(),
            parent: node.parent,
            children: node.children.into_owned(),
            kind: node.kind,
            size: node.size,
            files: node.files,
            folders: node.folders,
            category: node.category,
        });
    }
    if !bytes.is_empty() {
        return Err(bincode::error::DecodeError::Other(OPEN_ERROR));
    }
    Ok(Tree {
        nodes,
        root_path,
        errors,
    })
}

fn valid(tree: &Tree) -> bool {
    if tree
        .nodes
        .first()
        .is_none_or(|n| n.parent.is_some() || n.kind != Kind::Dir)
    {
        return false;
    }
    let mut seen = vec![false; tree.nodes.len()];
    seen[0] = true;
    for (id, node) in tree.nodes.iter().enumerate() {
        for &child in &node.children {
            if child <= id
                || child >= seen.len()
                || seen[child]
                || tree.nodes[child].parent != Some(id)
            {
                return false;
            }
            seen[child] = true;
        }
    }
    seen.into_iter().all(|s| s)
}

pub fn clear(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod benchmarks {
    use super::*;
    use crate::scanner::{Node, categories};
    use std::{hint::black_box, time::Instant};

    #[test]
    #[ignore = "manual 1.5M-node saved-scan timings; run in release mode"]
    fn saved_scan_timings() {
        let mut tree = Tree {
            nodes: Vec::with_capacity(1_500_000),
            root_path: r"C:\Synthetic".into(),
            errors: 17,
        };
        let mut random = 42_u64;
        for id in 0..1_500_000 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let directory = id < 75_000;
            let parent = if id == 0 {
                None
            } else if directory {
                Some((id - 1) / 4)
            } else {
                Some((id - 75_000) / 19)
            };
            tree.nodes.push(Node {
                name: format!("{:016x}_{}.dat", random, id % 10000),
                parent,
                children: vec![],
                kind: if directory { Kind::Dir } else { Kind::File },
                size: if directory { 0 } else { random % 100_000_000 },
                files: u64::from(!directory),
                folders: 0,
                category: Some((id % categories().len()) as u8),
            });
            if let Some(parent) = parent {
                tree.nodes[parent].children.push(id);
            }
        }
        for id in (1..tree.nodes.len()).rev() {
            let parent = tree.nodes[id].parent.unwrap();
            let (size, files, folders) = (
                tree.nodes[id].size,
                tree.nodes[id].files,
                tree.nodes[id].folders + u64::from(tree.nodes[id].kind == Kind::Dir),
            );
            tree.nodes[parent].size += size;
            tree.nodes[parent].files += files;
            tree.nodes[parent].folders += folders;
        }
        for id in 0..75_000 {
            let mut children = std::mem::take(&mut tree.nodes[id].children);
            children.sort_by_key(|&child| Reverse(tree.nodes[child].size));
            tree.nodes[id].children = children;
        }
        let dir = tempfile::tempdir().unwrap();
        let root = &tree.nodes[0];
        let meta = SavedScan {
            version: FORMAT_VERSION,
            path: tree.root_path.to_string_lossy().into_owned(),
            scanned_at: 123,
            size: root.size,
            files: root.files,
            folders: root.folders,
            errors: tree.errors,
            drive_free: None,
            drive_total: None,
        };
        let start = Instant::now();
        save(dir.path(), &tree, &meta).unwrap();
        eprintln!(
            "TIMING save: {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        let path = dir.path().join(format!("{}.tree", key(&meta.path)));
        eprintln!("TIMING cache: {} bytes", fs::metadata(&path).unwrap().len());
        drop(tree);
        let start = Instant::now();
        let mut reader = BufReader::new(File::open(path).unwrap());
        header(&mut reader).unwrap();
        let bytes = zstd::stream::decode_all(reader).unwrap();
        eprintln!(
            "TIMING read + decompress: {:.3} ms ({} bytes)",
            start.elapsed().as_secs_f64() * 1000.0,
            bytes.len()
        );
        let start = Instant::now();
        let tree = decode(&bytes).unwrap();
        eprintln!(
            "TIMING decode: {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        let start = Instant::now();
        assert!(valid(&tree));
        eprintln!(
            "TIMING valid: {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        for (label, depth, children) in [("root view", 0, 0), ("depth-3 view", 3, 100)] {
            let start = Instant::now();
            black_box(tree.view(0, depth, children).unwrap());
            eprintln!(
                "TIMING {label}: {:.3} ms",
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
        let start = Instant::now();
        let largest = black_box(tree.largest(0, 50));
        eprintln!(
            "TIMING largest: {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        let start = Instant::now();
        let breakdown = black_box(tree.breakdown(0));
        eprintln!(
            "TIMING breakdown: {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        let mut session = crate::Session {
            tree: Some(std::sync::Arc::new(tree)),
            saved: Some(meta.clone()),
            ..Default::default()
        };
        session.panels.largest.set(largest).unwrap();
        session.panels.breakdown.set(breakdown).unwrap();
        let start = Instant::now();
        black_box(
            session
                .open_saved(&meta.path, meta.scanned_at)
                .unwrap()
                .unwrap(),
        );
        eprintln!(
            "TIMING session reuse: {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        let start = Instant::now();
        black_box(session.panels.largest.get().unwrap().clone().unwrap());
        black_box(session.panels.breakdown.get().unwrap().clone().unwrap());
        eprintln!(
            "TIMING cached panels: {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        let start = Instant::now();
        drop(session);
        eprintln!(
            "TIMING drop: {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        let start = Instant::now();
        let loaded = black_box(load(dir.path(), &meta.path).unwrap());
        eprintln!(
            "TIMING load: {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        drop(loaded);
        eprintln!(
            "TIMING load + drop: {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}
