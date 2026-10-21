use crate::scanner::{self, Counters, Kind, Node, Progress, Tree};
use rayon::prelude::*;
use std::{
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

const REFERENCE_MASK: u64 = (1 << 48) - 1;
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BootSector {
    pub bytes_per_sector: u16,
    pub cluster_size: u32,
    pub mft_lcn: u64,
    pub record_size: u32,
}

pub(crate) fn parse_boot_sector(buf: &[u8]) -> Result<BootSector> {
    if buf.len() < 512 || &buf[3..11] != b"NTFS    " || buf[510..512] != [0x55, 0xaa] {
        return Err("Invalid NTFS boot sector");
    }
    let bytes_per_sector = u16_at(buf, 11)?;
    let sectors_per_cluster = match buf[13] {
        1..=128 => u32::from(buf[13]),
        // Large-cluster volumes encode the sector count as a negative power of two.
        244..=255 => 1 << -i32::from(buf[13] as i8),
        _ => return Err("Invalid NTFS cluster size"),
    };
    if !(512..=4096).contains(&bytes_per_sector)
        || !bytes_per_sector.is_power_of_two()
        || !sectors_per_cluster.is_power_of_two()
    {
        return Err("Invalid NTFS sector or cluster size");
    }
    let cluster_size = u32::from(bytes_per_sector) * sectors_per_cluster;
    if cluster_size > 2 * 1024 * 1024 {
        return Err("Invalid NTFS cluster size");
    }
    let clusters_per_record = buf[64] as i8;
    let record_size = if clusters_per_record < 0 {
        1_u32.checked_shl(-i32::from(clusters_per_record) as u32)
    } else {
        cluster_size.checked_mul(clusters_per_record as u32)
    }
    .ok_or("Invalid NTFS record size")?;
    if !(512..=65536).contains(&record_size) || !record_size.is_power_of_two() {
        return Err("Invalid NTFS record size");
    }
    let sectors = u64_at(buf, 40)?;
    let mft_lcn = u64_at(buf, 48)?;
    if mft_lcn == 0 || mft_lcn >= sectors / u64::from(sectors_per_cluster) {
        return Err("Invalid MFT location");
    }
    Ok(BootSector {
        bytes_per_sector,
        cluster_size,
        mft_lcn,
        record_size,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Link {
    pub parent: u64,
    pub name: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Extent {
    pub vcn: u64,
    pub lcn: Option<u64>,
    pub clusters: u64,
}

#[derive(Debug, Default)]
pub(crate) struct Record {
    pub number: u64,
    pub base: Option<u64>,
    pub directory: bool,
    pub excluded: bool,
    pub names: Vec<Link>,
    pub size: Option<u64>,
    pub extents: Vec<Extent>,
}

fn bytes(buf: &[u8], offset: usize, len: usize) -> Result<&[u8]> {
    buf.get(offset..offset.checked_add(len).ok_or("Offset overflow")?)
        .ok_or("Truncated NTFS structure")
}

fn u16_at(buf: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes(buf, offset, 2)?.try_into().unwrap(),
    ))
}

fn u32_at(buf: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes(buf, offset, 4)?.try_into().unwrap(),
    ))
}

fn u64_at(buf: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(
        bytes(buf, offset, 8)?.try_into().unwrap(),
    ))
}

pub(crate) fn parse_record(buf: &mut [u8], number: u64) -> Result<Option<Record>> {
    if bytes(buf, 0, 4)? != b"FILE" || buf.len() < 48 || !buf.len().is_multiple_of(512) {
        return Err("Invalid FILE record");
    }
    let usa = usize::from(u16_at(buf, 4)?);
    let count = usize::from(u16_at(buf, 6)?);
    // NTFS fixups protect 512-byte strides, even on volumes with larger sectors.
    if count != buf.len() / 512 + 1 || usa < 42 || usa % 2 != 0 || usa + count * 2 > 510 {
        return Err("Invalid update sequence array");
    }
    let usn = u16_at(buf, usa)?;
    for sector in 1..count {
        if u16_at(buf, sector * 512 - 2)? != usn {
            return Err("Update sequence mismatch");
        }
    }
    for sector in 1..count {
        let replacement = u16_at(buf, usa + sector * 2)?.to_le_bytes();
        buf[sector * 512 - 2..sector * 512].copy_from_slice(&replacement);
    }
    let flags = u16_at(buf, 22)?;
    if flags & 1 == 0 {
        return Ok(None);
    }
    let used = u32_at(buf, 24)? as usize;
    let allocated = u32_at(buf, 28)? as usize;
    let mut offset = usize::from(u16_at(buf, 20)?);
    if allocated != buf.len() || used > allocated || offset < usa + count * 2 || offset % 8 != 0 {
        return Err("Invalid FILE record bounds");
    }
    let base = u64_at(buf, 32)?;
    let mut record = Record {
        number,
        // The sequence bits distinguish an extension of record 0 from a base record.
        base: (base != 0).then_some(base & REFERENCE_MASK),
        directory: flags & 2 != 0,
        ..Record::default()
    };
    let buf = bytes(buf, 0, used)?;
    loop {
        let kind = u32_at(buf, offset)?;
        if kind == u32::MAX {
            return Ok(Some(record));
        }
        let len = u32_at(buf, offset + 4)? as usize;
        if len < 24 || !len.is_multiple_of(8) {
            return Err("Invalid attribute length");
        }
        let attr = bytes(buf, offset, len)?;
        let nonresident = attr[8];
        let header = match nonresident {
            0 => 24,
            1 => 64,
            _ => return Err("Invalid attribute residency"),
        };
        bytes(attr, 0, header)?;
        let name_len = usize::from(attr[9]);
        let name_offset = usize::from(u16_at(attr, 10)?);
        bytes(attr, name_offset, name_len * 2)?;
        if name_len > 0 && name_offset < header {
            return Err("Invalid attribute name offset");
        }
        let value = if nonresident == 0 {
            let value_offset = usize::from(u16_at(attr, 20)?);
            if value_offset < header || (name_len > 0 && value_offset < name_offset + name_len * 2)
            {
                return Err("Invalid resident value offset");
            }
            Some(bytes(attr, value_offset, u32_at(attr, 16)? as usize)?)
        } else {
            let run_offset = usize::from(u16_at(attr, 32)?);
            if run_offset < header || (name_len > 0 && run_offset < name_offset + name_len * 2) {
                return Err("Invalid data run offset");
            }
            bytes(attr, run_offset, 1)?;
            None
        };
        match kind {
            0x30 => {
                let value = value.ok_or("Non-resident FILE_NAME")?;
                bytes(value, 0, 66)?;
                let len = usize::from(value[64]);
                let namespace = value[65];
                let name = bytes(value, 66, len * 2)?;
                if len == 0 || namespace > 3 {
                    return Err("Invalid FILE_NAME");
                }
                if namespace != 2 {
                    let name: String = char::decode_utf16(
                        name.as_chunks::<2>()
                            .0
                            .iter()
                            .map(|pair| u16::from_le_bytes(*pair)),
                    )
                    .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
                    .collect();
                    let link = Link {
                        parent: u64_at(value, 0)? & REFERENCE_MASK,
                        name,
                    };
                    if !record.names.contains(&link) {
                        record.names.push(link);
                    }
                }
            }
            0x80 if name_len == 0 => {
                if let Some(value) = value {
                    record.size = Some(value.len() as u64);
                } else {
                    let vcn = u64_at(attr, 16)?;
                    if vcn == 0 {
                        record.size = Some(u64_at(attr, 48)?);
                    }
                    if record.base.unwrap_or(number) == 0 {
                        decode_runs(attr, &mut record.extents)?;
                    }
                }
            }
            0xc0 => {
                if let Some(value) = value {
                    record.excluded |= u32_at(value, 0)? & 0x20000000 != 0;
                }
            }
            _ => {}
        }
        offset += len;
    }
}

