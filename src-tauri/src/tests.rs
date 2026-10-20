use crate::{
    cache::{self, SavedScan},
    scanner::{self, Kind, Tree},
};
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

fn saved(tree: &Tree, scanned_at: u64) -> SavedScan {
    let root = &tree.nodes[0];
    SavedScan {
        version: cache::FORMAT_VERSION,
        path: tree.root_path.to_string_lossy().into_owned(),
        scanned_at,
        size: root.size,
        files: root.files,
        folders: root.folders,
        errors: tree.errors,
        drive_free: Some(100),
        drive_total: Some(1000),
    }
}

#[test]
fn drive_metadata_uses_the_longest_mount_prefix() {
    let drives = [r"C:\", r"C:\Mounted", r"D:\"].map(|mount| crate::Drive {
        mount: mount.into(),
        label: String::new(),
        total: 1000,
        free: 100,
    });
    for (path, expected) in [
        (r"c:\MOUNTED\folder", Some(r"C:\Mounted")),
        ("c:\\mounted/", Some(r"C:\Mounted")),
        (r"C:\MountedElsewhere", Some(r"C:\")),
        ("c:", Some(r"C:\")),
        (r"E:\folder", None),
    ] {
        assert_eq!(
            crate::drive_for_path(path, &drives).map(|d| d.mount.as_str()),
            expected
        );
    }
}

#[test]
fn saved_scan_round_trip() {
    let (_fixture, mut tree) = fixture();
    tree.errors = 3;
    tree.nodes[0].category = None;
    let dir = tempfile::tempdir().unwrap();
    let meta = saved(&tree, 123);
    cache::save(dir.path(), &tree, &meta).unwrap();
    let (loaded, loaded_meta) = cache::load(dir.path(), &meta.path).unwrap();
    assert_eq!(loaded_meta, meta);
    assert_eq!(loaded.nodes.len(), tree.nodes.len());
    assert_eq!(loaded.root_path, tree.root_path);
    assert_eq!(loaded.errors, tree.errors);
    for (actual, expected) in loaded.nodes.iter().zip(&tree.nodes) {
        assert_eq!(actual.name, expected.name);
        assert_eq!(actual.parent, expected.parent);
        assert_eq!(actual.children, expected.children);
        assert_eq!(actual.kind, expected.kind);
        assert_eq!(actual.size, expected.size);
        assert_eq!(actual.files, expected.files);
        assert_eq!(actual.folders, expected.folders);
        assert_eq!(actual.category, expected.category);
    }
    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(&tree).unwrap()
    );
    assert_eq!(
        serde_json::to_value(loaded.view(0, 3, 100).unwrap()).unwrap(),
        serde_json::to_value(tree.view(0, 3, 100).unwrap()).unwrap()
    );
    assert_eq!(
        cache::list(dir.path()).unwrap(),
        std::slice::from_ref(&meta)
    );
    let json: serde_json::Value = serde_json::from_slice(
        &fs::read(dir.path().join(format!("{}.json", cache::key(&meta.path)))).unwrap(),
    )
    .unwrap();
    assert_eq!(json["scannedAt"], 123);
    assert_eq!(json["driveFree"], 100);
}

#[test]
fn opening_matching_session_reuses_tree_and_panels() {
    use std::sync::{Arc, atomic::Ordering};
    let (_fixture, tree) = fixture();
    let meta = saved(&tree, 123);
    let tree = Arc::new(tree);
    let cancel = Arc::new(AtomicBool::new(false));
    let mut session = crate::Session {
        tree: Some(tree.clone()),
        saved: Some(meta.clone()),
        cancel: Some(cancel.clone()),
        ..Default::default()
    };
    let panels = session.panels.clone();
    let files = panels
        .largest
        .get_or_init(|| tree.largest(0, 50))
        .as_ref()
        .unwrap();
    let types = panels
        .breakdown
        .get_or_init(|| tree.breakdown(0))
        .as_ref()
        .unwrap();
    let loaded = session
        .open_saved(&format!("{}\\", meta.path.to_uppercase()), 123)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.saved, meta);
    assert_eq!(loaded.root.size, tree.nodes[0].size);
    assert_eq!(loaded.errors, tree.errors);
    assert_eq!(session.generation, 1);
    assert!(cancel.load(Ordering::Relaxed));
    assert!(session.cancel.is_none());
    assert!(Arc::ptr_eq(session.tree.as_ref().unwrap(), &tree));
    assert!(Arc::ptr_eq(&session.panels, &panels));
    assert_eq!(
        serde_json::to_value(files).unwrap(),
        serde_json::to_value(tree.largest(0, 50).unwrap()).unwrap()
    );
    assert_eq!(
        serde_json::to_value(types).unwrap(),
        serde_json::to_value(tree.breakdown(0).unwrap()).unwrap()
    );
    assert!(session.open_saved(&meta.path, 124).unwrap().is_none());
    assert!(session.open_saved("another path", 123).unwrap().is_none());
    session.saved = None;
    assert!(session.open_saved(&meta.path, 123).unwrap().is_none());
    session.saved = Some(meta.clone());
    session.tree = None;
    assert!(session.open_saved(&meta.path, 123).unwrap().is_none());
}

#[test]
fn saved_scan_keys_and_overwrite() {
    let (_fixture, mut tree) = fixture();
    let dir = tempfile::tempdir().unwrap();
    tree.root_path = r"C:\Example".into();
    let first = saved(&tree, 1);
    cache::save(dir.path(), &tree, &first).unwrap();
    tree.root_path = "c:\\EXAMPLE\\".into();
    tree.nodes[0].size += 1;
    let second = saved(&tree, 2);
    assert_eq!(cache::key(&first.path), cache::key(&second.path));
    assert_eq!(cache::key("HELLO//"), "a430d84680aabd0b");
    fs::write(dir.path().join("orphan.tree"), b"orphan").unwrap();
    fs::write(dir.path().join("orphan.tree.tmp"), b"partial").unwrap();
    cache::save(dir.path(), &tree, &second).unwrap();
    assert_eq!(
        cache::list(dir.path()).unwrap(),
        std::slice::from_ref(&second)
    );
    assert_eq!(cache::load(dir.path(), &first.path).unwrap().1, second);
    cache::save(dir.path(), &tree, &first).unwrap();
    assert_eq!(cache::load(dir.path(), &first.path).unwrap().1, second);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn saved_scan_retention_keeps_newest_ten() {
    let (_fixture, mut tree) = fixture();
    let dir = tempfile::tempdir().unwrap();
    for i in (0..12).rev() {
        tree.root_path = format!("C:\\folder{i}").into();
        cache::save(dir.path(), &tree, &saved(&tree, i)).unwrap();
    }
    let scans = cache::list(dir.path()).unwrap();
    assert_eq!(
        scans.iter().map(|s| s.scanned_at).collect::<Vec<_>>(),
        (2..12).rev().collect::<Vec<_>>()
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 20);
    for scan in scans {
        assert!(cache::load(dir.path(), &scan.path).is_ok());
    }
}

#[test]
fn invalid_saved_scans_and_orphans_are_removed() {
    let (_fixture, tree) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let meta = saved(&tree, 1);
    let key = cache::key(&meta.path);
    let tree_path = dir.path().join(format!("{key}.tree"));
    let meta_path = tree_path.with_extension("json");
    for fault in [
        "tree",
        "header version",
        "meta",
        "meta version",
        "missing tree",
    ] {
        for listing_first in [false, true] {
            cache::save(dir.path(), &tree, &meta).unwrap();
            match fault {
                "tree" => fs::write(&tree_path, b"corrupt").unwrap(),
                "header version" => {
                    let mut bytes = fs::read(&tree_path).unwrap();
                    bytes[8..12].copy_from_slice(&(cache::FORMAT_VERSION + 1).to_le_bytes());
                    fs::write(&tree_path, bytes).unwrap();
                }
                "meta" => fs::write(&meta_path, b"bad json").unwrap(),
                "meta version" => fs::write(
                    &meta_path,
                    serde_json::to_vec(&SavedScan {
                        version: cache::FORMAT_VERSION + 1,
                        ..meta.clone()
                    })
                    .unwrap(),
                )
                .unwrap(),
                _ => fs::remove_file(&tree_path).unwrap(),
            }
            if listing_first {
                assert!(cache::list(dir.path()).unwrap().is_empty(), "{fault}");
            }
            assert_eq!(
                cache::load(dir.path(), &meta.path).err().unwrap(),
                "This saved scan can't be opened. Scan again to refresh it."
            );
            assert!(!tree_path.exists());
            assert!(!meta_path.exists());
            assert!(cache::list(dir.path()).unwrap().is_empty());
        }
    }
    fs::write(dir.path().join("orphan.tree"), b"orphan").unwrap();
    fs::write(dir.path().join("orphan.tree.tmp"), b"partial").unwrap();
    fs::write(dir.path().join("orphan.json.tmp"), b"partial").unwrap();
    assert!(cache::list(dir.path()).unwrap().is_empty());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn corrupt_saved_payloads_fail_cleanly() {
    let (_fixture, mut tree) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let meta = saved(&tree, 1);
    let path = dir.path().join(format!("{}.tree", cache::key(&meta.path)));
    for truncated in [false, true] {
        cache::save(dir.path(), &tree, &meta).unwrap();
        let mut bytes = fs::read(&path).unwrap();
        if truncated {
            bytes.truncate(bytes.len() - 4);
        } else {
            *bytes.last_mut().unwrap() ^= 1;
        }
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            cache::list(dir.path()).unwrap(),
            std::slice::from_ref(&meta)
        );
        assert_eq!(
            cache::load(dir.path(), &meta.path).err().unwrap(),
            "This saved scan can't be opened. Scan again to refresh it."
        );
        assert!(!path.exists());
        assert!(!path.with_extension("json").exists());
        assert!(cache::list(dir.path()).unwrap().is_empty());
    }
    for fault in ["category index", "trailing bytes", "node count"] {
        cache::save(dir.path(), &tree, &meta).unwrap();
        let mut bytes = fs::read(&path).unwrap();
        let mut payload = zstd::stream::decode_all(&bytes[12..]).unwrap();
        match fault {
            "category index" => *payload.last_mut().unwrap() = u8::MAX,
            "trailing bytes" => payload.push(0),
            _ => payload[0] = 0,
        }
        bytes.truncate(12);
        bytes.extend(zstd::stream::encode_all(payload.as_slice(), 3).unwrap());
        fs::write(&path, bytes).unwrap();
        assert!(cache::load(dir.path(), &meta.path).is_err(), "{fault}");
        assert!(!path.exists());
        assert!(!path.with_extension("json").exists());
    }
    tree.nodes[0].children.push(usize::MAX);
    cache::save(dir.path(), &tree, &meta).unwrap();
    assert!(cache::load(dir.path(), &meta.path).is_err());
    assert!(cache::list(dir.path()).unwrap().is_empty());
}

#[test]
fn clear_saved_scans_removes_everything() {
    let (_fixture, tree) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let scans = dir.path().join("scans");
    assert!(cache::list(&scans).unwrap().is_empty());
    cache::clear(&scans).unwrap();
    cache::save(&scans, &tree, &saved(&tree, 1)).unwrap();
    fs::write(scans.join("unfinished.tmp"), b"partial").unwrap();
    fs::create_dir(scans.join("leftover")).unwrap();
    fs::write(scans.join("leftover/file"), b"partial").unwrap();
    cache::clear(&scans).unwrap();
    assert!(cache::list(&scans).unwrap().is_empty());
    assert!(!scans.exists());
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
