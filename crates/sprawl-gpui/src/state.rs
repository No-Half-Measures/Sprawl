use sprawl_core::{BackendEvent, scanner::Progress};

#[derive(Clone, Copy, Default)]
pub struct LaunchState {
    pub opening: bool,
    pub prompt_open: bool,
    pub delete_dialog: bool,
    pub delete_running: bool,
    pub delete_pending: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LaunchDecision {
    Start,
    Hold,
    Ignore,
}

pub fn launch_decision(state: LaunchState) -> LaunchDecision {
    if state.delete_dialog || state.delete_running || state.delete_pending {
        LaunchDecision::Ignore
    } else if state.opening || state.prompt_open {
        LaunchDecision::Hold
    } else {
        // Tree loads are invalidated by start; active scans are replaced by start_scan.
        LaunchDecision::Start
    }
}

pub fn route_launch(
    state: LaunchState,
    path: String,
    pending: &mut Option<String>,
) -> Option<String> {
    match launch_decision(state) {
        LaunchDecision::Start => Some(path),
        LaunchDecision::Hold => {
            *pending = Some(path);
            None
        }
        LaunchDecision::Ignore => None,
    }
}

#[derive(Default)]
pub struct RequestTag(u64);
impl RequestTag {
    pub fn next(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
    pub fn accepts(&self, tag: u64) -> bool {
        self.0 == tag
    }
}

pub fn coalesce(
    events: impl IntoIterator<Item = BackendEvent>,
    scan: Option<u64>,
) -> (Option<Progress>, Vec<BackendEvent>) {
    let mut progress = None;
    let mut rest = Vec::new();
    for event in events {
        if !accepts_event(&event, scan) {
            continue;
        }
        if let BackendEvent::ScanProgress(e) = event {
            progress = Some(e.data);
        } else {
            rest.push(event);
        }
    }
    (progress, rest)
}

pub fn linked_hover(tree: &crate::layout::Node, hover: Option<usize>) -> Option<usize> {
    let id = hover?;
    tree.children
        .iter()
        .flatten()
        .find(|child| contains(child, id))
        .and_then(|child| child.id)
        .or(Some(id))
}
fn contains(node: &crate::layout::Node, id: usize) -> bool {
    node.id == Some(id) || node.children.iter().flatten().any(|n| contains(n, id))
}
pub fn tile_dim(
    frame: bool,
    category: Option<&str>,
    filter: Option<&str>,
    id: Option<usize>,
    lit: Option<&std::collections::HashSet<Option<usize>>>,
) -> bool {
    (filter.is_some() && !frame && category != filter) || lit.is_some_and(|lit| !lit.contains(&id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sprawl_core::backend::{Cancelled, ScanError, ScanEvent};
    #[test]
    fn launch_decisions() {
        let idle = LaunchState::default();
        // Neither tree loading nor scanning blocks a launch; both use the idle policy.
        for scenario in ["idle", "tree loading", "scanning"] {
            assert_eq!(launch_decision(idle), LaunchDecision::Start, "{scenario}");
        }
        for state in [
            LaunchState {
                opening: true,
                ..idle
            },
            LaunchState {
                prompt_open: true,
                ..idle
            },
        ] {
            assert_eq!(launch_decision(state), LaunchDecision::Hold);
            let mut pending = None;
            assert_eq!(route_launch(state, "first".into(), &mut pending), None);
            assert_eq!(route_launch(state, "latest".into(), &mut pending), None);
            assert_eq!(pending.as_deref(), Some("latest"));
            let path = pending.take().unwrap();
            assert_eq!(
                route_launch(idle, path, &mut pending).as_deref(),
                Some("latest")
            );
            assert!(pending.is_none());
        }
        for state in [
            LaunchState {
                delete_dialog: true,
                ..idle
            },
            LaunchState {
                delete_running: true,
                ..idle
            },
            LaunchState {
                delete_pending: true,
                ..idle
            },
        ] {
            // Delete takes precedence even when an open or prompt has held a launch.
            let state = LaunchState {
                opening: true,
                prompt_open: true,
                ..state
            };
            assert_eq!(launch_decision(state), LaunchDecision::Ignore);
            let mut pending = Some("held".into());
            let path = pending.take().unwrap();
            assert_eq!(route_launch(state, path, &mut pending), None);
            assert!(pending.is_none());
        }
    }

    #[test]
    fn startup_keeps_latest_launch_until_ready() {
        let backend =
            sprawl_core::Backend::new(std::path::PathBuf::new(), Some("initial".into()), |event| {
                assert!(matches!(event, BackendEvent::ScanRequested(path) if path == "ready"))
            });
        backend.request_scan("before refresh".into());
        backend.request_scan("latest".into());
        assert_eq!(backend.take_launch_path().as_deref(), Some("latest"));
        assert_eq!(backend.take_launch_path(), None);
        backend.request_scan("ready".into());
    }
    #[test]
    fn latest_progress_and_current_terminal_only() {
        let progress = |scan_id, files| {
            BackendEvent::ScanProgress(ScanEvent {
                scan_id,
                data: Progress {
                    files,
                    ..Default::default()
                },
            })
        };
        let (p, events) = coalesce(
            [
                progress(2, 10),
                progress(1, 999),
                progress(2, 20),
                BackendEvent::ScanCancelled(ScanEvent {
                    scan_id: 1,
                    data: Cancelled {},
                }),
                BackendEvent::ScanError(ScanEvent {
                    scan_id: 2,
                    data: ScanError {
                        message: "test".into(),
                    },
                }),
            ],
            Some(2),
        );
        assert_eq!(p.unwrap().files, 20);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], BackendEvent::ScanError(_)));
        assert!(coalesce([progress(2, 10)], None).0.is_none());
    }
    #[test]
    fn old_folder_response_is_dropped() {
        let mut tag = RequestTag::default();
        let first = tag.next();
        let second = tag.next();
        assert!(!tag.accepts(first));
        assert!(tag.accepts(second));
        tag.next();
        assert!(!tag.accepts(second));
    }
}

pub fn accepts_event(event: &BackendEvent, scan: Option<u64>) -> bool {
    let id = match event {
        BackendEvent::ScanProgress(e) => Some(e.scan_id),
        BackendEvent::ScanComplete(e) => Some(e.scan_id),
        BackendEvent::ScanCancelled(e) => Some(e.scan_id),
        BackendEvent::ScanError(e) => Some(e.scan_id),
        _ => None,
    };
    id.is_none() || id == scan
}

#[cfg(test)]
mod batch_tests {
    use super::*;
    use sprawl_core::{
        backend::{Completed, ScanEvent},
        scanner::{Kind, ViewNode},
    };
    #[test]
    fn launch_before_old_completion_rechecks_generation() {
        let (_, events) = coalesce(
            [
                BackendEvent::ScanRequested("B".into()),
                BackendEvent::ScanComplete(ScanEvent {
                    scan_id: 1,
                    data: Completed {
                        root: ViewNode {
                            id: Some(0),
                            name: "A".into(),
                            kind: Kind::Dir,
                            size: 1,
                            allocated: 1,
                            modified: 0,
                            files: 1,
                            folders: 0,
                            category: None,
                            children: None,
                        },
                        errors: 0,
                        has_allocated: false,
                        has_modified: false,
                    },
                }),
            ],
            Some(1),
        );
        let mut scan = Some(1);
        let mut terminal_applied = false;
        for event in events {
            if !accepts_event(&event, scan) {
                continue;
            }
            match event {
                BackendEvent::ScanRequested(_) => {
                    scan = None;
                    assert_eq!(scan, None);
                    scan = Some(2);
                }
                _ => terminal_applied = true,
            }
        }
        assert_eq!(scan, Some(2));
        assert!(!terminal_applied);
    }
}
