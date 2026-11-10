use crate::scanner::{Kind, Tree, categories};
use serde::Serialize;
use std::{cmp::Reverse, collections::BinaryHeap, path::PathBuf};

#[derive(Debug, PartialEq)]
enum Term {
    Contains(String),
    Wildcard(Vec<char>),
    Size(Compare, u64),
    Modified(Compare, i64),
    Age(Compare, u64, u64),
    Category(u8),
    Kind(Kind),
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum Compare {
    Greater,
    Less,
    AtLeast,
    AtMost,
}

/// A parsed search: every term must match. Names compare case-insensitively.
#[derive(Debug, PartialEq)]
pub struct Query {
    terms: Vec<Term>,
}

impl std::str::FromStr for Query {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        Self::parse(
            text,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        )
    }
}

impl Query {
    pub fn parse(text: &str, now_ms: u64) -> Result<Self, String> {
        let mut terms = vec![];
        for (token, quoted) in tokens(text)? {
            terms.push(if quoted {
                name_term(&token)
            } else {
                term(&token, now_ms)?
            });
        }
        // Check the cheap numeric terms before comparing names.
        terms.sort_by_key(|term| matches!(term, Term::Contains(_) | Term::Wildcard(_)));
        Ok(Query { terms })
    }
}

fn tokens(text: &str) -> Result<Vec<(String, bool)>, String> {
    let mut tokens = vec![];
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut token = String::new();
            loop {
                match chars.next() {
                    Some('"') => break,
                    Some(c) => token.push(c),
                    None => return Err(crate::i18n::t("errors.searchQuote")),
                }
            }
            if !token.is_empty() {
                tokens.push((token, true));
            }
        } else {
            let mut token = String::new();
            while let Some(&c) = chars.peek()
                && !c.is_whitespace()
                && c != '"'
            {
                token.push(c);
                chars.next();
            }
            tokens.push((token, false));
        }
    }
    Ok(tokens)
}

fn name_term(token: &str) -> Term {
    let token = token.to_lowercase();
    if token.contains(['*', '?']) {
        Term::Wildcard(token.chars().collect())
    } else {
        Term::Contains(token)
    }
}

fn term(token: &str, now_ms: u64) -> Result<Term, String> {
    let lower = token.to_lowercase();
    for prefix in ["modified", "age"] {
        if let Some(rest) = lower.strip_prefix(prefix)
            && rest.starts_with(['<', '>', '='])
        {
            let (compare, value) = comparison(rest).ok_or_else(|| {
                crate::i18n::with_args(
                    "errors.searchFilter",
                    &[("prefix", prefix), ("token", token)],
                )
            })?;
            return if prefix == "modified" {
                crate::dates::parse(value)
                    .map(|ms| match compare {
                        Compare::AtMost => Term::Modified(Compare::Less, ms + 86_400_000),
                        Compare::Greater => Term::Modified(Compare::AtLeast, ms + 86_400_000),
                        _ => Term::Modified(compare, ms),
                    })
                    .ok_or_else(|| crate::i18n::with_args("errors.searchDate", &[("value", value)]))
            } else {
                let duration = value.len().checked_sub(1).and_then(|end| {
                    if !value.is_char_boundary(end) {
                        return None;
                    }
                    let days: u64 = match &value[end..] {
                        "d" => 1,
                        "w" => 7,
                        "m" => 30,
                        "y" => 365,
                        _ => return None,
                    };
                    value[..end]
                        .parse::<u64>()
                        .ok()?
                        .checked_mul(days)?
                        .checked_mul(86_400_000)
                });
                duration
                    .map(|ms| Term::Age(compare, ms, now_ms))
                    .ok_or_else(|| crate::i18n::with_args("errors.searchAge", &[("value", value)]))
            };
        }
    }
    if let Some(id) = lower.strip_prefix("type:") {
        return categories()
            .iter()
            .position(|c| c.id == id)
            .map(|index| Term::Category(index as u8))
            .ok_or_else(|| {
                let ids: Vec<_> = categories().iter().map(|c| c.id.as_str()).collect();
                crate::i18n::with_args(
                    "errors.searchType",
                    &[("id", id), ("types", &ids.join(", "))],
                )
            });
    }
    if let Some(kind) = lower.strip_prefix("kind:") {
        return match kind {
            "file" => Ok(Term::Kind(Kind::File)),
            "dir" => Ok(Term::Kind(Kind::Dir)),
            _ => Err(crate::i18n::with_args(
                "errors.searchKind",
                &[("kind", kind)],
            )),
        };
    }
    if let Some(rest) = lower.strip_prefix("size")
        && rest.starts_with(['<', '>', '='])
    {
        let (compare, value) = if let Some(value) = rest.strip_prefix(">=") {
            (Compare::AtLeast, value)
        } else if let Some(value) = rest.strip_prefix("<=") {
            (Compare::AtMost, value)
        } else if let Some(value) = rest.strip_prefix('>') {
            (Compare::Greater, value)
        } else if let Some(value) = rest.strip_prefix('<') {
            (Compare::Less, value)
        } else {
            return Err(crate::i18n::with_args(
                "errors.searchSizeFilter",
                &[("token", token)],
            ));
        };
        let bytes = parse_size(value).ok_or_else(|| {
            crate::i18n::with_args("errors.searchSize", &[("value", value), ("token", token)])
        })?;
        return Ok(Term::Size(compare, bytes));
    }
    Ok(name_term(token))
}

