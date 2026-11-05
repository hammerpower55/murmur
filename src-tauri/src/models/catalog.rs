//! The static model catalogue (spec 6.2), shipped as catalog.json. It is
//! never fetched from the network.

use serde::{Deserialize, Serialize};

const CATALOG: &str = include_str!("catalog.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Whisper,
    Parakeet,
    Moonshine,
    Sensevoice,
    /// Not a speech model: the language model that fixes misheard words.
    Fixup,
}

impl Family {
    pub fn is_speech(self) -> bool {
        self != Family::Fixup
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFile {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Large files: SHA-256 from Hugging Face's large file storage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Small files kept in git: the git blob SHA-1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha1: Option<String>,
    /// Other places serving the same bytes, tried when `url` fails or crawls.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mirrors: Vec<String>,
    /// A zip archive (how Qualcomm AI Hub ships models): unpacked into the
    /// model folder once its hash matches, its top folder dropped, then
    /// deleted. `<name>.unpacked` marks it done.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unzip: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub family: Family,
    /// 1 to 5, relative to the other models on this device class.
    pub accuracy: u8,
    pub speed: u8,
    /// Language codes, or ["*"] for multilingual Whisper models.
    pub languages: Vec<String>,
    pub languages_label: String,
    pub ram_mb: u32,
    #[serde(default)]
    pub default: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Word fixers: a hybrid model that thinks unless its chat prompt
    /// opens with an empty think block (Qwen3 1.7B). Instruct only models
    /// end their reply at once when given one.
    #[serde(default, skip_serializing)]
    pub thinking: bool,
    /// A build precompiled for the Snapdragon X NPU: listed only on PCs
    /// that have one, used only while "Use the NPU" is on.
    #[serde(default)]
    pub npu: bool,
    /// Word fixers: the whole number gain "Check every word" needs with this
    /// model, when it is not the usual MIN_GAIN_OPEN (small models are less
    /// sure).
    #[serde(default, skip_serializing)]
    pub open_gain: Option<u8>,
    pub files: Vec<ModelFile>,
}

impl CatalogModel {
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

#[derive(Deserialize)]
struct CatalogFile {
    models: Vec<CatalogModel>,
}

pub fn catalog() -> Vec<CatalogModel> {
    match serde_json::from_str::<CatalogFile>(CATALOG) {
        Ok(c) => c.models,
        Err(err) => {
            log::error!("model catalogue is broken: {err}");
            Vec::new()
        }
    }
}

pub fn find(id: &str) -> Option<CatalogModel> {
    catalog().into_iter().find(|m| m.id == id)
}

pub fn default_model() -> Option<CatalogModel> {
    catalog().into_iter().find(|m| m.default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The Models page filters the same way.
    fn supports(m: &CatalogModel, code: &str) -> bool {
        m.languages.iter().any(|l| l == "*" || l == code)
    }

    #[test]
    fn parses() {
        let models = catalog();
        assert!(models.len() >= 7, "{} models", models.len());
    }

    #[test]
    fn exactly_one_default_and_it_is_multilingual_whisper() {
        let defaults: Vec<_> = catalog().into_iter().filter(|m| m.default).collect();
        assert_eq!(defaults.len(), 1);
        assert_eq!(defaults[0].family, Family::Whisper);
        assert!(supports(&defaults[0], "ur"));
    }

    #[test]
    fn ids_are_unique_and_usable_as_folder_names() {
        let mut seen = HashSet::new();
        for m in catalog() {
            assert!(seen.insert(m.id.clone()), "duplicate {}", m.id);
            assert!(
                m.id.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'),
                "{}",
                m.id
            );
        }
    }

    #[test]
    fn every_file_is_pinned_sized_and_hashed() {
        for m in catalog() {
            assert!(!m.files.is_empty(), "{}", m.id);
            for f in &m.files {
                // Hugging Face pinned to a commit, or a Qualcomm AI Hub release,
                // whose path names its version.
                let hub = f.url.starts_with("https://huggingface.co/");
                let ai_hub = f.url.starts_with("https://qaihub-public-assets.s3.us-west-2.amazonaws.com/qai-hub-models/models/")
                    && f.url.contains("/releases/v");
                assert!(hub || ai_hub, "{}", f.url);
                assert!(!f.url.contains("/resolve/main/"), "unpinned {}", f.url);
                assert!(f.url.ends_with(&format!("/{}", f.name)), "{}", f.url);
                assert!(f.size > 0, "{}", f.name);
                let hashes = f.sha256.is_some() as u8 + f.git_sha1.is_some() as u8;
                assert_eq!(hashes, 1, "{} needs exactly one hash", f.name);
                if let Some(h) = &f.sha256 {
                    assert_eq!(h.len(), 64);
                }
                if let Some(h) = &f.git_sha1 {
                    assert_eq!(h.len(), 40);
                }
            }
        }
    }

    #[test]
    fn ratings_are_one_to_five() {
        // The word fixer has no speech ratings.
        for m in catalog().into_iter().filter(|m| m.family.is_speech()) {
            assert!((1..=5).contains(&m.accuracy), "{}", m.id);
            assert!((1..=5).contains(&m.speed), "{}", m.id);
        }
    }

    #[test]
    fn slow_models_say_so() {
        assert_eq!(
            find("whisper-large-v3-q5").and_then(|m| m.note).as_deref(),
            Some("Slow on this device")
        );
    }

    #[test]
    fn language_support() {
        let p = find("parakeet-tdt-0.6b-v2").unwrap();
        assert!(supports(&p, "en"));
        assert!(!supports(&p, "ur"));
        assert!(supports(&find("whisper-small").unwrap(), "ur"));
    }

    #[test]
    fn sizes_add_up() {
        let p = find("parakeet-tdt-0.6b-v2").unwrap();
        assert_eq!(p.size(), 652184014 + 8998286 + 139764 + 9384);
    }
}
