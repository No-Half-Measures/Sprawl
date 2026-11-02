use crate::scanner::{Kind, Tree, categories};
use serde::Serialize;
use std::{cmp::Reverse, collections::BinaryHeap, path::PathBuf};

#[derive(Debug, PartialEq)]
enum Term {
    Contains(String),
    Wildcard(Vec<char>),
    Size(Compare, u64),
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
        let mut terms = vec![];
        for (token, quoted) in tokens(text)? {
            terms.push(if quoted {
                name_term(&token)
            } else {
                term(&token)?
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
                    None => return Err("Close the quote at the end of the search.".into()),
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

fn term(token: &str) -> Result<Term, String> {
    let lower = token.to_lowercase();
    if let Some(id) = lower.strip_prefix("type:") {
        return categories()
            .iter()
            .position(|c| c.id == id)
            .map(|index| Term::Category(index as u8))
            .ok_or_else(|| {
                let ids: Vec<_> = categories().iter().map(|c| c.id.as_str()).collect();
                format!("Unknown type \"{id}\". Use one of: {}.", ids.join(", "))
            });
    }
    if let Some(kind) = lower.strip_prefix("kind:") {
        return match kind {
            "file" => Ok(Term::Kind(Kind::File)),
            "dir" => Ok(Term::Kind(Kind::Dir)),
            _ => Err(format!(
                "Unknown kind \"{kind}\". Use kind:file or kind:dir."
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
            return Err(format!(
                "Invalid size filter \"{token}\". Use size>, size<, size>= or size<=."
            ));
        };
        let bytes = parse_size(value).ok_or_else(|| {
            format!("Invalid size \"{value}\" in \"{token}\". Use a number with KB, MB, GB or TB.")
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
    fn matches(&self, node: &crate::scanner::Node, lower: &mut String) -> bool {
        let mut lowered = false;
        self.terms.iter().all(|term| match term {
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

    fn relative_path(&self, base: usize, id: usize) -> String {
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