fn decode_runs(attr: &[u8], extents: &mut Vec<Extent>) -> Result<()> {
    let mut offset = usize::from(u16_at(attr, 32)?);
    let mut vcn = u64_at(attr, 16)?;
    let last_vcn = u64_at(attr, 24)?;
    let mut lcn = 0_i64;
    loop {
        let header = bytes(attr, offset, 1)?[0];
        offset += 1;
        if header == 0 {
            if vcn != last_vcn.checked_add(1).ok_or("VCN overflow")? {
                return Err("Data run VCN mismatch");
            }
            return Ok(());
        }
        let len_size = usize::from(header & 15);
        let offset_size = usize::from(header >> 4);
        if len_size == 0 || len_size > 8 || offset_size > 8 {
            return Err("Invalid data run header");
        }
        let mut raw = [0; 8];
        raw[..len_size].copy_from_slice(bytes(attr, offset, len_size)?);
        let clusters = u64::from_le_bytes(raw);
        offset += len_size;
        if clusters == 0 {
            return Err("Empty data run");
        }
        let physical = if offset_size == 0 {
            None
        } else {
            let delta = bytes(attr, offset, offset_size)?;
            let mut raw = if delta[offset_size - 1] & 0x80 != 0 {
                [255; 8]
            } else {
                [0; 8]
            };
            raw[..offset_size].copy_from_slice(delta);
            lcn = lcn
                .checked_add(i64::from_le_bytes(raw))
                .ok_or("LCN overflow")?;
            let lcn = u64::try_from(lcn).map_err(|_| "Negative LCN")?;
            lcn.checked_add(clusters).ok_or("LCN overflow")?;
            Some(lcn)
        };
        offset += offset_size;
        extents.push(Extent {
            vcn,
            lcn: physical,
            clusters,
        });
        vcn = vcn.checked_add(clusters).ok_or("VCN overflow")?;
        if vcn > last_vcn.checked_add(1).ok_or("VCN overflow")? {
            return Err("Data run exceeds VCN range");
        }
    }
}