/// Parses `1.5GB`-style sizes in binary units, matching the app's formatting.
fn parse_size(value: &str) -> Option<u64> {
    let split = value
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number: f64 = number.parse().ok()?;
    let scale = match unit {
        "" | "b" => 1_u64,
        "kb" | "k" => 1 << 10,
        "mb" | "m" => 1 << 20,
        "gb" | "g" => 1 << 30,
        "tb" | "t" => 1 << 40,
        _ => return None,
    };
    let bytes = (number * scale as f64).round();
    (bytes.is_finite() && bytes < u64::MAX as f64).then_some(bytes as u64)
}

fn wildcard(pattern: &[char], name: &str) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star = None;
    loop {
        if pattern.get(p) == Some(&'*') {
            star = Some((p, n));
            p += 1;
            continue;
        }
        let Some(c) = name[n..].chars().next() else {
            return pattern[p..].iter().all(|&c| c == '*');
        };
        if pattern
            .get(p)
            .is_some_and(|&expected| expected == '?' || expected == c)
        {
            p += 1;
            n += c.len_utf8();
        } else if let Some((star_p, star_n)) = star {
            let skipped = name[star_n..].chars().next().map_or(1, char::len_utf8);
            star = Some((star_p, star_n + skipped));
            p = star_p + 1;
            n = star_n + skipped;
        } else {
            return false;
        }
    }
}

