use crate::layout::Node;
use sprawl_core::scanner::Kind;
use std::{collections::HashMap, path::PathBuf};

pub fn compare_mounts(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        if a.peek().is_some_and(char::is_ascii_digit) && b.peek().is_some_and(char::is_ascii_digit)
        {
            let left: String = std::iter::from_fn(|| {
                if a.peek().is_some_and(char::is_ascii_digit) {
                    a.next()
                } else {
                    None
                }
            })
            .collect();
            let right: String = std::iter::from_fn(|| {
                if b.peek().is_some_and(char::is_ascii_digit) {
                    b.next()
                } else {
                    None
                }
            })
            .collect();
            let (left, right) = (left.trim_start_matches('0'), right.trim_start_matches('0'));
            let order = left.len().cmp(&right.len()).then_with(|| left.cmp(right));
            if order != Ordering::Equal {
                return order;
            }
        } else {
            match (a.next(), b.next()) {
                (None, None) => return Ordering::Equal,
                (left, right) if left != right => return left.cmp(&right),
                _ => {}
            }
        }
    }
}

pub struct Arguments {
    pub data: Option<PathBuf>,
    pub open: Option<String>,
    pub appearance: Option<gpui::WindowAppearance>,
    pub remaining: Vec<String>,
}
pub fn arguments(args: impl IntoIterator<Item = String>) -> Result<Arguments, String> {
    let mut result = Arguments {
        data: None,
        open: None,
        appearance: None,
        remaining: vec![],
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--appearance" => {
                let value = args.next().ok_or("--appearance requires light or dark")?;
                if cfg!(feature = "dev-flags") {
                    result.appearance = Some(match value.as_str() {
                        "light" => gpui::WindowAppearance::Light,
                        "dark" => gpui::WindowAppearance::Dark,
                        _ => return Err("--appearance requires light or dark".into()),
                    });
                }
            }
            "--data-dir" => {
                let value = args.next().ok_or("--data-dir requires a path")?;
                if cfg!(feature = "dev-flags") {
                    result.data = Some(value.into());
                }
            }
            "--open-saved" => {
                let value = args.next().ok_or("--open-saved requires a path")?;
                if cfg!(feature = "dev-flags") {
                    result.open = Some(value);
                }
            }
            _ => result.remaining.push(arg),
        }
    }
    Ok(result)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Open,
    Reveal,
    Copy,
    Delete,
}
pub fn menu_items(node: &Node) -> Vec<Command> {
    if node.id.is_none() {
        return vec![];
    }
    let mut items = vec![];
    if node.kind == Kind::Dir {
        items.push(Command::Open);
    }
    items.extend([Command::Reveal, Command::Copy]);
    if node.id != Some(0) {
        items.push(Command::Delete);
    }
    items
}
pub fn export_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| !"\\/:*?\"<>|".contains(*c))
        .collect();
    format!(
        "{}.csv",
        if clean.trim().is_empty() {
            "scan"
        } else {
            clean.trim()
        }
    )
}
pub fn export_format(path: &std::path::Path) -> &'static str {
    if path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("json"))
    {
        "json"
    } else {
        "csv"
    }
}
pub fn refresh_trails(
    trail: &[Node],
    history: &[Vec<Node>],
    ids: &[usize],
    nodes: Vec<Option<Node>>,
) -> (Vec<Node>, Vec<Vec<Node>>) {
    let refreshed: HashMap<_, _> = ids.iter().copied().zip(nodes).collect();
    let replace = |n: &Node| {
        n.id.and_then(|id| refreshed.get(&id))
            .and_then(Clone::clone)
    };
    let trail = trail.iter().map_while(replace).collect();
    let history = history
        .iter()
        .filter_map(|path| path.iter().map(replace).collect::<Option<Vec<_>>>())
        .collect();
    (trail, history)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeleteState {
    #[default]
    Choose,
    Permanent,
    Running,
}
#[derive(Clone, Copy, Debug)]
pub enum DeleteInput {
    Cancel,
    Back,
    Permanent,
    Recycle,
}
#[derive(Debug, PartialEq, Eq)]
pub enum DeleteEffect {
    None,
    Close,
    Submit(bool),
}
impl DeleteState {
    pub fn transition(&mut self, input: DeleteInput) -> DeleteEffect {
        use DeleteEffect::*;
        use DeleteInput::*;
        match (*self, input) {
            (Self::Running, _) => None,
            (_, Cancel) => Close,
            (Self::Permanent, Back) => {
                *self = Self::Choose;
                None
            }
            (Self::Choose, Permanent) => {
                *self = Self::Permanent;
                None
            }
            (Self::Permanent, Permanent) => {
                *self = Self::Running;
                Submit(true)
            }
            (Self::Choose, Recycle) => {
                *self = Self::Running;
                Submit(false)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drive_mounts_use_case_insensitive_numeric_order() {
        let mut mounts = ["D:\\", "c:\\", "volume10", "Volume2"];
        mounts.sort_by(|a, b| compare_mounts(a, b));
        assert_eq!(mounts, ["c:\\", "D:\\", "Volume2", "volume10"]);
    }
    #[test]
    fn dev_flags_are_removed_before_launch_path() {
        let a = arguments(
            [
                "--data-dir",
                "data path",
                "--open-saved",
                "saved",
                "--elevated",
                "--elevated-parent=1",
                "D:\\folder",
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            a.data,
            cfg!(feature = "dev-flags").then(|| PathBuf::from("data path"))
        );
        assert_eq!(
            a.open.as_deref(),
            cfg!(feature = "dev-flags").then_some("saved")
        );
        assert_eq!(
            sprawl_core::shell::launch_path(a.remaining).as_deref(),
            Some("D:\\folder")
        );
        let a = arguments(["--data-dir", "data"].map(String::from)).unwrap();
        assert_eq!(sprawl_core::shell::launch_path(a.remaining), None);
        assert!(arguments(["--data-dir".into()]).is_err());
    }
    #[test]
    fn appearance_flag_is_consumed() {
        for (value, appearance) in [
            ("light", gpui::WindowAppearance::Light),
            ("dark", gpui::WindowAppearance::Dark),
        ] {
            let args = arguments(["--appearance", value, "folder"].map(String::from)).unwrap();
            assert_eq!(
                args.appearance,
                cfg!(feature = "dev-flags").then_some(appearance)
            );
            assert_eq!(args.remaining, ["folder"]);
        }
        assert!(arguments(["--appearance".into()]).is_err());
        assert_eq!(
            arguments(["--appearance", "invalid"].map(String::from)).is_err(),
            cfg!(feature = "dev-flags")
        );
    }
    #[test]
    fn export_names_and_formats() {
        assert_eq!(export_name(" \\/:*?\"<>| "), "scan.csv");
        assert_eq!(export_name(" C:\\A & 日本, B "), "CA & 日本, B.csv");
        for (p, expected) in [
            ("a.JSON", "json"),
            ("a.csv", "csv"),
            ("a.json.csv", "csv"),
            ("a", "csv"),
        ] {
            assert_eq!(export_format(p.as_ref()), expected);
        }
    }
    #[test]
    fn delete_transitions_and_running_guards() {
        let mut state = DeleteState::Choose;
        assert_eq!(state.transition(DeleteInput::Permanent), DeleteEffect::None);
        assert_eq!(state, DeleteState::Permanent);
        assert_eq!(state.transition(DeleteInput::Back), DeleteEffect::None);
        assert_eq!(state, DeleteState::Choose);
        assert_eq!(state.transition(DeleteInput::Cancel), DeleteEffect::Close);
        state.transition(DeleteInput::Permanent);
        assert_eq!(
            state.transition(DeleteInput::Permanent),
            DeleteEffect::Submit(true)
        );
        for input in [
            DeleteInput::Cancel,
            DeleteInput::Back,
            DeleteInput::Permanent,
            DeleteInput::Recycle,
        ] {
            assert_eq!(state.transition(input), DeleteEffect::None);
            assert_eq!(state, DeleteState::Running);
        }
        let mut state = DeleteState::Choose;
        assert_eq!(
            state.transition(DeleteInput::Recycle),
            DeleteEffect::Submit(false)
        );
        assert_eq!(state.transition(DeleteInput::Recycle), DeleteEffect::None);
    }
    #[test]
    fn node_menu_permissions() {
        let mut node: Node =
            serde_json::from_str(r#"{"id":null,"name":"x","kind":"other","size":0}"#).unwrap();
        assert!(menu_items(&node).is_empty());
        node.id = Some(0);
        node.kind = Kind::Dir;
        assert_eq!(
            menu_items(&node),
            vec![Command::Open, Command::Reveal, Command::Copy]
        );
        node.id = Some(1);
        assert_eq!(
            menu_items(&node),
            vec![
                Command::Open,
                Command::Reveal,
                Command::Copy,
                Command::Delete
            ]
        );
        node.kind = Kind::File;
        assert_eq!(
            menu_items(&node),
            vec![Command::Reveal, Command::Copy, Command::Delete]
        );
    }
}

// Supplied clock keeps dialog pointer timing testable without a window.
pub struct DeletePointerGuard(std::time::Instant);
impl DeletePointerGuard {
    pub fn new(now: std::time::Instant) -> Self {
        Self(now)
    }
    pub fn restart(&mut self, now: std::time::Instant) {
        self.0 = now;
    }
    pub fn accepts(&self, now: std::time::Instant) -> bool {
        now.saturating_duration_since(self.0) >= std::time::Duration::from_millis(300)
    }
}

#[cfg(test)]
mod pointer_tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn pointer_delay_on_open_and_step_change() {
        let now = Instant::now();
        let mut guard = DeletePointerGuard::new(now);
        assert!(!guard.accepts(now + Duration::from_millis(299)));
        assert!(guard.accepts(now + Duration::from_millis(300)));
        guard.restart(now + Duration::from_millis(400));
        assert!(!guard.accepts(now + Duration::from_millis(699)));
        assert!(guard.accepts(now + Duration::from_millis(700)));
        // Keyboard transitions use DeleteState directly, independently of the guard.
        let mut state = DeleteState::Choose;
        assert_eq!(
            state.transition(DeleteInput::Recycle),
            DeleteEffect::Submit(false)
        );
    }
}
