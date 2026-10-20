use crate::scanner::{self, Kind, Tree};
use std::{fs, path::Path, sync::atomic::AtomicBool, time::Instant};

fn fixture() -> (tempfile::TempDir, Tree) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("nested")).unwrap();
    fs::create_dir(dir.path().join("empty")).unwrap();
    fs::write(dir.path().join("large.MP4"), [0; 100]).unwrap();
    fs::write(dir.path().join("small.txt"), [0; 10]).unwrap();
    fs::write(dir.path().join("nested/code.rs"), [0; 40]).unwrap();
    let tree = scanner::scan(dir.path(), &AtomicBool::new(false), |_| {})
        .unwrap()
        .unwrap();
    (dir, tree)
}

#[test]
fn rollup_counts_sort_and_paths() {
    let (dir, tree) = fixture();
    let root = &tree.nodes[0];
    assert_eq!((root.size, root.files, root.folders), (150, 3, 2));
    assert_eq!(
        root.children
            .iter()
            .map(|i| tree.nodes[*i].size)
            .collect::<Vec<_>>(),
        vec![100, 40, 10, 0]
    );
    assert!(tree.path(root.children[0]).unwrap().ends_with("large.MP4"));
    assert_eq!(
        tree.root_path,
        scanner::strip_verbatim_prefix(&fs::canonicalize(dir.path()).unwrap())
    );
    assert_eq!(tree.path(0).unwrap(), tree.root_path);
    assert!(
        !tree
            .path(root.children[0])
            .unwrap()
            .to_string_lossy()
            .starts_with(r"\\?\")
    );
    assert!(tree.view(usize::MAX, 2, 10).is_err());
    assert!(tree.path(usize::MAX).is_err());
    assert!(tree.largest(usize::MAX, 50).is_err());
    assert!(tree.breakdown(usize::MAX).is_err());
}

#[test]
fn normal_paths_are_unchanged() {
    for path in [
        r"C:\Users\Example",
        r"\\server\share\folder",
        "relative/folder",
    ] {
        assert_eq!(
            scanner::strip_verbatim_prefix(Path::new(path)),
            Path::new(path)
        );
    }
}

#[cfg(windows)]
#[test]
fn verbatim_drive_and_unc_prefixes_are_stripped() {
    for (verbatim, normal) in [
        (r"\\?\C:\", r"C:\"),
        (r"\\?\C:\Users\Example", r"C:\Users\Example"),
        (r"\\?\C:\Users\日本語", r"C:\Users\日本語"),
        (r"\\?\UNC\server\share\", r"\\server\share\"),
        (r"\\?\UNC\server\share\folder", r"\\server\share\folder"),
    ] {
        assert_eq!(
            scanner::strip_verbatim_prefix(Path::new(verbatim)),
            Path::new(normal)
        );
    }
}

#[test]
fn depth_and_aggregate_preserve_totals() {
    let (_, tree) = fixture();
    assert!(tree.view(0, 0, 10).unwrap().children.is_none());
    let view = tree.view(0, 1, 2).unwrap();
    let children = view.children.unwrap();
    assert_eq!(children.len(), 3);
    assert!(children.iter().all(|n| n.children.is_none()));
    let aggregate = &children[2];
    assert_eq!(aggregate.name, "2 smaller items");
    assert_eq!(
        (
            aggregate.id,
            aggregate.kind,
            aggregate.size,
            aggregate.files,
            aggregate.folders
        ),
        (None, Kind::Other, 10, 1, 1)
    );
    assert_eq!(children.iter().map(|n| n.size).sum::<u64>(), 150);
    let all = tree.view(0, 4, 0).unwrap().children.unwrap();
    assert_eq!(
        (all.len(), all[0].size, all[0].files, all[0].folders),
        (1, 150, 3, 2)
    );
    let nested = tree.view(0, 2, 100).unwrap().children.unwrap().remove(1);
    assert_eq!(nested.children.unwrap()[0].size, 40);
}

#[test]
fn largest_and_breakdown_are_scoped() {
    let (_, tree) = fixture();
    let largest = tree.largest(0, 2).unwrap();
    assert_eq!(
        largest.iter().map(|n| n.size).collect::<Vec<_>>(),
        vec![100, 40]
    );
    assert!(largest[1].relative_path.ends_with("code.rs"));
    let nested = tree.nodes[0].children[1];
    assert_eq!(tree.largest(nested, 50).unwrap().len(), 1);
    assert_eq!(
        tree.largest(nested, 50).unwrap()[0].relative_path,
        "code.rs"
    );
    assert!(tree.largest(0, 0).unwrap().is_empty());
    let types = tree.breakdown(0).unwrap();
    assert_eq!(types.len(), 8);
    assert_eq!(types.iter().map(|r| r.bytes).sum::<u64>(), 150);
    assert_eq!(types.iter().map(|r| r.files).sum::<u64>(), 3);
    assert_eq!(
        types.iter().find(|r| r.category == "video").unwrap().bytes,
        100
    );
    assert_eq!(
        tree.breakdown(nested)
            .unwrap()
            .iter()
            .find(|r| r.category == "code")
            .unwrap()
            .bytes,
        40
    );
    assert_eq!(scanner::category("unknown.ext"), "other");
}

#[test]
fn dominant_folder_category_uses_all_descendant_bytes() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("mixed")).unwrap();
    fs::create_dir(dir.path().join("empty")).unwrap();
    fs::write(dir.path().join("mixed/build.rlib"), [0; 60]).unwrap();
    fs::write(dir.path().join("mixed/game.pbo"), [0; 40]).unwrap();
    fs::write(dir.path().join("more-game.uasset"), [0; 50]).unwrap();
    let tree = scanner::scan(dir.path(), &AtomicBool::new(false), |_| {})
        .unwrap()
        .unwrap();
    let mixed = tree.nodes[0]
        .children
        .iter()
        .copied()
        .find(|id| tree.nodes[*id].name == "mixed")
        .unwrap();
    let empty = tree.nodes[0]
        .children
        .iter()
        .copied()
        .find(|id| tree.nodes[*id].name == "empty")
        .unwrap();
    assert_eq!(tree.nodes[mixed].category.as_deref(), Some("code"));
    assert_eq!(tree.nodes[0].category.as_deref(), Some("apps"));
    assert_eq!(tree.nodes[empty].category.as_deref(), Some("other"));
    assert_eq!(
        tree.view(mixed, 0, 100).unwrap().category.as_deref(),
        Some("code")
    );
    let breakdown = tree.breakdown(0).unwrap();
    assert_eq!(breakdown.iter().map(|c| c.bytes).sum::<u64>(), 150);
    assert_eq!(breakdown.iter().map(|c| c.files).sum::<u64>(), 3);
    assert_eq!(
        breakdown
            .iter()
            .find(|c| c.category == "apps")
            .unwrap()
            .bytes,
        90
    );
    assert_eq!(
        breakdown
            .iter()
            .find(|c| c.category == "code")
            .unwrap()
            .bytes,
        60
    );
}

#[test]
fn shared_category_mapping_includes_build_game_and_disk_assets() {
    assert_eq!(scanner::categories().len(), 8);
    let mut extensions = std::collections::HashSet::new();
    for category in scanner::categories() {
        for extension in &category.extensions {
            assert!(extensions.insert(extension));
            assert_eq!(
                scanner::category(&format!("file.{}", extension.to_uppercase())),
                category.id
            );
        }
    }
    for (name, expected) in [
        ("build.LIB", "code"),
        ("symbols.pdb", "code"),
        ("mod.pbo", "apps"),
        ("disk.vhdx", "archives"),
    ] {
        assert_eq!(scanner::category(name), expected);
    }
}

#[test]
fn cancelled_and_invalid_scan() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        scanner::scan(dir.path(), &AtomicBool::new(true), |_| {})
            .unwrap()
            .is_none()
    );
    assert!(scanner::scan(&dir.path().join("missing"), &AtomicBool::new(false), |_| {}).is_err());
}

