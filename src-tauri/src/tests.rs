use crate::{
    admin::{self, Settings},
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
    let tree = scanner::scan(dir.path(), None, &AtomicBool::new(false), |_| {})
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
        recycled: 0,
        deleted: 0,
        errors: tree.errors,
        drive_free: Some(100),
        drive_total: Some(1000),
    }
}

#[test]
fn settings_default_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!admin::read_settings(&dir.path().join("settings.json")).always_elevate);
}

#[test]
fn settings_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data/settings.json");
    for enabled in [true, false, true] {
        admin::write_settings(
            &path,
            &Settings {
                always_elevate: enabled,
            },
        )
        .unwrap();
        assert_eq!(admin::read_settings(&path).always_elevate, enabled);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap(),
            serde_json::json!({ "alwaysElevate": enabled })
        );
        assert!(!path.with_extension("json.tmp").exists());
    }
}

#[test]
fn settings_default_when_corrupt_or_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    for contents in [
        "not json",
        "{",
        "{}",
        r#"{"alwaysElevate": "yes"}"#,
        r#"{"alwaysElevate": true, "unknown": 1}"#,
    ] {
        fs::write(&path, contents).unwrap();
        assert!(!admin::read_settings(&path).always_elevate);
    }
}

#[cfg(windows)]
#[test]
fn elevated_arguments_preserve_windows_quoting() {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{Foundation::LocalFree, UI::Shell::CommandLineToArgvW};

    let args = [
        "",
        "plain",
        "two words",
        r#"a"b"#,
        r"C:\with spaces\",
        "日本語",
    ];
    let mut command: Vec<u16> = "app.exe ".encode_utf16().collect();
    for arg in args {
        command.extend(admin::quote_arg(std::ffi::OsStr::new(arg)));
        command.push(b' ' as u16);
    }
    command.push(0);
    let mut count = 0;
    unsafe {
        let parsed = CommandLineToArgvW(command.as_ptr(), &mut count);
        assert!(!parsed.is_null());
        assert_eq!(count as usize, args.len() + 1);
        for (index, arg) in args.iter().enumerate() {
            let ptr = *parsed.add(index + 1);
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            assert_eq!(
                std::ffi::OsString::from_wide(std::slice::from_raw_parts(ptr, len)),
                *arg
            );
        }
        LocalFree(parsed.cast());
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
    let meta = SavedScan {
        recycled: 12,
        deleted: 34,
        ..saved(&tree, 123)
    };
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
    assert_eq!(json["recycled"], 12);
    assert_eq!(json["deleted"], 34);
}

#[test]
fn saved_mft_records_remain_aligned_after_remove_and_compaction() {
    let (_fixture, mut tree) = fixture();
    tree.records = (0..tree.nodes.len())
        .map(|id| if id == 0 { 5 } else { 24 + id as u64 })
        .collect();
    tree.checkpoint = Some(scanner::JournalCheckpoint {
        journal_id: 42,
        next_usn: 1234,
    });
    let original_records = tree.records.clone();
    let dir = tempfile::tempdir().unwrap();
    cache::save(dir.path(), &tree, &saved(&tree, 1)).unwrap();
    let (loaded, _) = cache::load(dir.path(), &tree.root_path.to_string_lossy()).unwrap();
    assert_eq!(loaded.records, tree.records);
    assert_eq!(loaded.checkpoint, tree.checkpoint);
    let nested = tree.nodes.iter().position(|n| n.name == "nested").unwrap();
    tree.remove(nested).unwrap();
    assert_eq!(tree.records, original_records);
    assert_eq!(tree.records.len(), tree.nodes.len());
    cache::save(dir.path(), &tree, &saved(&tree, 1)).unwrap();
    let (loaded, _) = cache::load(dir.path(), &tree.root_path.to_string_lossy()).unwrap();
    assert_eq!(loaded.records.len(), loaded.nodes.len());
    assert_eq!(loaded.nodes.len(), tree.nodes.len() - 2);
    assert_eq!(loaded.checkpoint, tree.checkpoint);
    for (id, node) in loaded.nodes.iter().enumerate() {
        let original = tree.nodes.iter().position(|n| n.name == node.name).unwrap();
        assert_eq!(loaded.records[id], original_records[original]);
    }
}

#[test]
fn invalid_mft_cache_metadata_is_rejected() {
    let (_fixture, mut tree) = fixture();
    let dir = tempfile::tempdir().unwrap();
    for fault in [
        "missing records",
        "root record",
        "record range",
        "negative checkpoint",
        "duplicate directory",
        "mixed record kinds",
        "different link sizes",
    ] {
        tree.records = (0..tree.nodes.len())
            .map(|id| if id == 0 { 5 } else { 24 + id as u64 })
            .collect();
        tree.checkpoint = Some(scanner::JournalCheckpoint {
            journal_id: 42,
            next_usn: 1234,
        });
        match fault {
            "missing records" => tree.records.clear(),
            "root record" => tree.records[0] = 6,
            "record range" => tree.records[1] = 1 << 48,
            "negative checkpoint" => tree.checkpoint.as_mut().unwrap().next_usn = -1,
            _ => {
                let name = if fault == "different link sizes" {
                    "large.MP4"
                } else {
                    "nested"
                };
                let duplicate = if fault == "duplicate directory" {
                    "empty"
                } else {
                    "small.txt"
                };
                let source = tree.nodes.iter().position(|n| n.name == name).unwrap();
                let target = tree.nodes.iter().position(|n| n.name == duplicate).unwrap();
                tree.records[target] = tree.records[source];
            }
        }
        cache::save(dir.path(), &tree, &saved(&tree, 1)).unwrap();
        assert!(
            cache::load(dir.path(), &tree.root_path.to_string_lossy()).is_err(),
            "{fault}"
        );
    }
    tree.records = vec![5];
    assert!(cache::save(dir.path(), &tree, &saved(&tree, 1)).is_err());
}

#[test]
fn remove_updates_ancestors_order_counts_and_rejects_detached_descendants() {
    let mut tree = comparison_tree(&[
        ("a", 0, Kind::Dir, 0),
        ("b", 0, Kind::Dir, 0),
        ("nested", 1, Kind::Dir, 0),
        ("peer", 1, Kind::File, 20),
        ("subtree", 3, Kind::Dir, 0),
        ("gone", 5, Kind::File, 100),
        ("stay", 3, Kind::File, 10),
        ("other", 2, Kind::File, 40),
    ]);
    for id in 0..tree.nodes.len() {
        let mut children = std::mem::take(&mut tree.nodes[id].children);
        children.sort_by_key(|&id| std::cmp::Reverse(tree.nodes[id].size));
        tree.nodes[id].children = children;
    }
    let original = tree.clone();
    assert_eq!(tree.nodes[0].children, vec![1, 2]);
    assert_eq!(tree.nodes[1].children, vec![3, 4]);
    tree.remove(5).unwrap();
    for (id, expected) in [(0, (70, 3, 3)), (1, (30, 2, 1)), (3, (10, 1, 0))] {
        let n = tree.node(id).unwrap();
        assert_eq!((n.size, n.files, n.folders), expected);
    }
    assert_eq!(tree.nodes[0].children, vec![2, 1]);
    assert_eq!(tree.nodes[1].children, vec![4, 3]);
    assert_eq!(tree.nodes[3].children, vec![7]);
    assert_eq!(tree.nodes.len(), original.nodes.len());
    assert_eq!(tree.nodes[5].parent, None);
    assert_eq!(tree.nodes[6].parent, Some(5));
    for id in [5, 6] {
        assert_eq!(tree.node(id).err().unwrap(), "This item was deleted.");
        assert_eq!(tree.path(id).unwrap_err(), "This item was deleted.");
        assert_eq!(tree.view(id, 0, 0).unwrap_err(), "This item was deleted.");
        assert_eq!(tree.largest(id, 50).unwrap_err(), "This item was deleted.");
        assert_eq!(tree.breakdown(id).unwrap_err(), "This item was deleted.");
        assert_eq!(tree.remove(id).unwrap_err(), "This item was deleted.");
    }
    assert_eq!(original.nodes[0].size, 170);
    assert!(original.node(6).is_ok());
    tree.remove(4).unwrap();
    assert_eq!(
        (
            tree.nodes[0].size,
            tree.nodes[0].files,
            tree.nodes[0].folders
        ),
        (50, 2, 3)
    );
    tree.remove(1).unwrap();
    assert_eq!(
        (
            tree.nodes[0].size,
            tree.nodes[0].files,
            tree.nodes[0].folders
        ),
        (40, 1, 1)
    );
    assert!(tree.remove(0).is_err());
    assert!(tree.remove(usize::MAX).is_err());
}

#[test]
fn resave_removed_subtree_compacts_without_rotating_or_losing_recycled() {
    let (_fixture, mut tree) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let old = saved(&tree, 1);
    cache::save(dir.path(), &tree, &old).unwrap();
    let original = saved(&tree, 2);
    cache::save(dir.path(), &tree, &original).unwrap();
    let previous = dir.path().join("previous");
    let previous_tree = fs::read(previous.join(format!("{}.tree", cache::key(&old.path)))).unwrap();
    let nested = tree.nodes.iter().position(|n| n.name == "nested").unwrap();
    let removed = tree.nodes[nested].size;
    tree.remove(nested).unwrap();
    let meta = SavedScan {
        recycled: removed,
        ..saved(&tree, 2)
    };
    cache::save(dir.path(), &tree, &meta).unwrap();
    let (loaded, actual) = cache::load(dir.path(), &meta.path).unwrap();
    assert_eq!(actual, meta);
    assert_eq!(loaded.nodes.len(), tree.nodes.len() - 2);
    assert!(
        loaded
            .nodes
            .iter()
            .all(|n| n.name != "nested" && n.name != "code.rs")
    );
    assert_eq!(
        (
            loaded.nodes[0].size,
            loaded.nodes[0].files,
            loaded.nodes[0].folders
        ),
        (110, 2, 1)
    );
    assert_eq!(
        loaded.nodes[0]
            .children
            .iter()
            .map(|&id| loaded.nodes[id].size)
            .collect::<Vec<_>>(),
        vec![100, 10, 0]
    );
    assert_eq!(
        loaded.nodes.iter().map(|n| &n.name).collect::<Vec<_>>(),
        tree.nodes
            .iter()
            .enumerate()
            .filter(|(id, _)| tree.node(*id).is_ok())
            .map(|(_, n)| &n.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(cache::load(&previous, &meta.path).unwrap().1, old);
    assert_eq!(
        fs::read(previous.join(format!("{}.tree", cache::key(&old.path)))).unwrap(),
        previous_tree
    );
    cache::save(dir.path(), &tree, &original).unwrap();
    assert_eq!(cache::load(dir.path(), &meta.path).unwrap().1, meta);
    let fresh = tempfile::tempdir().unwrap();
    cache::save(fresh.path(), &tree, &meta).unwrap();
    cache::save(fresh.path(), &tree, &meta).unwrap();
    assert!(!fresh.path().join("previous").exists());
}

#[test]
fn saved_scan_without_deletion_totals_is_backwards_compatible() {
    let (_fixture, tree) = fixture();
    let mut json = serde_json::to_value(saved(&tree, 1)).unwrap();
    json.as_object_mut().unwrap().remove("recycled");
    json.as_object_mut().unwrap().remove("deleted");
    let meta = serde_json::from_value::<SavedScan>(json).unwrap();
    assert_eq!((meta.recycled, meta.deleted), (0, 0));
    let mut json = serde_json::to_value(meta).unwrap();
    json["recycled"] = 40.into();
    json.as_object_mut().unwrap().remove("deleted");
    let meta = serde_json::from_value::<SavedScan>(json).unwrap();
    assert_eq!((meta.recycled, meta.deleted), (40, 0));
}

#[test]
fn stale_background_save_cannot_reduce_either_deletion_total() {
    let (_fixture, tree) = fixture();
    let original = saved(&tree, 123);
    for permanent in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut meta = original.clone();
        if permanent {
            meta.deleted = 40;
        } else {
            meta.recycled = 40;
        }
        cache::save(dir.path(), &tree, &meta).unwrap();
        cache::save(dir.path(), &tree, &original).unwrap();
        assert_eq!(cache::load(dir.path(), &meta.path).unwrap().1, meta);
    }
}

#[test]
fn stale_background_save_cannot_restore_a_zero_byte_item() {
    for name in ["empty", "zero.txt"] {
        let (fixture, _) = fixture();
        fs::write(fixture.path().join("zero.txt"), []).unwrap();
        let mut tree = scanner::scan(fixture.path(), None, &AtomicBool::new(false), |_| {})
            .unwrap()
            .unwrap();
        let original = tree.clone();
        let old = saved(&original, 123);
        let dir = tempfile::tempdir().unwrap();
        cache::save(dir.path(), &tree, &old).unwrap();
        let id = tree.nodes.iter().position(|n| n.name == name).unwrap();
        tree.remove(id).unwrap();
        let meta = saved(&tree, 123);
        assert_eq!(meta.recycled, old.recycled);
        assert_eq!(meta.deleted, old.deleted);
        cache::save(dir.path(), &tree, &meta).unwrap();
        cache::save(dir.path(), &original, &old).unwrap();
        let (loaded, actual) = cache::load(dir.path(), &meta.path).unwrap();
        assert_eq!(actual, meta);
        assert!(loaded.nodes.iter().all(|n| n.name != name));
    }
}

#[test]
fn recycle_refuses_a_changed_kind_or_missing_path() {
    let (dir, tree) = fixture();
    for (path, kind) in [
        (dir.path().join("small.txt"), Kind::Dir),
        (dir.path().join("nested"), Kind::File),
        (dir.path().join("missing"), Kind::File),
    ] {
        assert_eq!(
            crate::recycle::check(&path, kind).unwrap_err(),
            "This item has changed. Rescan to refresh it."
        );
    }
    assert!(crate::recycle::check(&tree.root_path.join("small.txt"), Kind::File).is_ok());
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
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    assert_eq!(
        cache::load(&dir.path().join("previous"), &first.path)
            .unwrap()
            .1,
        first
    );
}

#[test]
fn saved_scan_rotation_and_baselines() {
    let (_fixture, mut tree) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let previous = dir.path().join("previous");
    let first = saved(&tree, 1);
    assert!(cache::baseline(dir.path(), &first).unwrap().is_none());
    cache::save(dir.path(), &tree, &first).unwrap();
    assert!(cache::baseline(dir.path(), &first).unwrap().is_none());
    tree.nodes[0].size += 10;
    let second = saved(&tree, 2);
    assert_eq!(
        cache::baseline(dir.path(), &second).unwrap().unwrap().1,
        first
    );
    cache::save(dir.path(), &tree, &second).unwrap();
    assert_eq!(cache::load(&previous, &first.path).unwrap().1, first);
    assert_eq!(
        cache::baseline(dir.path(), &second).unwrap().unwrap().1,
        first
    );
    tree.nodes[0].size += 10;
    let third = saved(&tree, 3);
    assert_eq!(
        cache::baseline(dir.path(), &third).unwrap().unwrap().1,
        second
    );
    cache::save(dir.path(), &tree, &third).unwrap();
    assert_eq!(cache::load(&previous, &first.path).unwrap().1, second);
    assert_eq!(
        cache::baseline(dir.path(), &third).unwrap().unwrap().1,
        second
    );
    assert!(cache::baseline(dir.path(), &first).unwrap().is_none());
    cache::save(dir.path(), &tree, &third).unwrap();
    assert_eq!(cache::load(&previous, &first.path).unwrap().1, second);
    assert_eq!(cache::list(dir.path()).unwrap(), vec![third]);
    assert!(previous.is_dir());
    assert_eq!(cache::list(&previous).unwrap(), vec![second]);
    assert_eq!(fs::read_dir(&previous).unwrap().count(), 2);
}

#[test]
fn eviction_removes_previous_and_orphaned_baselines() {
    let (_fixture, mut tree) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let previous = dir.path().join("previous");
    let first = saved(&tree, 1);
    cache::save(dir.path(), &tree, &first).unwrap();
    cache::save(dir.path(), &tree, &saved(&tree, 2)).unwrap();
    let key = cache::key(&first.path);
    assert!(previous.join(format!("{key}.tree")).is_file());
    tree.root_path = "C:\\orphan".into();
    cache::save(&previous, &tree, &saved(&tree, 1)).unwrap();
    let orphan = cache::key(&saved(&tree, 1).path);
    for i in 3..13 {
        tree.root_path = format!("C:\\folder{i}").into();
        cache::save(dir.path(), &tree, &saved(&tree, i)).unwrap();
    }
    for key in [key, orphan] {
        assert!(!previous.join(format!("{key}.tree")).exists());
        assert!(!previous.join(format!("{key}.json")).exists());
    }
    assert_eq!(cache::list(dir.path()).unwrap().len(), 10);
    assert!(cache::list(&previous).unwrap().is_empty());
}

#[test]
fn interrupted_rotation_cannot_load_a_partial_pair() {
    let (_fixture, mut tree) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let first = saved(&tree, 1);
    cache::save(dir.path(), &tree, &first).unwrap();
    tree.nodes[0].size += 1;
    cache::save(dir.path(), &tree, &saved(&tree, 2)).unwrap();
    let key = cache::key(&first.path);
    let previous = dir.path().join("previous");
    for extension in ["tree", "json"] {
        fs::remove_file(previous.join(format!("{key}.{extension}"))).unwrap();
    }
    fs::rename(
        dir.path().join(format!("{key}.tree")),
        previous.join(format!("{key}.tree")),
    )
    .unwrap();
    assert!(cache::load(&previous, &first.path).is_err());
    assert!(cache::load(dir.path(), &first.path).is_err());
    assert!(cache::list(dir.path()).unwrap().is_empty());
    assert!(cache::list(&previous).unwrap().is_empty());
}

fn comparison_tree(entries: &[(&str, usize, Kind, u64)]) -> Tree {
    let mut tree = Tree {
        nodes: vec![scanner::Node {
            name: "root".into(),
            parent: None,
            children: vec![],
            kind: Kind::Dir,
            size: 0,
            files: 0,
            folders: 0,
            category: None,
        }],
        root_path: "C:\\root".into(),
        errors: 0,
        records: vec![],
        checkpoint: None,
    };
    for &(name, parent, kind, size) in entries {
        let id = tree.nodes.len();
        tree.nodes.push(scanner::Node {
            name: name.into(),
            parent: Some(parent),
            children: vec![],
            kind,
            size,
            files: u64::from(kind == Kind::File),
            folders: 0,
            category: None,
        });
        tree.nodes[parent].children.push(id);
    }
    for id in (1..tree.nodes.len()).rev() {
        let (size, files, folders) = (
            tree.nodes[id].size,
            tree.nodes[id].files,
            tree.nodes[id].folders + u64::from(tree.nodes[id].kind == Kind::Dir),
        );
        let parent = tree.nodes[id].parent.unwrap();
        tree.nodes[parent].size += size;
        tree.nodes[parent].files += files;
        tree.nodes[parent].folders += folders;
    }
    tree
}

#[test]
fn comparison_matches_names_and_kinds_and_scopes_removals() {
    use crate::changes::Comparison;
    let previous = comparison_tree(&[
        ("grown", 0, Kind::File, 10),
        ("shrunk", 0, Kind::File, 100),
        ("removed", 0, Kind::File, 7),
        ("swap", 0, Kind::Dir, 0),
        ("inside", 4, Kind::File, 9),
        ("nested", 0, Kind::Dir, 0),
        ("gone", 6, Kind::File, 4),
        ("Case", 0, Kind::File, 8),
        ("empty", 0, Kind::Dir, 0),
        ("file-to-dir", 0, Kind::File, 3),
    ]);
    let current = comparison_tree(&[
        ("nested", 0, Kind::Dir, 0),
        ("new", 1, Kind::File, 50),
        ("grown", 0, Kind::File, 30),
        ("shrunk", 0, Kind::File, 60),
        ("swap", 0, Kind::File, 5),
        ("case", 0, Kind::File, 8),
        ("empty", 0, Kind::Dir, 0),
        ("zero", 0, Kind::File, 0),
        ("file-to-dir", 0, Kind::Dir, 0),
        ("inside", 9, Kind::File, 2),
    ]);
    let comparison = Comparison::new(&current, &previous, 123);
    let root = comparison.view(&current, 0).unwrap();
    assert_eq!(root.since, 123);
    assert_eq!(root.previous_size, Some(previous.nodes[0].size));
    assert_eq!(
        root.children
            .iter()
            .map(|c| (c.id, c.previous_size))
            .collect::<Vec<_>>(),
        vec![
            (1, Some(4)),
            (3, Some(10)),
            (4, Some(100)),
            (5, None),
            (6, None),
            (7, Some(0)),
            (8, None),
            (9, None)
        ]
    );
    assert_eq!((root.removed.items, root.removed.bytes), (4, 27));
    assert_eq!(
        root.grown
            .iter()
            .map(|f| (f.name.as_str(), f.previous_size))
            .collect::<Vec<_>>(),
        vec![
            ("new", None),
            ("grown", Some(10)),
            ("case", None),
            ("swap", None),
            ("inside", None)
        ]
    );
    assert_eq!(root.grown[0].relative_path, r"nested\new");
    let nested = comparison.view(&current, 1).unwrap();
    assert_eq!((nested.removed.items, nested.removed.bytes), (1, 4));
    assert_eq!(nested.grown[0].relative_path, "new");
    assert_eq!(comparison.view(&current, 9).unwrap().previous_size, None);
    assert_eq!(comparison.view(&current, 9).unwrap().removed.items, 0);
    assert!(comparison.view(&current, 7).unwrap().grown.is_empty());
    assert!(comparison.view(&current, usize::MAX).is_err());
    let json = serde_json::to_value(root).unwrap();
    assert_eq!(json["children"][0]["previousSize"], 4);
    assert!(json["grown"][0]["previousSize"].is_null());
}

#[test]
fn grown_files_are_ranked_by_growth_and_limited_to_fifty() {
    let names: Vec<_> = (0..70).map(|i| format!("file{i:02}")).collect();
    let old: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), 0, Kind::File, 1000))
        .collect();
    let new: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.as_str(), 0, Kind::File, 1000 + i as u64))
        .collect();
    let previous = comparison_tree(&old);
    let current = comparison_tree(&new);
    let comparison = crate::changes::Comparison::new(&current, &previous, 1);
    let grown = comparison.view(&current, 0).unwrap().grown;
    assert_eq!(grown.len(), 50);
    assert_eq!(grown.first().unwrap().name, "file69");
    assert_eq!(grown.last().unwrap().name, "file20");
    assert!(grown.windows(2).all(|pair| pair[0].size > pair[1].size));
    assert!(comparison.view(&current, 1).unwrap().grown.is_empty());
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
    cache::save(&scans, &tree, &saved(&tree, 2)).unwrap();
    assert_eq!(cache::list(&scans.join("previous")).unwrap().len(), 1);
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
    assert_eq!(
        scanner::categories()[usize::from(scanner::category("unknown.ext"))].id,
        "other"
    );
}

