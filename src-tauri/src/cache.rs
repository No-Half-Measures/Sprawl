use crate::scanner::{Kind, Tree};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Write},
    path::Path,
};

const MAGIC: &[u8; 8] = b"DISKMAP\0";
pub const FORMAT_VERSION: u32 = 1;
const OPEN_ERROR: &str = "This saved scan can't be opened. Scan again to refresh it.";

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
    bincode::serde::encode_into_std_write(tree, &mut writer, bincode::config::standard())
        .map_err(io::Error::other)?;
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
        let mut decoder = BufReader::new(zstd::stream::read::Decoder::new(reader)?);
        let tree: Tree =
            bincode::serde::decode_from_std_read(&mut decoder, bincode::config::standard())?;
        let mut extra = [0];
        if decoder.read(&mut extra)? != 0
            || !valid(&tree)
            || self::key(&tree.root_path.to_string_lossy()) != key
        {
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
