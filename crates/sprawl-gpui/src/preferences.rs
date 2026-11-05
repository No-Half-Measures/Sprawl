use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorBy {
    #[default]
    Type,
    Age,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    pub split: f32,
    pub color_by: ColorBy,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            split: 48.,
            color_by: ColorBy::Type,
        }
    }
}
pub fn clamp_split(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(20., 80.)
    } else {
        48.
    }
}
impl Preferences {
    pub fn parse(text: &str) -> Self {
        let mut prefs: Self = serde_json::from_str(text).unwrap_or_default();
        prefs.split = clamp_split(prefs.split);
        prefs
    }
    pub fn load(path: &Path) -> Self {
        Self::parse(&std::fs::read_to_string(path).unwrap_or_default())
    }
    pub fn save(self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec(&self)?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_and_corrupt_preferences() {
        assert_eq!(clamp_split(-10.), 20.);
        assert_eq!(clamp_split(90.), 80.);
        assert_eq!(clamp_split(f32::NAN), 48.);
        assert_eq!(Preferences::parse("{broken").split, 48.);
        assert_eq!(
            Preferences::parse(r#"{"split":95,"colorBy":"age"}"#).split,
            80.
        );
        assert_eq!(Preferences::parse(r#"{"colorBy":"invalid"}"#).split, 48.);
    }
    #[test]
    fn missing_corrupt_and_round_trip_file() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("preferences-test-{}.json", std::process::id()));
        assert!(!path.exists());
        assert_eq!(Preferences::load(&path).split, 48.);
        std::fs::write(&path, "{corrupt").unwrap();
        assert_eq!(Preferences::load(&path).split, 48.);
        Preferences {
            split: 63.,
            color_by: ColorBy::Age,
        }
        .save(&path)
        .unwrap();
        let loaded = Preferences::load(&path);
        assert_eq!(loaded.split, 63.);
        assert!(loaded.color_by == ColorBy::Age);
        std::fs::remove_file(path).unwrap();
    }
}