#[test]
fn dominant_folder_category_uses_all_descendant_bytes() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("mixed")).unwrap();
    fs::create_dir(dir.path().join("empty")).unwrap();
    fs::write(dir.path().join("mixed/build.rlib"), [0; 60]).unwrap();
    fs::write(dir.path().join("mixed/game.pbo"), [0; 40]).unwrap();
    fs::write(dir.path().join("more-game.uasset"), [0; 50]).unwrap();
    let tree = scanner::scan(dir.path(), None, &AtomicBool::new(false), |_| {})
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
    assert_eq!(
        tree.nodes[mixed].category,
        Some(scanner::category("file.rs"))
    );
    assert_eq!(tree.nodes[0].category, Some(scanner::category("file.exe")));
    assert_eq!(tree.nodes[empty].category, Some(scanner::category("file")));
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
                scanner::categories()[usize::from(scanner::category(&format!(
                    "file.{}",
                    extension.to_uppercase()
                )))]
                .id,
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
        assert_eq!(
            scanner::categories()[usize::from(scanner::category(name))].id,
            expected
        );
    }
}

#[test]
fn cancelled_and_invalid_scan() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        scanner::scan(dir.path(), None, &AtomicBool::new(true), |_| {})
            .unwrap()
            .is_none()
    );
    assert!(
        scanner::scan(
            &dir.path().join("missing"),
            None,
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err()
    );
}

