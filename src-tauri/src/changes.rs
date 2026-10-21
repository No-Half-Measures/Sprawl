use crate::scanner::{Kind, Tree};
use serde::Serialize;
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    path::Path,
};

#[derive(Clone, Copy, Default, Debug, Serialize)]
pub struct Removed {
    pub items: usize,
    pub bytes: u64,
}

pub struct Comparison {
    since: u64,
    previous: Vec<Option<u64>>,
    removed: HashMap<usize, Removed>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Child {
    pub id: usize,
    pub previous_size: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrownFile {
    pub id: usize,
    pub name: String,
    pub relative_path: String,
    pub size: u64,
    pub previous_size: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    pub since: u64,
    pub previous_size: Option<u64>,
    pub children: Vec<Child>,
    pub removed: Removed,
    pub grown: Vec<GrownFile>,
}

impl Comparison {
    pub fn new(current: &Tree, previous: &Tree, since: u64) -> Self {
        let mut result = Self {
            since,
            previous: vec![None; current.nodes.len()],
            removed: HashMap::new(),
        };
        let mut stack = vec![(0, 0)];
        while let Some((id, old_id)) = stack.pop() {
            let node = &current.nodes[id];
            let old = &previous.nodes[old_id];
            result.previous[id] = Some(old.size);
            if node.kind != Kind::Dir {
                continue;
            }
            let mut children: HashMap<_, _> = old
                .children
                .iter()
                .map(|&id| (previous.nodes[id].name.as_str(), id))
                .collect();
            for &child in &node.children {
                let node = &current.nodes[child];
                if let Some(&old_child) = children.get(node.name.as_str())
                    && node.kind == previous.nodes[old_child].kind
                {
                    children.remove(node.name.as_str());
                    result.previous[child] = Some(previous.nodes[old_child].size);
                    if node.kind == Kind::Dir {
                        stack.push((child, old_child));
                    }
                }
            }
            if !children.is_empty() {
                result.removed.insert(
                    id,
                    Removed {
                        items: children.len(),
                        bytes: children.values().map(|&id| previous.nodes[id].size).sum(),
                    },
                );
            }
        }
        result
    }

    pub fn view(&self, tree: &Tree, id: usize) -> Result<Changes, String> {
        let node = tree.node(id)?;
        let base = tree.path(id)?;
        let mut heap = BinaryHeap::new();
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            let node = &tree.nodes[id];
            stack.extend(&node.children);
            let growth = node.size.saturating_sub(self.previous[id].unwrap_or(0));
            if node.kind == Kind::File && growth > 0 {
                heap.push(Reverse((growth, id)));
                if heap.len() > 50 {
                    heap.pop();
                }
            }
        }
        let mut grown = vec![];
        for Reverse((_, id)) in heap {
            let node = &tree.nodes[id];
            grown.push(GrownFile {
                id,
                name: node.name.clone(),
                relative_path: tree
                    .path(id)?
                    .strip_prefix(&base)
                    .unwrap_or(Path::new(&node.name))
                    .to_string_lossy()
                    .into_owned(),
                size: node.size,
                previous_size: self.previous[id],
            });
        }
        grown.sort_unstable_by(|a, b| {
            (b.size - b.previous_size.unwrap_or(0))
                .cmp(&(a.size - a.previous_size.unwrap_or(0)))
                .then_with(|| a.relative_path.cmp(&b.relative_path))
        });
        Ok(Changes {
            since: self.since,
            previous_size: self.previous[id],
            children: node
                .children
                .iter()
                .map(|&id| Child {
                    id,
                    previous_size: self.previous[id],
                })
                .collect(),
            removed: self.removed.get(&id).copied().unwrap_or_default(),
            grown,
        })
    }
}
