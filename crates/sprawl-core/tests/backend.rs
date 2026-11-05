use sprawl_core::{Backend, BackendEvent};
use std::{fs, sync::mpsc, time::Duration};

#[test]
fn scan_emits_progress_completion_and_saved_change_then_serves_tree() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("example.txt"), b"hello").unwrap();
    let (send, receive) = mpsc::channel();
    let backend = Backend::new(data.path().into(), None, move |event| {
        send.send(event).unwrap();
    });
    let scan_id = backend
        .start_scan(root.path().to_string_lossy().into())
        .unwrap();
    let mut progress = false;
    let mut complete = false;
    loop {
        match receive.recv_timeout(Duration::from_secs(30)).unwrap() {
            BackendEvent::ScanProgress(event) => {
                assert_eq!(event.scan_id, scan_id);
                assert!(!complete);
                progress = true;
            }
            BackendEvent::ScanComplete(event) => {
                assert_eq!(event.scan_id, scan_id);
                assert!(progress);
                assert_eq!(event.data.root.size, 5);
                assert_eq!(event.data.root.files, 1);
                complete = true;
            }
            BackendEvent::SavedScansChanged => {
                assert!(complete);
                break;
            }
            _ => panic!("Unexpected backend event"),
        }
    }
    let tree = backend.get_tree(None, 1, 100).unwrap();
    assert_eq!(tree.size, 5);
    let files = backend.get_largest_files(0, 50).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "example.txt");
    let saved = backend.list_saved_scans().unwrap();
    assert_eq!(saved.len(), 1);
    assert!(data.path().join("scans").is_dir());
    let loaded = backend
        .load_saved_scan(saved[0].path.clone(), saved[0].scanned_at)
        .unwrap();
    assert_eq!(loaded.root.size, tree.size);
    assert_eq!(
        backend.delete_item(0, true, 0).err().unwrap(),
        "The scan root can't be deleted."
    );
}

#[test]
fn launch_requests_remain_pending_until_ready() {
    let data = tempfile::tempdir().unwrap();
    let (send, receive) = mpsc::channel();
    let backend = Backend::new(data.path().into(), Some("initial".into()), move |event| {
        send.send(event).unwrap();
    });
    backend.request_scan("pending".into());
    assert!(receive.try_recv().is_err());
    assert_eq!(backend.take_launch_path().as_deref(), Some("pending"));
    assert_eq!(backend.take_launch_path(), None);
    backend.clone().request_scan("ready".into());
    assert!(
        matches!(receive.try_recv().unwrap(), BackendEvent::ScanRequested(path) if path == "ready")
    );
}