#[test]
fn traversal_is_independent_of_worker_count() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..1100 {
        fs::write(dir.path().join(format!("file{i:04}.TxT")), [0; 3]).unwrap();
    }
    for i in 0..32 {
        let nested = dir.path().join(format!("folder{i:02}"));
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("equal.MP4"), [0; 3]).unwrap();
        fs::write(nested.join("equal.txt"), [0; 3]).unwrap();
    }
    fn without_ids(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(object) => {
                object.remove("id");
                for value in object.values_mut() {
                    without_ids(value);
                }
            }
            serde_json::Value::Array(values) => values.iter_mut().for_each(without_ids),
            _ => (),
        }
    }
    let mut expected = None;
    for workers in [1, 2, 4] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap();
        let progress = std::sync::Mutex::new(scanner::Progress::default());
        let tree = pool.install(|| {
            scanner::scan(dir.path(), None, &AtomicBool::new(false), |p| {
                *progress.lock().unwrap() = p;
            })
            .unwrap()
            .unwrap()
        });
        let root = &tree.nodes[0];
        assert_eq!(
            (root.size, root.files, root.folders, tree.errors),
            (3492, 1164, 32, 0)
        );
        let progress = progress.into_inner().unwrap();
        assert_eq!(
            (
                progress.bytes,
                progress.files,
                progress.folders,
                progress.errors
            ),
            (3492, 1164, 32, 0)
        );
        assert_eq!(progress.current_path, dir.path().to_string_lossy());
        for (id, node) in tree.nodes.iter().enumerate() {
            if let Some(parent) = node.parent {
                assert!(parent < id);
                assert!(tree.nodes[parent].children.contains(&id));
            }
            assert!(node.children.windows(2).all(|pair| {
                let a = &tree.nodes[pair[0]];
                let b = &tree.nodes[pair[1]];
                a.size > b.size || (a.size == b.size && a.name <= b.name)
            }));
        }
        let largest = tree.largest(0, 50).unwrap();
        assert_eq!(largest.first().unwrap().relative_path, "file0000.TxT");
        assert_eq!(largest.last().unwrap().relative_path, "file0049.TxT");
        assert_eq!(largest[0].category, "documents");
        let mut actual = serde_json::json!({
            "view": tree.view(0, 3, 500).unwrap(),
            "largest": largest,
            "breakdown": tree.breakdown(0).unwrap(),
        });
        without_ids(&mut actual);
        if let Some(expected) = &expected {
            assert_eq!(&actual, expected);
        } else {
            expected = Some(actual);
        }
    }
}