impl Query {
    pub(crate) fn matches(&self, node: &crate::scanner::Node, lower: &mut String) -> bool {
        let mut lowered = false;
        self.terms.iter().all(|term| match term {
            Term::Modified(compare, ms) => {
                node.modified != 0 && compare.matches(i128::from(node.modified), i128::from(*ms))
            }
            Term::Age(compare, ms, now) => {
                node.modified != 0
                    && compare.matches(
                        i128::from(*now) - i128::from(node.modified),
                        i128::from(*ms),
                    )
            }
            Term::Kind(kind) => node.kind == *kind,
            Term::Category(index) => node.kind == Kind::File && node.category == Some(*index),
            Term::Size(Compare::Greater, bytes) => node.size > *bytes,
            Term::Size(Compare::Less, bytes) => node.size < *bytes,
            Term::Size(Compare::AtLeast, bytes) => node.size >= *bytes,
            Term::Size(Compare::AtMost, bytes) => node.size <= *bytes,
            Term::Contains(_) | Term::Wildcard(_) => {
                if !lowered {
                    lowered = true;
                    lower.clear();
                    if node.name.is_ascii() {
                        lower.push_str(&node.name);
                        lower.make_ascii_lowercase();
                    } else {
                        lower.push_str(&node.name.to_lowercase());
                    }
                }
                match term {
                    Term::Contains(text) => lower.contains(text.as_str()),
                    Term::Wildcard(pattern) => wildcard(pattern, lower),
                    _ => unreachable!(),
                }
            }
        })
    }
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub id: usize,
    pub name: String,
    pub relative_path: String,
    pub kind: Kind,
    pub size: u64,
    pub allocated: u64,
    pub modified: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct Matches {
    pub total: u64,
    pub results: Vec<Match>,
}

impl Tree {
    /// Finds items under `id`, excluding `id` itself, and returns the `limit` largest.
    pub fn search(&self, id: usize, query: &Query, limit: usize) -> Result<Matches, String> {
        self.node(id)?;
        let mut heap = BinaryHeap::new();
        let mut total = 0;
        let mut lower = String::new();
        let mut stack = self.nodes[id].children.clone();
        while let Some(i) = stack.pop() {
            let node = &self.nodes[i];
            stack.extend(&node.children);
            if !query.matches(node, &mut lower) {
                continue;
            }
            total += 1;
            if limit > 0 {
                heap.push((Reverse(node.size), i));
                if heap.len() > limit {
                    heap.pop();
                }
            }
        }
        let mut results: Vec<_> = heap
            .into_iter()
            .map(|(_, i)| {
                let node = &self.nodes[i];
                Match {
                    id: i,
                    name: node.name.clone(),
                    relative_path: self.relative_path(id, i),
                    kind: node.kind,
                    size: node.size,
                    allocated: node.allocated,
                    modified: node.modified,
                    category: (node.kind == Kind::File)
                        .then_some(node.category)
                        .flatten()
                        .map(|index| categories()[usize::from(index)].id.clone()),
                }
            })
            .collect();
        results.sort_unstable_by(|a, b| {
            b.size
                .cmp(&a.size)
                .then_with(|| a.relative_path.cmp(&b.relative_path))
        });
        Ok(Matches { total, results })
    }

