use serde::{Deserialize, Serialize};
use sprawl_core::scanner::{Kind, ViewNode};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Node {
    pub id: Option<usize>,
    pub name: String,
    pub kind: Kind,
    pub size: u64,
    #[serde(default)]
    pub allocated: u64,
    #[serde(default)]
    pub modified: u64,
    #[serde(default)]
    pub files: u64,
    #[serde(default)]
    pub folders: u64,
    pub category: Option<String>,
    pub children: Option<Vec<Node>>,
}

impl From<ViewNode> for Node {
    fn from(n: ViewNode) -> Self {
        Self {
            id: n.id,
            name: n.name,
            kind: n.kind,
            size: n.size,
            allocated: n.allocated,
            modified: n.modified,
            files: n.files,
            folders: n.folders,
            category: n.category,
            children: n.children.map(|v| v.into_iter().map(Self::from).collect()),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Tile {
    pub node: Node,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub depth: usize,
    pub ancestors: Vec<Option<usize>>,
    pub branch: Option<usize>,
    pub frame: bool,
}

struct Entry<'a> {
    node: &'a Node,
    value: f64,
    children: Vec<Entry<'a>>,
    rect: [f64; 4],
}

fn hierarchy(node: &Node) -> Entry<'_> {
    let mut children: Vec<_> = node.children.iter().flatten().map(hierarchy).collect();
    let value = if children.is_empty() {
        node.size as f64
    } else {
        children.iter().map(|n| n.value).sum()
    };
    children.sort_by(|a, b| b.value.total_cmp(&a.value));
    Entry {
        node,
        value,
        children,
        rect: [0.; 4],
    }
}

fn collapse(a: &mut f64, b: &mut f64) {
    if *b < *a {
        *a = (*a + *b) / 2.;
        *b = *a;
    }
}

// d3-hierarchy's squarify uses the golden ratio, including zero-valued rows.
fn squarify(
    children: &mut [Entry<'_>],
    mut value: f64,
    mut x0: f64,
    mut y0: f64,
    x1: f64,
    y1: f64,
) {
    let ratio = (1. + 5_f64.sqrt()) / 2.;
    let mut i0 = 0;
    let mut i1 = 0;
    while i0 < children.len() {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let mut sum;
        loop {
            sum = children[i1].value;
            i1 += 1;
            if sum != 0. || i1 == children.len() {
                break;
            }
        }
        let mut min = sum;
        let mut max = sum;
        let alpha = js_max(dy / dx, dx / dy) / (value * ratio);
        let beta = sum * sum * alpha;
        let mut best = js_max(max / beta, beta / min);
        while i1 < children.len() {
            let v = children[i1].value;
            sum += v;
            min = min.min(v);
            max = max.max(v);
            let beta = sum * sum * alpha;
            let next = js_max(max / beta, beta / min);
            if next > best {
                sum -= v;
                break;
            }
            best = next;
            i1 += 1;
        }
        if dx < dy {
            let end = if value != 0. {
                y0 + dy * sum / value
            } else {
                y1
            };
            let k = if sum != 0. { dx / sum } else { 0. };
            let mut x = x0;
            for child in &mut children[i0..i1] {
                child.rect = [x, y0, x + child.value * k, end];
                x = child.rect[2];
            }
            y0 = end;
        } else {
            let end = if value != 0. {
                x0 + dx * sum / value
            } else {
                x1
            };
            let k = if sum != 0. { dy / sum } else { 0. };
            let mut y = y0;
            for child in &mut children[i0..i1] {
                child.rect = [x0, y, end, y + child.value * k];
                y = child.rect[3];
            }
            x0 = end;
        }
        value -= sum;
        i0 = i1;
    }
}

fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

fn position(n: &mut Entry<'_>, depth: usize, frames: bool, area: f64, root_size: f64) {
    let p = if frames && depth > 0 { 1. } else { 0. };
    let [mut x0, mut y0, mut x1, mut y1] = n.rect;
    x0 += p;
    y0 += p;
    x1 -= p;
    y1 -= p;
    collapse(&mut x0, &mut x1);
    collapse(&mut y0, &mut y1);
    n.rect = [x0, y0, x1, y1];
    if !n.children.is_empty() {
        let inner = if frames { 1. } else { 0. };
        let outer = if frames && depth > 0 { 3. } else { 0. };
        let top = if frames && depth > 0 && n.value / root_size * area > 3500. {
            23.
        } else {
            outer
        };
        x0 += outer - inner;
        y0 += top - inner;
        x1 -= outer - inner;
        y1 -= outer - inner;
        collapse(&mut x0, &mut x1);
        collapse(&mut y0, &mut y1);
        squarify(&mut n.children, n.value, x0, y0, x1, y1);
        for child in &mut n.children {
            position(child, depth + 1, frames, area, root_size);
        }
    }
}

fn visible(n: &Entry<'_>) -> bool {
    n.rect[2] - n.rect[0] >= 3. && n.rect[3] - n.rect[1] >= 3.
}

pub fn layout_tree(tree: &Node, width: f64, height: f64, frames: bool) -> Vec<Tile> {
    if width <= 0. || height <= 0. || tree.size == 0 {
        return vec![];
    }
    let mut root = hierarchy(tree);
    root.rect = [0., 0., width, height];
    position(&mut root, 0, frames, width * height, tree.size as f64);
    let mut queue = std::collections::VecDeque::from([(&root, Vec::new())]);
    let mut tiles = Vec::new();
    while let Some((n, mut ancestors)) = queue.pop_front() {
        let depth = ancestors.len();
        ancestors.push(n.node.id);
        if depth > 0 && visible(n) {
            let [x, y, x1, y1] = n.rect;
            tiles.push(Tile {
                node: n.node.clone(),
                x,
                y,
                width: x1 - x,
                height: y1 - y,
                depth,
                branch: ancestors.get(1).copied().flatten(),
                ancestors: ancestors.clone(),
                frame: n.children.iter().any(visible),
            });
        }
        for child in &n.children {
            queue.push_back((child, ancestors.clone()));
        }
    }
    tiles
}

pub fn hit(tiles: &[Tile], x: f64, y: f64) -> Option<usize> {
    tiles
        .iter()
        .rposition(|t| x >= t.x && x <= t.x + t.width && y >= t.y && y <= t.y + t.height)
}

pub fn fit_label(name: &str, width: f64, measure: impl Fn(&str) -> f64) -> Option<String> {
    if width <= 0. {
        return None;
    }
    if measure(name) <= width {
        return Some(name.to_owned());
    }
    let mut chars: Vec<_> = name.chars().collect();
    while chars.len() >= 4 {
        chars.pop();
        let label = format!("{}…", chars.iter().collect::<String>());
        if chars.len() >= 4 && measure(&label) <= width {
            return Some(label);
        }
    }
    None
}

pub fn wrap_label(
    name: &str,
    width: f64,
    max_lines: usize,
    measure: impl Fn(&str) -> f64,
) -> Vec<String> {
    let Some(fitted) = fit_label(name, width, &measure) else {
        return vec![];
    };
    if max_lines == 0 {
        return vec![];
    }
    if fitted == name || max_lines == 1 {
        return vec![fitted];
    }
    let mut chars: Vec<_> = name.chars().collect();
    let mut lines = Vec::new();
    while !chars.is_empty() {
        let remainder: String = chars.iter().collect();
        if lines.len() == max_lines - 1 || measure(&remainder) <= width {
            if let Some(last) = fit_label(&remainder, width, &measure) {
                lines.push(last);
            }
            break;
        }
        let mut end = 0;
        let mut separator = 0;
        while end < chars.len() && measure(&chars[..=end].iter().collect::<String>()) <= width {
            if " -_.\\/".contains(chars[end]) {
                separator = end + 1;
            }
            end += 1;
        }
        let end = if separator > 0 { separator } else { end };
        if end == 0 {
            break;
        }
        lines.push(chars.drain(..end).collect());
    }
    lines
}

pub fn text_color(hex: &str) -> &'static str {
    let mut luminance = 0.;
    for (i, weight) in [(1, 0.2126), (3, 0.7152), (5, 0.0722)] {
        let v = u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0) as f64 / 255.;
        luminance += weight
            * if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            };
    }
    if luminance > 0.2 {
        "#152330"
    } else {
        "#ffffff"
    }
}