#[test]
fn deep_directory_chain_on_one_worker() {
    let dir = tempfile::tempdir().unwrap();
    let mut path = dir.path().to_owned();
    for _ in 0..96 {
        path.push("d");
        fs::create_dir(&path).unwrap();
    }
    fs::write(path.join("leaf.rs"), [0; 7]).unwrap();
    let tree = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| {
            scanner::scan(dir.path(), None, &AtomicBool::new(false), |_| {})
                .unwrap()
                .unwrap()
        });
    assert_eq!(
        (
            tree.nodes[0].size,
            tree.nodes[0].files,
            tree.nodes[0].folders
        ),
        (7, 1, 96)
    );
    assert_eq!(
        tree.path(tree.nodes.len() - 1).unwrap(),
        tree.root_path
            .join(path.strip_prefix(dir.path()).unwrap())
            .join("leaf.rs")
    );
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
    let tree = scanner::scan(dir.path(), None, &AtomicBool::new(false), |p| {
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
                category: Some(scanner::category("file")),
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
    assert!(crate::recycle::check(&dir.path().join("nested"), Kind::Dir).is_ok());
    assert!(crate::recycle::check(&dir.path().join("nested/code.rs"), Kind::File).is_ok());
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
    let tree = scanner::scan(dir.path(), None, &AtomicBool::new(false), |_| {})
        .unwrap()
        .unwrap();
    assert_eq!((tree.nodes[0].size, tree.nodes[0].folders), (150, 2));
    assert!(tree.nodes.iter().all(|n| n.name != "loop"));
    assert!(scanner::scan(&link, None, &AtomicBool::new(false), |_| {}).is_err());
    assert_eq!(
        crate::recycle::check(&link, Kind::Dir).unwrap_err(),
        "This item has changed. Rescan to refresh it."
    );
    for (path, kind) in [
        (link.join("nested"), Kind::Dir),
        (link.join("nested/code.rs"), Kind::File),
    ] {
        assert!(!scanner::excluded(&fs::symlink_metadata(&path).unwrap()));
        assert_eq!(
            crate::recycle::check(&path, kind).unwrap_err(),
            "This item has changed. Rescan to refresh it."
        );
    }
    fs::remove_dir(&link)
        .or_else(|_| fs::remove_file(&link))
        .unwrap();
}

#[test]
#[ignore = "manual real-directory smoke scan"]
fn smoke_real_directory() {
    let path =
        std::env::var("SPRAWL_SMOKE_PATH").unwrap_or_else(|_| r"C:\Windows\System32".into());
    let start = Instant::now();
    let tree = scanner::scan(Path::new(&path), None, &AtomicBool::new(false), |_| {})
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
