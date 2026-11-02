use crate::scanner::{Kind, Tree, categories};
use serde::Serialize;
use std::{
    fs,
    io::{self, BufWriter, Write},
    path::{MAIN_SEPARATOR, Path},
};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Csv,
    Json,
}

impl Format {
    pub fn parse(text: &str) -> Result<Self, String> {
        match text.to_ascii_lowercase().as_str() {
            "csv" => Ok(Self::Csv),
            "json" => Ok(Self::Json),
            _ => Err(format!(
                "Unknown export format \"{text}\". Use csv or json."
            )),
        }
    }

    /// JSON for a `.json` file, otherwise CSV.
    pub fn for_path(path: &Path) -> Self {
        match path.extension() {
            Some(extension) if extension.eq_ignore_ascii_case("json") => Self::Json,
            _ => Self::Csv,
        }
    }
}

#[derive(Serialize)]
struct Row<'a> {
    path: &'a str,
    kind: Kind,
    size: u64,
    allocated: Option<u64>,
    files: u64,
    folders: u64,
    category: Option<&'a str>,
}

/// Writes every item under and including `id` to `path`, depth first in stored
/// order, and returns the number of items. A failed export leaves no file behind.
pub fn export(tree: &Tree, id: usize, path: &Path, format: Format) -> Result<u64, String> {
    let root = tree.path(id)?.to_string_lossy().into_owned();
    let name = path
        .file_name()
        .ok_or_else(|| format!("Choose a file to export to, not {}.", path.display()))?;
    let mut temp_name = name.to_owned();
    temp_name.push(format!(".{}.tmp", std::process::id()));
    let temp = path.with_file_name(temp_name);
    let result = fs::File::create(&temp)
        .and_then(|file| {
            let mut writer = BufWriter::new(file);
            let count = write(tree, id, root, format, &mut writer)?;
            writer.into_inner()?.sync_all()?;
            Ok(count)
        })
        .and_then(|count| fs::rename(&temp, path).map(|()| count));
    result.map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("Could not export to {}: {e}", path.display())
    })
}

fn write(
    tree: &Tree,
    id: usize,
    root: String,
    format: Format,
    out: &mut impl Write,
) -> io::Result<u64> {
    match format {
        Format::Csv => {
            out.write_all("\u{feff}path,kind,size,".as_bytes())?;
            if tree.has_allocated {
                out.write_all(b"allocated,")?;
            }
            out.write_all(b"files,folders,category\r\n")?;
        }
        Format::Json => out.write_all(b"[")?,
    }
    let mut path = root;
    let mut ends: Vec<usize> = vec![];
    let mut stack = vec![(id, 0)];
    let mut count = 0;
    while let Some((i, depth)) = stack.pop() {
        let node = &tree.nodes[i];
        if depth > 0 {
            path.truncate(ends[depth - 1]);
            if !path.ends_with(MAIN_SEPARATOR) {
                path.push(MAIN_SEPARATOR);
            }
            path.push_str(&node.name);
        }
        ends.truncate(depth);
        ends.push(path.len());
        stack.extend(node.children.iter().rev().map(|&child| (child, depth + 1)));
        let row = Row {
            path: &path,
            kind: node.kind,
            size: node.size,
            allocated: tree.has_allocated.then_some(node.allocated),
            files: node.files,
            folders: node.folders,
            category: (node.kind == Kind::File)
                .then_some(node.category)
                .flatten()
                .map(|index| categories()[usize::from(index)].id.as_str()),
        };
        match format {
            Format::Csv => {
                csv_field(out, row.path)?;
                write!(out, ",{},{},", kind(row.kind), row.size)?;
                if let Some(allocated) = row.allocated {
                    write!(out, "{allocated},")?;
                }
                write!(out, "{},{},", row.files, row.folders)?;
                csv_field(out, row.category.unwrap_or_default())?;
                out.write_all(b"\r\n")?;
            }
            Format::Json => {
                if count > 0 {
                    out.write_all(b",")?;
                }
                out.write_all(b"\n")?;
                serde_json::to_writer(&mut *out, &row)?;
            }
        }
        count += 1;
    }
    if format == Format::Json {
        out.write_all(b"\n]\n")?;
    }
    Ok(count)
}

fn kind(kind: Kind) -> &'static str {
    match kind {
        Kind::Dir => "dir",
        Kind::File => "file",
        Kind::Other => "other",
    }
}

/// Writes an RFC 4180 field, quoting it when it holds a comma, quote, CR or LF.
fn csv_field(out: &mut impl Write, field: &str) -> io::Result<()> {
    if field.contains([',', '"', '\r', '\n']) {
        write!(out, "\"{}\"", field.replace('"', "\"\""))
    } else {
        out.write_all(field.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn csv_fields_follow_rfc_4180() {
        for (field, expected) in [
            ("plain", "plain"),
            ("a,b", "\"a,b\""),
            ("say \"hi\"", "\"say \"\"hi\"\"\""),
            ("line\r\nbreak", "\"line\r\nbreak\""),
            ("lf\n", "\"lf\n\""),
            ("", ""),
        ] {
            let mut out = vec![];
            super::csv_field(&mut out, field).unwrap();
            assert_eq!(String::from_utf8(out).unwrap(), expected);
        }
    }
}
