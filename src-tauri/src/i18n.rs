use serde_json::Value;
use std::sync::{
    OnceLock,
    atomic::{AtomicUsize, Ordering},
};

static CATALOGS: OnceLock<[Value; 4]> = OnceLock::new();
static LOCALE: AtomicUsize = AtomicUsize::new(0);

fn index(locale: &str) -> usize {
    match locale {
        "fr" => 1,
        "es" => 2,
        "de" => 3,
        _ => 0,
    }
}

pub fn set_locale(locale: &str) {
    LOCALE.store(index(locale), Ordering::Relaxed);
}

fn translate(locale: usize, key: &str, args: &[(&str, &str)]) -> String {
    let catalogs = CATALOGS.get_or_init(|| {
        [
            include_str!("../../src/locales/en.json"),
            include_str!("../../src/locales/fr.json"),
            include_str!("../../src/locales/es.json"),
            include_str!("../../src/locales/de.json"),
        ]
        .map(|text| serde_json::from_str(text).expect("valid locale catalog"))
    });
    let text = lookup(&catalogs[locale], &catalogs[0], key);
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find('}') else { break };
        let name = &rest[1..end];
        result.push_str(
            args.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| *value)
                .unwrap_or(&rest[..=end]),
        );
        rest = &rest[end + 1..];
    }
    result.push_str(rest);
    result
}

fn lookup<'a>(catalog: &'a Value, english: &'a Value, key: &'a str) -> &'a str {
    catalog
        .get(key)
        .and_then(Value::as_str)
        .or_else(|| english.get(key).and_then(Value::as_str))
        .unwrap_or(key)
}

pub fn t(key: &str) -> String {
    with_args(key, &[])
}

pub fn with_args(key: &str, args: &[(&str, &str)]) -> String {
    translate(LOCALE.load(Ordering::Relaxed), key, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lookup_and_placeholders() {
        assert_eq!(
            translate(index("en"), "errors.deleteCancelled", &[]),
            "Delete cancelled."
        );
        assert_eq!(
            translate(index("fr"), "delete.question", &[("name", "{size}")]),
            "Supprimer {size} ?"
        );
        assert_eq!(
            translate(index("unknown"), "delete.question", &[("name", "file")]),
            "Delete file?"
        );
        assert_eq!(translate(index("de"), "missing.key", &[]), "missing.key");
    }

    #[test]
    fn missing_translation_uses_english() {
        let english = serde_json::json!({ "example": "English fallback" });
        assert_eq!(
            lookup(&serde_json::json!({}), &english, "example"),
            "English fallback"
        );
    }
}