pub(crate) fn build_tree(
    records: impl IntoIterator<Item = Record>,
    root_path: PathBuf,
    cancel: &AtomicBool,
) -> Option<Tree> {
    let mut bases: Vec<Option<Record>> = Vec::new();
    let mut extensions = Vec::new();
    for record in records {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let number = usize::try_from(record.number).ok()?;
        if number >= bases.len() {
            bases.resize_with(number.checked_add(1)?, || None);
        }
        if record.base.is_some() {
            extensions.push(record);
        } else {
            bases[number].get_or_insert(record);
        }
    }
    for mut extension in extensions {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let base = usize::try_from(extension.base?).ok();
        if let Some(base) = base
            .and_then(|number| bases.get_mut(number))
            .and_then(Option::as_mut)
        {
            base.names.append(&mut extension.names);
            base.size = base.size.or(extension.size);
            base.excluded |= extension.excluded;
            base.extents.append(&mut extension.extents);
        }
    }
    let root = bases.get(5)?.as_ref()?;
    if !root.directory || root.excluded {
        return None;
    }
    let mut children: Vec<Vec<(usize, String)>> = (0..bases.len()).map(|_| Vec::new()).collect();
    for (number, record) in bases.iter_mut().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let Some(record) = record else {
            continue;
        };
        if number < 24 || record.excluded {
            continue;
        }
        record
            .names
            .sort_unstable_by(|a, b| a.parent.cmp(&b.parent).then_with(|| a.name.cmp(&b.name)));
        record.names.dedup();
        for link in std::mem::take(&mut record.names) {
            if let Ok(parent) = usize::try_from(link.parent)
                && let Some(children) = children.get_mut(parent)
            {
                children.push((number, link.name));
            }
        }
    }
    let mut tree = Tree {
        nodes: vec![Node {
            name: root_path.file_name().map_or_else(
                || root_path.display().to_string(),
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
        root_path,
        errors: 0,
    };
    let mut visited = vec![false; bases.len()];
    visited[5] = true;
    let mut queue = vec![(0, 5)];
    let mut next = 0;
    while next < queue.len() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let (parent, number) = queue[next];
        next += 1;
        for (number, name) in std::mem::take(&mut children[number]) {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let record = bases[number].as_ref()?;
            if record.directory {
                if visited[number] {
                    continue;
                }
                visited[number] = true;
            }
            let kind = if record.directory {
                Kind::Dir
            } else {
                Kind::File
            };
            let id = tree.nodes.len();
            let category = (kind == Kind::File).then(|| scanner::category(&name));
            tree.nodes.push(Node {
                name,
                parent: Some(parent),
                children: vec![],
                kind,
                size: if record.directory {
                    0
                } else {
                    record.size.unwrap_or(0)
                },
                files: u64::from(kind == Kind::File),
                folders: 0,
                category,
            });
            tree.nodes[parent].children.push(id);
            if record.directory {
                queue.push((id, number));
            }
        }
    }
    scanner::finish(&mut tree, cancel).then_some(tree)
}

#[cfg(windows)]
pub(crate) fn scan_volume(
    root_path: &Path,
    cancel: &AtomicBool,
    report: &(impl Fn(Progress) + Sync),
) -> std::result::Result<Option<Tree>, String> {
    use std::{fs::File, os::windows::fs::FileExt};

    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let drive = scanner::drive_letter(root_path).ok_or("Not a local drive root")?;
    let volume = File::open(format!(r"\\.\{}:", char::from(drive)))
        .map_err(|e| format!("Cannot open volume: {e}"))?;
    read_mft(
        |mut offset, mut buf| {
            while !buf.is_empty() {
                match volume.seek_read(buf, offset) {
                    Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                    Ok(read) => {
                        offset += read as u64;
                        buf = &mut buf[read..];
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        },
        root_path,
        cancel,
        report,
    )
}

// Raw volume I/O must use sector-aligned offsets and lengths, even for small records.
fn read_aligned(
    read_at: &(impl Fn(u64, &mut [u8]) -> io::Result<()> + Sync),
    offset: u64,
    buf: &mut [u8],
    sector: u16,
) -> std::result::Result<(), String> {
    let sector = u64::from(sector);
    let start = offset / sector * sector;
    let end = offset
        .checked_add(buf.len() as u64)
        .and_then(|end| end.checked_add(sector - 1))
        .ok_or("Read offset overflow")?
        / sector
        * sector;
    if start == offset && end - start == buf.len() as u64 {
        return read_at(offset, buf).map_err(|e| e.to_string());
    }
    let mut aligned = vec![0; usize::try_from(end - start).map_err(|_| "Read too large")?];
    read_at(start, &mut aligned).map_err(|e| e.to_string())?;
    let skip = (offset - start) as usize;
    buf.copy_from_slice(&aligned[skip..skip + buf.len()]);
    Ok(())
}

struct Chunk {
    offset: u64,
    number: u64,
    len: usize,
}

fn mft_chunks(extents: &[Extent], boot: &BootSector, size: u64) -> Result<Vec<Chunk>> {
    let cluster = u64::from(boot.cluster_size);
    let record = u64::from(boot.record_size);
    let mut end_vcn = 0_u64;
    let mut chunks = Vec::new();
    for extent in extents {
        let lcn = extent.lcn.ok_or("Sparse MFT data run")?;
        if extent.vcn != end_vcn || extent.clusters == 0 {
            return Err("MFT extents have a gap or overlap");
        }
        end_vcn = end_vcn.checked_add(extent.clusters).ok_or("VCN overflow")?;
        let start = extent.vcn.checked_mul(cluster).ok_or("VCN overflow")?;
        let len = extent
            .clusters
            .checked_mul(cluster)
            .ok_or("Run size overflow")?;
        let physical = lcn.checked_mul(cluster).ok_or("LCN overflow")?;
        physical.checked_add(len).ok_or("LCN overflow")?;
        if !start.is_multiple_of(record) || !len.is_multiple_of(record) {
            return Err("MFT extent splits a record");
        }
        let mut consumed = 0;
        let len = len.min(size.saturating_sub(start));
        while consumed < len {
            let bytes = (len - consumed).min(4 * 1024 * 1024);
            chunks.push(Chunk {
                offset: physical + consumed,
                number: (start + consumed) / record,
                len: bytes as usize,
            });
            consumed += bytes;
        }
    }
    if end_vcn.checked_mul(cluster).ok_or("MFT size overflow")? < size {
        return Err("MFT extents do not cover its data");
    }
    Ok(chunks)
}

fn read_mft(
    read_at: impl Fn(u64, &mut [u8]) -> io::Result<()> + Sync,
    root_path: &Path,
    cancel: &AtomicBool,
    report: &(impl Fn(Progress) + Sync),
) -> std::result::Result<Option<Tree>, String> {
    let result = (|| {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let mut buf = vec![0; 4096];
        read_at(0, &mut buf).map_err(|e| e.to_string())?;
        let boot = parse_boot_sector(&buf)?;
        let record_size = u64::from(boot.record_size);
        let offset = boot
            .mft_lcn
            .checked_mul(u64::from(boot.cluster_size))
            .ok_or("MFT offset overflow")?;
        buf.resize(boot.record_size as usize, 0);
        read_aligned(&read_at, offset, &mut buf, boot.bytes_per_sector)?;
        let mut zero = parse_record(&mut buf, 0)?.ok_or("Unused MFT record 0")?;
        if zero.base.is_some() || zero.directory {
            return Err("Invalid MFT record 0".into());
        }
        let size = zero.size.ok_or("Missing MFT data size")?;
        if !size.is_multiple_of(record_size) || size / record_size <= 5 {
            return Err("Invalid MFT data size".into());
        }
        let mut extents = std::mem::take(&mut zero.extents);
        extents.sort_unstable_by_key(|extent| extent.vcn);
        let first = extents.first().ok_or("Missing MFT extents")?;
        if first.vcn != 0 || first.lcn != Some(boot.mft_lcn) {
            return Err("Invalid first MFT extent".into());
        }
        let counters = Counters::new();
        let parse_chunk = |chunk: &Chunk| -> std::result::Result<Vec<Record>, String> {
            if cancel.load(Ordering::Relaxed) {
                return Ok(Vec::new());
            }
            let mut buf = vec![0; chunk.len];
            read_aligned(&read_at, chunk.offset, &mut buf, boot.bytes_per_sector)?;
            let mut records = Vec::new();
            let mut counts = Progress::default();
            for (index, buf) in buf.chunks_exact_mut(boot.record_size as usize).enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(Vec::new());
                }
                let number = chunk.number + index as u64;
                let record = if buf.iter().all(|byte| *byte == 0) {
                    Ok(None)
                } else {
                    parse_record(buf, number)
                };
                match record {
                    Ok(Some(record)) => {
                        if number == 0 && (record.base.is_some() || record.directory)
                            || number == 5
                                && (record.base.is_some() || !record.directory || record.excluded)
                        {
                            return Err("Invalid required MFT record".into());
                        }
                        if record.directory {
                            counts.folders += 1;
                        } else {
                            counts.files += 1;
                            counts.bytes += record.size.unwrap_or(0);
                        }
                        records.push(record);
                    }
                    _ if number == 0 || number == 5 => {
                        return Err("Unreadable MFT record 0 or 5".into());
                    }
                    Ok(None) => {}
                    Err(_) => counts.errors += 1,
                }
                if index % 1024 == 1023 {
                    counters.flush(&mut counts, root_path, report);
                }
            }
            counters.flush(&mut counts, root_path, report);
            Ok(records)
        };
        let mut records = Vec::new();
        let mut first_records = 0;
        if mft_chunks(&extents, &boot, size).is_err() {
            // Attribute-list continuations must be discoverable in the first known run.
            let first_size = first
                .clusters
                .checked_mul(u64::from(boot.cluster_size))
                .ok_or("MFT extent size overflow")?
                .min(size);
            let chunks = mft_chunks(std::slice::from_ref(first), &boot, first_size)?;
            let parsed = chunks
                .par_iter()
                .map(&parse_chunk)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            records.extend(parsed.into_iter().flatten());
            if cancel.load(Ordering::Relaxed) {
                return Ok(None);
            }
            first_records = first_size / record_size;
            for record in &mut records {
                if record.base == Some(0) {
                    extents.append(&mut record.extents);
                }
            }
            extents.sort_unstable_by_key(|extent| extent.vcn);
        }
        let chunks = mft_chunks(&extents, &boot, size)?;
        let parsed = chunks
            .par_iter()
            .filter(|chunk| chunk.number >= first_records)
            .map(parse_chunk)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let mut tree = build_tree(
            records.into_iter().chain(parsed.into_iter().flatten()),
            root_path.to_owned(),
            cancel,
        )
        .ok_or("Cannot build MFT tree")?;
        tree.errors = counters.snapshot(root_path).errors;
        let root = &tree.nodes[0];
        report(Progress {
            files: root.files,
            folders: root.folders,
            bytes: root.size,
            errors: tree.errors,
            current_path: root_path.to_string_lossy().into_owned(),
        });
        Ok(Some(tree))
    })();
    if cancel.load(Ordering::Relaxed) {
        Ok(None)
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put16(buf: &mut [u8], offset: usize, value: u16) {
        buf[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put32(buf: &mut [u8], offset: usize, value: u32) {
        buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put64(buf: &mut [u8], offset: usize, value: u64) {
        buf[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn resident(kind: u32, value: &[u8], stream: &str) -> Vec<u8> {
        let stream: Vec<_> = stream.encode_utf16().collect();
        let value_offset = (24 + stream.len() * 2).next_multiple_of(8);
        let mut attr = vec![0; (value_offset + value.len()).next_multiple_of(8)];
        let len = attr.len() as u32;
        put32(&mut attr, 0, kind);
        put32(&mut attr, 4, len);
        attr[9] = stream.len() as u8;
        put16(&mut attr, 10, 24);
        put32(&mut attr, 16, value.len() as u32);
        put16(&mut attr, 20, value_offset as u16);
        for (i, ch) in stream.into_iter().enumerate() {
            put16(&mut attr, 24 + i * 2, ch);
        }
        attr[value_offset..value_offset + value.len()].copy_from_slice(value);
        attr
    }

    fn filename(parent: u64, name: &str, namespace: u8) -> Vec<u8> {
        let name: Vec<_> = name.encode_utf16().collect();
        let mut value = vec![0; 66 + name.len() * 2];
        put64(&mut value, 0, parent | (7 << 48));
        value[64] = name.len() as u8;
        value[65] = namespace;
        for (i, ch) in name.into_iter().enumerate() {
            put16(&mut value, 66 + i * 2, ch);
        }
        resident(0x30, &value, "")
    }

    fn nonresident(size: u64, vcn: u64, last_vcn: u64, runs: &[u8]) -> Vec<u8> {
        let mut attr = vec![0; (64 + runs.len()).next_multiple_of(8)];
        let len = attr.len() as u32;
        put32(&mut attr, 0, 0x80);
        put32(&mut attr, 4, len);
        attr[8] = 1;
        put64(&mut attr, 16, vcn);
        put64(&mut attr, 24, last_vcn);
        put16(&mut attr, 32, 64);
        put64(&mut attr, 48, size);
        attr[64..64 + runs.len()].copy_from_slice(runs);
        attr
    }

    fn record_buf(flags: u16, base: u64, attrs: &[Vec<u8>]) -> Vec<u8> {
        let mut buf = vec![0; 1024];
        buf[..4].copy_from_slice(b"FILE");
        put16(&mut buf, 4, 48);
        put16(&mut buf, 6, 3);
        put16(&mut buf, 20, 56);
        put16(&mut buf, 22, flags);
        put32(&mut buf, 28, 1024);
        put64(&mut buf, 32, base);
        let mut offset = 56;
        for attr in attrs {
            buf[offset..offset + attr.len()].copy_from_slice(attr);
            offset += attr.len();
        }
        put32(&mut buf, offset, u32::MAX);
        put32(&mut buf, 24, (offset + 8) as u32);
        put16(&mut buf, 48, 0xa123);
        for sector in 1..=2 {
            let end = sector * 512 - 2;
            let saved = u16_at(&buf, end).unwrap();
            put16(&mut buf, 48 + sector * 2, saved);
            put16(&mut buf, end, 0xa123);
        }
        buf
    }

    fn parsed(number: u64, flags: u16, base: u64, attrs: &[Vec<u8>]) -> Record {
        parse_record(&mut record_buf(flags, base, attrs), number)
            .unwrap()
            .unwrap()
    }

    fn directory(number: u64, parent: u64, name: &str) -> Record {
        parsed(number, 3, 0, &[filename(parent, name, 1)])
    }

    fn root() -> Record {
        directory(5, 5, ".")
    }

    fn tree(records: Vec<Record>) -> Tree {
        build_tree(records, PathBuf::from("volume"), &AtomicBool::new(false)).unwrap()
    }

    fn image(fragmented: bool) -> Vec<u8> {
        let mut image = vec![0; 16 * 4096];
        image[3..11].copy_from_slice(b"NTFS    ");
        image[510..512].copy_from_slice(&[0x55, 0xaa]);
        put16(&mut image, 11, 512);
        image[13] = 8;
        image[64] = (-10_i8) as u8;
        put64(&mut image, 40, 128);
        put64(&mut image, 48, 2);
        let (last_vcn, clusters) = if fragmented { (3, 4) } else { (9, 10) };
        let records = [
            (
                0,
                record_buf(
                    1,
                    0,
                    &[nonresident(40 * 1024, 0, last_vcn, &[0x11, clusters, 2, 0])],
                ),
            ),
            (5, record_buf(3, 0, &[filename(5, ".", 1)])),
            (
                6,
                if fragmented {
                    record_buf(1, 1 << 48, &[nonresident(0, 4, 9, &[0x11, 6, 10, 0])])
                } else {
                    record_buf(0, 0, &[])
                },
            ),
            (24, record_buf(3, 0, &[filename(5, "alpha", 1)])),
            (25, record_buf(3, 0, &[filename(24, "nested", 1)])),
            (
                26,
                record_buf(
                    1,
                    0,
                    &[filename(25, "small.txt", 1), resident(0x80, &[0; 7], "")],
                ),
            ),
            (
                27,
                record_buf(
                    1,
                    0,
                    &[filename(5, "root.txt", 1), resident(0x80, &[0; 11], "")],
                ),
            ),
            (
                28,
                record_buf(
                    1,
                    29 | (1 << 48),
                    &[filename(24, "merged.bin", 1), resident(0x80, &[0; 3], "")],
                ),
            ),
            (29, record_buf(1, 0, &[])),
            (7, vec![0xff; 1024]),
        ];
        for (number, record) in records {
            let offset = if fragmented && number >= 16 {
                10 * 4096 + (number - 16) * 1024
            } else {
                2 * 4096 + number * 1024
            };
            image[offset..offset + 1024].copy_from_slice(&record);
        }
        image
    }

    fn image_reader(image: &[u8]) -> impl Fn(u64, &mut [u8]) -> io::Result<()> + Sync + '_ {
        move |offset, buf| {
            let offset = offset as usize;
            let source = image
                .get(offset..offset + buf.len())
                .ok_or(io::ErrorKind::UnexpectedEof)?;
            buf.copy_from_slice(source);
            Ok(())
        }
    }

    #[test]
    fn in_memory_scan_and_attribute_list_fragmentation() {
        for fragmented in [false, true] {
            let reports = std::sync::Mutex::new(Vec::new());
            let tree = read_mft(
                image_reader(&image(fragmented)),
                Path::new(r"C:\"),
                &AtomicBool::new(false),
                &|progress| reports.lock().unwrap().push(progress),
            )
            .unwrap()
            .unwrap();
            assert_eq!(tree.nodes.len(), 6);
            assert_eq!(tree.nodes[0].name, r"C:\");
            assert_eq!(tree.root_path, Path::new(r"C:\"));
            assert_eq!(
                (
                    tree.nodes[0].files,
                    tree.nodes[0].folders,
                    tree.nodes[0].size
                ),
                (3, 2, 21)
            );
            assert_eq!(tree.errors, 1);
            let small = tree
                .nodes
                .iter()
                .position(|node| node.name == "small.txt")
                .unwrap();
            assert_eq!(
                tree.path(small).unwrap(),
                Path::new(r"C:\alpha\nested\small.txt")
            );
            let merged = tree
                .nodes
                .iter()
                .position(|node| node.name == "merged.bin")
                .unwrap();
            assert_eq!(
                tree.path(merged).unwrap(),
                Path::new(r"C:\alpha\merged.bin")
            );
            assert_eq!(tree.nodes[merged].size, 3);
            let reports = reports.lock().unwrap();
            let last = reports.last().unwrap();
            assert_eq!(
                (last.files, last.folders, last.bytes, last.errors),
                (3, 2, 21, 1)
            );
            assert_eq!(last.current_path, r"C:\");
        }
    }

    #[test]
    fn fragmentation_in_record_zero_run_list() {
        let mut image = image(true);
        let zero = record_buf(
            1,
            0,
            &[nonresident(40 * 1024, 0, 9, &[0x11, 4, 2, 0x11, 6, 8, 0])],
        );
        image[8192..9216].copy_from_slice(&zero);
        let tree = read_mft(
            image_reader(&image),
            Path::new(r"C:\"),
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap()
        .unwrap();
        assert_eq!((tree.nodes[0].size, tree.errors), (21, 1));
    }

    #[test]
    fn invalid_mft_layouts_are_errors() {
        for data in [
            nonresident(40 * 1024, 0, 9, &[0x11, 4, 2, 0x01, 6, 0]),
            nonresident(40 * 1024, 0, 3, &[0x11, 4, 2, 0]),
            nonresident(40 * 1024 - 1, 0, 9, &[0x11, 10, 2, 0]),
        ] {
            let mut image = image(false);
            image[8192..9216].copy_from_slice(&record_buf(1, 0, &[data]));
            assert!(
                read_mft(
                    image_reader(&image),
                    Path::new(r"C:\"),
                    &AtomicBool::new(false),
                    &|_| {}
                )
                .is_err()
            );
        }
        for (vcn, last_vcn) in [(5, 10), (3, 8)] {
            let mut image = image(true);
            image[14336..15360].copy_from_slice(&record_buf(
                1,
                1 << 48,
                &[nonresident(0, vcn, last_vcn, &[0x11, 6, 10, 0])],
            ));
            assert!(
                read_mft(
                    image_reader(&image),
                    Path::new(r"C:\"),
                    &AtomicBool::new(false),
                    &|_| {}
                )
                .is_err()
            );
        }
    }

    #[test]
    fn required_records_and_read_failures_are_fatal() {
        for number in [0, 5] {
            for record in [
                vec![0; 1024],
                vec![0xff; 1024],
                record_buf(0, 0, &[]),
                record_buf(1, 1 << 48, &[]),
            ] {
                let mut image = image(false);
                let offset = 8192 + number * 1024;
                image[offset..offset + 1024].copy_from_slice(&record);
                assert!(
                    read_mft(
                        image_reader(&image),
                        Path::new(r"C:\"),
                        &AtomicBool::new(false),
                        &|_| {}
                    )
                    .is_err()
                );
            }
        }
        let mut image = image(false);
        image[3] = 0;
        assert!(
            read_mft(
                image_reader(&image),
                Path::new(r"C:\"),
                &AtomicBool::new(false),
                &|_| {}
            )
            .is_err()
        );
        assert!(
            read_mft(
                |_, _| Err(io::ErrorKind::PermissionDenied.into()),
                Path::new(r"C:\"),
                &AtomicBool::new(false),
                &|_| {}
            )
            .is_err()
        );
        assert!(
            read_mft(
                image_reader(&image[..9000]),
                Path::new(r"C:\"),
                &AtomicBool::new(false),
                &|_| {}
            )
            .is_err()
        );
    }

    #[test]
    fn reads_are_aligned_on_4kn_volumes() {
        let mut image = image(false);
        put16(&mut image, 11, 4096);
        image[13] = 1;
        let reader = image_reader(&image);
        let tree = read_mft(
            |offset, buf| {
                assert!(offset.is_multiple_of(4096));
                assert!(buf.len().is_multiple_of(4096));
                reader(offset, buf)
            },
            Path::new(r"C:\"),
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap()
        .unwrap();
        assert_eq!(tree.nodes[0].size, 21);
    }

    #[test]
    fn chunks_are_bounded_and_do_not_cross_extents() {
        let boot = parse_boot_sector(&image(false)).unwrap();
        let extents = [
            Extent {
                vcn: 0,
                lcn: Some(2),
                clusters: 1025,
            },
            Extent {
                vcn: 1025,
                lcn: Some(9000),
                clusters: 3,
            },
        ];
        let chunks = mft_chunks(&extents, &boot, 1028 * 4096).unwrap();
        assert_eq!(chunks.len(), 3);
        assert_eq!(
            (chunks[0].offset, chunks[0].number, chunks[0].len),
            (8192, 0, 4 * 1024 * 1024)
        );
        assert_eq!(
            (chunks[1].offset, chunks[1].number, chunks[1].len),
            (8192 + 4 * 1024 * 1024, 4096, 4096)
        );
        assert_eq!(
            (chunks[2].offset, chunks[2].number, chunks[2].len),
            (9000 * 4096, 4100, 3 * 4096)
        );
    }

    #[test]
    fn cancellation_wins_over_read_errors_and_during_parsing() {
        let cancel = AtomicBool::new(true);
        assert!(
            read_mft(
                |_, _| panic!("Cancelled before reading"),
                Path::new(r"C:\"),
                &cancel,
                &|_| {}
            )
            .unwrap()
            .is_none()
        );
        cancel.store(false, Ordering::Relaxed);
        assert!(
            read_mft(
                |_, _| {
                    cancel.store(true, Ordering::Relaxed);
                    Err(io::ErrorKind::PermissionDenied.into())
                },
                Path::new(r"C:\"),
                &cancel,
                &|_| {}
            )
            .unwrap()
            .is_none()
        );
        cancel.store(false, Ordering::Relaxed);
        let image = image(false);
        let reader = image_reader(&image);
        assert!(
            read_mft(
                |offset, buf| {
                    reader(offset, buf)?;
                    if buf.len() > 4096 {
                        cancel.store(true, Ordering::Relaxed);
                    }
                    Ok(())
                },
                Path::new(r"C:\"),
                &cancel,
                &|_| {}
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn reading_progress_can_cancel_before_tree_assembly() {
        let size = 2048 * 1024;
        let mut image = image(false);
        image.resize(8192 + size, 0);
        let sectors = image.len() / 512;
        put64(&mut image, 40, sectors as u64);
        image[8192..9216].copy_from_slice(&record_buf(
            1,
            0,
            &[nonresident(size as u64, 0, 511, &[0x12, 0, 2, 2, 0])],
        ));
        let cancel = AtomicBool::new(false);
        let reader = image_reader(&image);
        let tree = read_mft(
            |offset, buf| {
                reader(offset, buf)?;
                if buf.len() > 4096 {
                    std::thread::sleep(std::time::Duration::from_millis(110));
                }
                Ok(())
            },
            Path::new(r"C:\"),
            &cancel,
            &|progress| {
                assert_eq!(
                    (
                        progress.files,
                        progress.folders,
                        progress.bytes,
                        progress.errors
                    ),
                    (5, 3, size as u64 + 21, 1)
                );
                assert_eq!(progress.current_path, r"C:\");
                cancel.store(true, Ordering::Relaxed);
            },
        )
        .unwrap();
        assert!(cancel.load(Ordering::Relaxed));
        assert!(tree.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn inaccessible_volume_returns_an_error() {
        let current = std::env::current_dir().unwrap();
        let root = current.ancestors().last().unwrap();
        match scan_volume(root, &AtomicBool::new(false), &|_| {}) {
            Err(error) => {
                assert!(!error.is_empty());
                eprintln!("Raw volume unavailable: {error}");
            }
            Ok(_) => eprintln!("Raw volume is accessible; skipping the access-denied assertion"),
        }
        assert!(scan_volume(Path::new(r"?:\"), &AtomicBool::new(false), &|_| {}).is_err());
    }

    #[test]
    fn boot_sector_geometry_and_invalid_values() {
        let mut buf = vec![0; 512];
        buf[3..11].copy_from_slice(b"NTFS    ");
        buf[510..512].copy_from_slice(&[0x55, 0xaa]);
        put16(&mut buf, 11, 512);
        buf[13] = 8;
        put64(&mut buf, 40, 80000);
        put64(&mut buf, 48, 100);
        buf[64] = (-10_i8) as u8;
        assert_eq!(
            parse_boot_sector(&buf).unwrap(),
            BootSector {
                bytes_per_sector: 512,
                cluster_size: 4096,
                mft_lcn: 100,
                record_size: 1024,
            }
        );
        buf[64] = 2;
        assert_eq!(parse_boot_sector(&buf).unwrap().record_size, 8192);
        put16(&mut buf, 11, 4096);
        buf[64] = (-12_i8) as u8;
        assert_eq!(parse_boot_sector(&buf).unwrap().cluster_size, 32768);
        for (offset, value) in [
            (3, 0),
            (11, 1),
            (13, 0),
            (13, 3),
            (64, 0),
            (64, 127),
            (64, 128),
            (64, 255),
            (510, 0),
        ] {
            let mut invalid = buf.clone();
            invalid[offset] = value;
            assert!(
                parse_boot_sector(&invalid).is_err(),
                "offset {offset}, value {value}"
            );
        }
        put64(&mut buf, 48, 10000);
        assert!(parse_boot_sector(&buf).is_err());
        put64(&mut buf, 48, 1);
        put16(&mut buf, 11, 512);
        buf[13] = (-12_i8) as u8;
        assert_eq!(
            parse_boot_sector(&buf).unwrap().cluster_size,
            2 * 1024 * 1024
        );
    }

    #[test]
    fn fixups_restore_sector_ends_and_reject_mismatch() {
        let data: Vec<_> = (0..600).map(|i| i as u8).collect();
        let mut buf = record_buf(1, 0, &[resident(0x80, &data, "")]);
        let saved = u16_at(&buf, 50).unwrap();
        let record = parse_record(&mut buf, 24).unwrap().unwrap();
        assert_eq!(record.size, Some(600));
        assert_eq!(u16_at(&buf, 510).unwrap(), saved);
        assert_eq!(&buf[80..680], data);
        let mut corrupt = record_buf(1, 0, &[]);
        corrupt[1022] ^= 1;
        assert_eq!(
            parse_record(&mut corrupt, 24).unwrap_err(),
            "Update sequence mismatch"
        );
        assert_eq!(u16_at(&corrupt, 510).unwrap(), 0xa123);
    }

    #[test]
    fn unused_records_are_skipped() {
        assert!(
            parse_record(&mut record_buf(0, 0, &[]), 24)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn larger_records_fix_every_512_byte_stride() {
        let mut buf = vec![0; 4096];
        buf[..4].copy_from_slice(b"FILE");
        put16(&mut buf, 4, 48);
        put16(&mut buf, 6, 9);
        put16(&mut buf, 20, 72);
        put16(&mut buf, 22, 1);
        put32(&mut buf, 24, 80);
        put32(&mut buf, 28, 4096);
        put32(&mut buf, 72, u32::MAX);
        put16(&mut buf, 48, 0x1234);
        for sector in 1..=8 {
            put16(&mut buf, 48 + sector * 2, sector as u16);
            put16(&mut buf, sector * 512 - 2, 0x1234);
        }
        assert!(parse_record(&mut buf, 24).unwrap().is_some());
        for sector in 1..=8 {
            assert_eq!(u16_at(&buf, sector * 512 - 2).unwrap(), sector as u16);
        }
    }

    #[test]
    fn namespaces_and_distinct_names_under_one_parent() {
        let record = parsed(
            24,
            1,
            0,
            &[
                filename(5, "SHORT~1", 2),
                filename(5, "long.txt", 1),
                filename(5, "both.txt", 3),
                filename(5, "posix.txt", 0),
                filename(5, "long.txt", 0),
                filename(5, "😀.txt", 1),
            ],
        );
        assert_eq!(record.names.len(), 4);
        assert!(record.names.iter().all(|n| n.parent == 5));
        let tree = tree(vec![root(), record]);
        assert_eq!(tree.nodes[0].files, 4);
        assert!(tree.nodes.iter().any(|n| n.name == "😀.txt"));
        assert!(!tree.nodes.iter().any(|n| n.name == "SHORT~1"));
    }

    #[test]
    fn data_sizes_ignore_ads_and_continuation_segments() {
        let record = parsed(
            24,
            1,
            0,
            &[
                resident(0x80, &[1, 2, 3], ""),
                resident(0x80, &[0; 30], "ads"),
            ],
        );
        assert_eq!(record.size, Some(3));
        let record = parsed(
            25,
            1,
            0,
            &[
                nonresident(123456, 0, 2, &[0x11, 3, 20, 0]),
                nonresident(999, 3, 4, &[0x11, 2, 30, 0]),
            ],
        );
        assert_eq!(record.size, Some(123456));
        assert!(record.extents.is_empty());
        let mut ads = nonresident(999, 0, 1, &[0]);
        ads[9] = 1;
        put16(&mut ads, 10, 64);
        put16(&mut ads, 32, 66);
        assert_eq!(parsed(26, 1, 0, &[ads]).size, None);
    }

    #[test]
    fn hard_links_roll_up_and_sort_like_directory_scan() {
        let file = parsed(
            26,
            1,
            0,
            &[
                filename(24, "b.txt", 1),
                filename(25, "a.txt", 1),
                resident(0x80, &[0; 10], ""),
            ],
        );
        let tree = tree(vec![
            file,
            directory(25, 5, "b"),
            root(),
            directory(24, 5, "a"),
        ]);
        assert_eq!(
            (
                tree.nodes[0].size,
                tree.nodes[0].files,
                tree.nodes[0].folders
            ),
            (20, 2, 2)
        );
        assert_eq!(tree.nodes[tree.nodes[0].children[0]].name, "a");
        for (id, node) in tree.nodes.iter().enumerate() {
            assert!(node.parent.is_none_or(|parent| parent < id));
            assert!(node.children.iter().all(|child| *child > id));
            if node.kind == Kind::File {
                assert_eq!(node.category, Some(scanner::category(&node.name)));
            }
        }
        assert_eq!(tree.nodes[0].category, Some(scanner::category("a.txt")));
    }

    #[test]
    fn extensions_merge_names_data_and_reparse_in_any_order() {
        let extension = parsed(
            40,
            1,
            24 | (2 << 48),
            &[filename(5, "file.txt", 1), resident(0x80, &[0; 7], "")],
        );
        assert_eq!(extension.base, Some(24));
        let tree = tree(vec![
            extension,
            root(),
            parsed(24, 1, 0, &[filename(5, "file.txt", 1)]),
        ]);
        assert_eq!((tree.nodes.len(), tree.nodes[0].size), (2, 7));
        let extension = parsed(40, 1, 24, &[nonresident(123456, 0, 2, &[0x11, 3, 20, 0])]);
        let tree = self::tree(vec![
            root(),
            parsed(24, 1, 0, &[filename(5, "large.txt", 1)]),
            extension,
        ]);
        assert_eq!(tree.nodes[0].size, 123456);
        let extension = parsed(
            41,
            1,
            25,
            &[resident(0xc0, &0xa000000c_u32.to_le_bytes(), "")],
        );
        let tree = self::tree(vec![
            root(),
            directory(25, 5, "junction"),
            extension,
            directory(26, 25, "hidden"),
        ]);
        assert_eq!(tree.nodes.len(), 1);
        let orphan_extension = parsed(42, 1, 99, &[filename(5, "missing-base", 1)]);
        assert_eq!(self::tree(vec![root(), orphan_extension]).nodes.len(), 1);
    }

    #[test]
    fn absurd_extension_bases_and_parents_are_ignored() {
        let extension = parsed(40, 1, REFERENCE_MASK, &[filename(5, "invalid-base", 1)]);
        let orphan = directory(24, REFERENCE_MASK, "invalid-parent");
        let tree = tree(vec![extension, orphan, root(), directory(25, 5, "kept")]);
        assert_eq!(tree.nodes.len(), 2);
        assert_eq!(tree.nodes[1].name, "kept");
    }

    #[test]
    fn nonresident_reparse_keeps_record_and_continues_parsing() {
        let mut reparse = nonresident(100, 0, 0, &[0x11, 1, 20, 0]);
        put32(&mut reparse, 0, 0xc0);
        let record = parsed(
            24,
            3,
            0,
            &[reparse, filename(5, "kept", 1), resident(0x80, &[0; 7], "")],
        );
        assert!(!record.excluded);
        assert_eq!(record.size, Some(7));
        let tree = tree(vec![root(), record, directory(25, 24, "child")]);
        assert_eq!(tree.nodes.len(), 3);
        assert_eq!(tree.nodes[1].name, "kept");
        assert_eq!(tree.nodes[2].name, "child");
    }

    #[test]
    fn reparse_surrogates_exclude_subtrees_but_cloud_tags_remain() {
        let junction = parsed(
            24,
            3,
            0,
            &[
                filename(5, "junction", 1),
                resident(0xc0, &0xa0000003_u32.to_le_bytes(), ""),
            ],
        );
        let cloud = parsed(
            26,
            3,
            0,
            &[
                filename(5, "cloud", 1),
                resident(0xc0, &0x9000001a_u32.to_le_bytes(), ""),
            ],
        );
        let tree = tree(vec![
            root(),
            junction,
            directory(25, 24, "hidden"),
            cloud,
            directory(27, 26, "kept"),
        ]);
        assert_eq!(tree.nodes.len(), 3);
        assert!(tree.nodes.iter().any(|n| n.name == "kept"));
        assert!(!tree.nodes.iter().any(|n| n.name == "hidden"));
    }

    #[test]
    fn metafiles_extend_orphans_and_file_parents_are_dropped() {
        let tree = tree(vec![
            root(),
            directory(11, 5, "$Extend"),
            directory(12, 5, "$Meta"),
            directory(24, 11, "under-extend"),
            directory(25, 24, "descendant"),
            directory(26, 99, "orphan"),
            directory(27, 26, "orphan-child"),
            parsed(28, 1, 0, &[filename(5, "file", 1)]),
            directory(29, 28, "bad-parent"),
        ]);
        assert_eq!(tree.nodes.len(), 2);
        assert_eq!(tree.nodes[1].name, "file");
    }

    #[test]
    fn cycles_and_repeated_directory_links_are_skipped() {
        let a = parsed(
            24,
            3,
            0,
            &[
                filename(5, "a", 1),
                filename(25, "back", 1),
                filename(5, "alias", 0),
            ],
        );
        let tree = tree(vec![
            root(),
            a,
            directory(25, 24, "b"),
            directory(26, 26, "self"),
            directory(27, 28, "cycle1"),
            directory(28, 27, "cycle2"),
        ]);
        assert_eq!(tree.nodes.len(), 3);
        assert_eq!(tree.nodes[0].folders, 2);
        assert_eq!(tree.nodes.iter().filter(|n| n.name == "b").count(), 1);
        assert!(tree.nodes.iter().all(|n| n.name != "back"));
        assert!(tree.nodes.iter().all(|n| n.name != "alias"));
    }

    #[test]
    fn directory_under_two_parents_emits_its_subtree_once() {
        let shared = parsed(
            26,
            3,
            0,
            &[filename(24, "first", 1), filename(25, "second", 1)],
        );
        let file = parsed(
            27,
            1,
            0,
            &[filename(26, "file.txt", 1), resident(0x80, &[0; 7], "")],
        );
        let tree = tree(vec![
            shared,
            file,
            directory(25, 5, "b"),
            root(),
            directory(24, 5, "a"),
        ]);
        assert_eq!(tree.nodes.len(), 5);
        assert_eq!(
            (
                tree.nodes[0].size,
                tree.nodes[0].files,
                tree.nodes[0].folders
            ),
            (7, 1, 3)
        );
        assert_eq!(tree.nodes.iter().filter(|n| n.name == "first").count(), 1);
        assert!(tree.nodes.iter().all(|n| n.name != "second"));
        assert_eq!(
            tree.nodes.iter().filter(|n| n.name == "file.txt").count(),
            1
        );
        let shared = tree.nodes.iter().find(|n| n.name == "first").unwrap();
        assert_eq!(tree.nodes[shared.parent.unwrap()].name, "a");
    }

    #[test]
    fn run_lists_decode_signed_offsets_sparse_runs_and_extension_vcns() {
        let record = parsed(
            0,
            1,
            0,
            &[nonresident(
                12345,
                0,
                8,
                &[0x11, 3, 100, 0x11, 2, 0xf6, 0x01, 1, 0x21, 3, 0x2c, 1, 0],
            )],
        );
        assert_eq!(
            record.extents,
            vec![
                Extent {
                    vcn: 0,
                    lcn: Some(100),
                    clusters: 3
                },
                Extent {
                    vcn: 3,
                    lcn: Some(90),
                    clusters: 2
                },
                Extent {
                    vcn: 5,
                    lcn: None,
                    clusters: 1
                },
                Extent {
                    vcn: 6,
                    lcn: Some(390),
                    clusters: 3
                },
            ]
        );
        let extension = parsed(40, 1, 3 << 48, &[nonresident(0, 9, 10, &[0x11, 2, 60, 0])]);
        assert_eq!(extension.base, Some(0));
        assert_eq!(extension.size, None);
        assert_eq!(
            extension.extents,
            vec![Extent {
                vcn: 9,
                lcn: Some(60),
                clusters: 2
            }]
        );
        let record = parsed(
            0,
            1,
            0,
            &[nonresident(
                1,
                0,
                0,
                &[0x81, 1, 0xff, 0xff, 0xff, 0x7f, 0, 0, 0, 0, 0],
            )],
        );
        assert_eq!(record.extents[0].lcn, Some(0x7fffffff));
    }

    #[test]
    fn corrupt_runs_are_errors() {
        for runs in [
            &[0x10, 1, 0][..],
            &[0x19, 1, 0],
            &[0x91, 1, 0],
            &[0x11, 0, 1, 0],
            &[0x11, 1, 255, 0],
            &[0],
            &[0x11, 2, 1, 0],
        ] {
            assert!(
                parse_record(&mut record_buf(1, 0, &[nonresident(10, 0, 0, runs)]), 0).is_err()
            );
        }
        let mut extents = vec![];
        assert!(decode_runs(&nonresident(1, 0, 0, &[0x88, 1]), &mut extents).is_err());
    }

    #[test]
    fn truncated_garbage_and_malformed_offsets_never_panic() {
        let valid = record_buf(1, 0, &[filename(5, "file.txt", 1)]);
        for len in 0..valid.len() {
            assert!(
                parse_record(&mut valid[..len].to_vec(), 24).is_err(),
                "length {len}"
            );
        }
        for len in 0..512 {
            assert!(parse_boot_sector(&vec![0; len]).is_err());
        }
        for seed in 0..256_u32 {
            let mut random = seed + 1;
            let mut buf = vec![0; 1024];
            for byte in &mut buf {
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                *byte = (random >> 24) as u8;
            }
            assert!(parse_record(&mut buf, 24).is_err());
            assert!(parse_boot_sector(&buf).is_err());
        }
        for (offset, width, value) in [
            (4, 2, 1024),
            (6, 2, 0),
            (20, 2, 16),
            (20, 2, 1024),
            (24, 4, 1025),
            (28, 4, 0),
            (60, 4, 0),
            (60, 4, u32::MAX),
            (66, 2, 65535),
            (72, 4, u32::MAX),
            (76, 2, 0),
        ] {
            let mut buf = valid.clone();
            if width == 2 {
                put16(&mut buf, offset, value as u16);
            } else {
                put32(&mut buf, offset, value);
            }
            assert!(parse_record(&mut buf, 24).is_err(), "offset {offset}");
        }
        for value in [2, 255] {
            let mut buf = valid.clone();
            buf[64] = value;
            assert!(parse_record(&mut buf, 24).is_err());
        }
        for offset in 0..valid.len() {
            for value in [0, 255] {
                let mut buf = valid.clone();
                buf[offset] = value;
                let _ = parse_record(&mut buf, 24);
            }
        }
    }

    #[test]
    fn cancellation_and_missing_root() {
        assert!(build_tree(vec![root()], PathBuf::new(), &AtomicBool::new(true)).is_none());
        assert!(build_tree(vec![], PathBuf::new(), &AtomicBool::new(false)).is_none());
        let cancel = AtomicBool::new(false);
        let records = [root(), directory(24, 5, "a")]
            .into_iter()
            .inspect(|record| {
                if record.number == 24 {
                    cancel.store(true, Ordering::Relaxed);
                }
            });
        assert!(build_tree(records, PathBuf::new(), &cancel).is_none());
    }
}
