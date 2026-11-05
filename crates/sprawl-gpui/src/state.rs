use sprawl_core::{BackendEvent, scanner::Progress};

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
        let id = match &event {
            BackendEvent::ScanProgress(e) => Some(e.scan_id),
            BackendEvent::ScanComplete(e) => Some(e.scan_id),
            BackendEvent::ScanCancelled(e) => Some(e.scan_id),
            BackendEvent::ScanError(e) => Some(e.scan_id),
            _ => None,
        };
        if id.is_some() && id != scan {
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