#[test]
fn event_payloads_match_the_ipc_contract() {
    let progress = crate::ScanEvent {
        scan_id: 7,
        data: scanner::Progress {
            files: 3,
            ..Default::default()
        },
    };
    let json = serde_json::to_value(progress).unwrap();
    assert_eq!(json["files"], 3);
    assert_eq!(json["scanId"], 7);
    assert_eq!(json["currentPath"], "");
    assert!(json.get("data").is_none());
    assert_eq!(
        serde_json::to_value(crate::ScanEvent {
            scan_id: 7,
            data: crate::Cancelled {}
        })
        .unwrap(),
        serde_json::json!({ "scanId": 7 })
    );
}

#[test]
fn empty_folder_and_progress_totals() {
    let dir = tempfile::tempdir().unwrap();
    let progress = std::sync::Mutex::new(Vec::new());
    let tree = scanner::scan(dir.path(), &AtomicBool::new(false), |p| {
        progress.lock().unwrap().push(p)
    })
    .unwrap()
    .unwrap();
    assert_eq!(
        (
            tree.nodes[0].size,
            tree.nodes[0].files,
            tree.nodes[0].folders,
            tree.errors
        ),
        (0, 0, 0, 0)
    );
    assert!(tree.view(0, 3, 100).unwrap().children.unwrap().is_empty());
    assert_eq!(progress.lock().unwrap().last().unwrap().bytes, 0);
}