    pub(crate) fn relative_path(&self, base: usize, id: usize) -> String {
        let mut parts = vec![];
        let mut current = id;
        while current != base {
            parts.push(self.nodes[current].name.as_str());
            current = self.nodes[current].parent.unwrap_or(base);
        }
        parts
            .into_iter()
            .rev()
            .collect::<PathBuf>()
            .to_string_lossy()
            .into_owned()
    }
}

fn comparison(rest: &str) -> Option<(Compare, &str)> {
    for (op, compare) in [
        (">=", Compare::AtLeast),
        ("<=", Compare::AtMost),
        (">", Compare::Greater),
        ("<", Compare::Less),
    ] {
        if let Some(value) = rest.strip_prefix(op) {
            return Some((compare, value));
        }
    }
    None
}
impl Compare {
    fn matches(self, a: i128, b: i128) -> bool {
        match self {
            Self::Greater => a > b,
            Self::Less => a < b,
            Self::AtLeast => a >= b,
            Self::AtMost => a <= b,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Query, String> {
        text.parse()
    }

    fn terms(text: &str) -> Vec<Term> {
        parse(text).unwrap().terms
    }

    #[test]
    fn parses_each_token_type() {
        assert_eq!(terms("Report"), [Term::Contains("report".into())]);
        assert_eq!(
            terms("*.ISO report-??.pdf"),
            [
                Term::Wildcard("*.iso".chars().collect()),
                Term::Wildcard("report-??.pdf".chars().collect()),
            ]
        );
        assert_eq!(
            terms("size>1GB size<2kb size>=1.5MB SIZE<=3 size>1t"),
            [
                Term::Size(Compare::Greater, 1 << 30),
                Term::Size(Compare::Less, 2048),
                Term::Size(Compare::AtLeast, 3 << 19),
                Term::Size(Compare::AtMost, 3),
                Term::Size(Compare::Greater, 1 << 40),
            ]
        );
        let video = categories().iter().position(|c| c.id == "video").unwrap() as u8;
        assert_eq!(terms("Type:Video"), [Term::Category(video)]);
        assert_eq!(
            terms("kind:file KIND:dir"),
            [Term::Kind(Kind::File), Term::Kind(Kind::Dir)]
        );
        assert_eq!(terms("  sizeable  "), [Term::Contains("sizeable".into())]);
        assert!(terms("").is_empty());
    }

    #[test]
    fn quotes_group_words_and_stay_names() {
        assert_eq!(
            terms(r#""My Files" "size>1GB"x "*.tmp""#),
            [
                Term::Contains("my files".into()),
                Term::Contains("size>1gb".into()),
                Term::Contains("x".into()),
                Term::Wildcard("*.tmp".chars().collect()),
            ]
        );
        assert!(parse(r#""unclosed"#).unwrap_err().contains("quote"));
    }

    #[test]
    fn rejects_invalid_tokens() {
        for (text, message) in [
            ("size>", "Invalid size"),
            ("size>abc", "Invalid size"),
            ("size>5PB", "Invalid size"),
            ("size>1.2.3", "Invalid size"),
            ("size=5", "Invalid size filter"),
            ("type:movies", "Unknown type \"movies\". Use one of: video,"),
            ("kind:link", "Unknown kind"),
        ] {
            let error = parse(text).unwrap_err();
            assert!(error.contains(message), "{text}: {error}");
        }
    }

    #[test]
    fn wildcards_match_whole_names() {
        let matches =
            |pattern: &str, name: &str| wildcard(&pattern.chars().collect::<Vec<_>>(), name);
        assert!(matches("*.iso", "win11.iso"));
        assert!(!matches("*.iso", "win11.iso.bak"));
        assert!(matches("report-??.pdf", "report-01.pdf"));
        assert!(!matches("report-??.pdf", "report-1.pdf"));
        assert!(matches("*a*b*", "xxaxxbxx"));
        assert!(matches("ü?é*", "üxé"));
        assert!(!matches("?", ""));
        assert!(matches("*", ""));
    }
}

#[cfg(test)]
mod date_tests {
    use super::*;
    #[test]
    fn date_and_age_filters_handle_boundaries_unknown_and_future() {
        let ms = crate::dates::parse("2025-01-01").unwrap() as u64;
        let mut node = crate::scanner::Node {
            name: "test".into(),
            parent: None,
            children: vec![],
            kind: Kind::File,
            size: 1,
            allocated: 1,
            modified: ms,
            files: 1,
            folders: 0,
            category: None,
        };
        for (text, expected) in [
            ("modified>=2025-01-01", true),
            ("modified<=2025-01-01", true),
            ("modified>2025-01-01", false),
            ("modified<2025-01-01", false),
            ("modified>2024-12-31", true),
            ("modified<=2024-12-31", false),
            ("age>=1d", true),
            ("age>1d", false),
            ("age<1w", true),
            ("age<=1m", true),
            ("age>1y", false),
        ] {
            let q = Query::parse(text, ms + 86_400_000).unwrap();
            assert_eq!(q.matches(&node, &mut String::new()), expected, "{text}");
            node.modified = 0;
            assert!(!q.matches(&node, &mut String::new()));
            node.modified = ms;
        }
        node.modified = ms + 86_400_000 - 1;
        assert!(
            Query::parse("modified<=2025-01-01", ms)
                .unwrap()
                .matches(&node, &mut String::new())
        );
        assert!(
            !Query::parse("modified>2025-01-01", ms)
                .unwrap()
                .matches(&node, &mut String::new())
        );
        node.modified += 1;
        assert!(
            !Query::parse("modified<=2025-01-01", ms)
                .unwrap()
                .matches(&node, &mut String::new())
        );
        assert!(
            Query::parse("modified>2025-01-01", ms)
                .unwrap()
                .matches(&node, &mut String::new())
        );
        node.modified = ms;
        assert!(
            Query::parse("age<1d", ms - 1)
                .unwrap()
                .matches(&node, &mut String::new())
        );
        for text in [
            "modified=2025-01-01",
            "modified>2025-02-29",
            "modified>",
            "age=1d",
            "age>1",
            "age>-1d",
            "age>1.5d",
            "age>18446744073709551615y",
            "age>\u{e9}",
        ] {
            assert!(Query::parse(text, ms).is_err(), "{text}");
        }
    }
}