#[test]
fn total_response_budget_preserves_each_folders_size() {
    let (_, mut tree) = fixture();
    tree.nodes.truncate(1);
    tree.nodes[0].children.clear();
    for i in 0..100 {
        let id = tree.nodes.len();
        tree.nodes[0].children.push(id);
        tree.nodes.push(scanner::Node {
            name: format!("folder{i}"),
            parent: Some(0),
            children: vec![],
            kind: Kind::Dir,
            size: 100,
            files: 100,
            folders: 0,
            category: None,
        });
        for j in 0..100 {
            let file = tree.nodes.len();
            tree.nodes[id].children.push(file);
            tree.nodes.push(scanner::Node {
                name: format!("file{j}"),
                parent: Some(id),
                children: vec![],
                kind: Kind::File,
                size: 1,
                files: 1,
                folders: 0,
                category: Some("other".into()),
            });
        }
    }
    tree.nodes[0].size = 10000;
    fn check(node: &scanner::ViewNode) -> usize {
        if let Some(children) = &node.children {
            assert_eq!(children.iter().map(|c| c.size).sum::<u64>(), node.size);
            1 + children.iter().map(check).sum::<usize>()
        } else {
            1
        }
    }
    let count = check(&tree.view(0, 99, 9999).unwrap());
    assert!(count <= 8008, "response contained {count} nodes");
}

#[test]
fn links_are_not_followed() {
    let (dir, _) = fixture();
    let link = dir.path().join("loop");
    #[cfg(windows)]
    let created = std::os::windows::fs::symlink_dir(dir.path(), &link).is_ok();
    #[cfg(unix)]
    let created = std::os::unix::fs::symlink(dir.path(), &link).is_ok();
    #[cfg(windows)]
    let created = created
        || std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(dir.path())
            .output()
            .is_ok_and(|o| o.status.success());
    if !created {
        eprintln!("Skipping link test: this account cannot create links or junctions");
        return;
    }
    let tree = scanner::scan(dir.path(), &AtomicBool::new(false), |_| {})
        .unwrap()
        .unwrap();
    assert_eq!((tree.nodes[0].size, tree.nodes[0].folders), (150, 2));
    assert!(tree.nodes.iter().all(|n| n.name != "loop"));
    assert!(scanner::scan(&link, &AtomicBool::new(false), |_| {}).is_err());
    fs::remove_dir(&link)
        .or_else(|_| fs::remove_file(&link))
        .unwrap();
}

#[test]
#[ignore = "manual real-directory smoke scan"]
fn smoke_real_directory() {
    let path =
        std::env::var("DISKMAPPER_SMOKE_PATH").unwrap_or_else(|_| r"C:\Windows\System32".into());
    let start = Instant::now();
    let tree = scanner::scan(Path::new(&path), &AtomicBool::new(false), |_| {})
        .unwrap()
        .unwrap();
    let root = &tree.nodes[0];
    let elapsed = start.elapsed().as_secs_f64();
    eprintln!(
        "SMOKE {path}: {:.3}s, {} files, {} folders, {} bytes, {} access errors, {:.0} files/sec",
        elapsed,
        root.files,
        root.folders,
        root.size,
        tree.errors,
        root.files as f64 / elapsed
    );
    assert_eq!(
        tree.breakdown(0)
            .unwrap()
            .iter()
            .map(|r| r.bytes)
            .sum::<u64>(),
        root.size
    );
}
