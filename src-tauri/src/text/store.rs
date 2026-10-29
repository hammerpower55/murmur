//! The user's dictionary, snippets and Roman Urdu table, saved as JSON in
//! %APPDATA%\Murmur and kept in memory for the dictation thread. The Roman
//! Urdu table falls back to the shipped one until the user edits it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::dictionary::DictEntry;
use super::roman_urdu;
use super::snippets::Snippet;

const DICTIONARY: &str = "dictionary.json";
const SNIPPETS: &str = "snippets.json";
const ROMAN_URDU: &str = "roman_urdu.json";

/// One canonical Roman Urdu spelling and the variants rewritten to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UrduRow {
    pub canonical: String,
    pub variants: Vec<String>,
}

pub fn default_urdu_rows() -> Vec<UrduRow> {
    let json = roman_urdu::DEFAULT_TABLE;
    let table: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(json).unwrap_or_default();
    table
        .into_iter()
        .filter(|(k, _)| !k.starts_with('_'))
        .map(|(canonical, variants)| UrduRow {
            canonical,
            variants: variants
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default(),
        })
        .collect()
}

/// Variant (lowercase) to canonical spelling.
pub fn urdu_map(rows: &[UrduRow]) -> HashMap<String, String> {
    rows.iter()
        .flat_map(|r| {
            r.variants
                .iter()
                .map(move |v| (v.trim().to_lowercase(), r.canonical.trim().to_string()))
        })
        .collect()
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

pub fn validate_dictionary(entries: &[DictEntry]) -> Result<()> {
    let mut seen = Vec::new();
    for e in entries {
        if e.spoken.trim().is_empty() || e.written.trim().is_empty() {
            bail!("Fill in both the spoken and the written form.");
        }
        let key = norm(&e.spoken);
        if seen.contains(&key) {
            bail!("\"{}\" is already in your dictionary.", e.spoken.trim());
        }
        seen.push(key);
    }
    Ok(())
}

pub fn validate_snippets(snippets: &[Snippet]) -> Result<()> {
    let mut seen = Vec::new();
    for s in snippets {
        if s.trigger.trim().is_empty() || s.text.trim().is_empty() {
            bail!("Fill in both the trigger and the text.");
        }
        if super::phrase_words(&s.trigger).is_empty() {
            bail!("A trigger needs at least one word.");
        }
        let key = norm(&s.trigger);
        if seen.contains(&key) {
            bail!("\"{}\" is already a snippet trigger.", s.trigger.trim());
        }
        seen.push(key);
    }
    Ok(())
}

pub fn validate_urdu(rows: &[UrduRow]) -> Result<()> {
    let mut owner: HashMap<String, &str> = HashMap::new();
    for r in rows {
        let canonical = r.canonical.trim();
        if canonical.is_empty() || canonical.contains(char::is_whitespace) {
            bail!("Each spelling must be one word.");
        }
        if r.variants.is_empty() {
            bail!("Add at least one variant for \"{canonical}\".");
        }
        for v in &r.variants {
            let v = v.trim().to_lowercase();
            if v.is_empty() || v.contains(char::is_whitespace) {
                bail!("Each variant must be one word.");
            }
            if let Some(other) = owner.insert(v.clone(), canonical) {
                if other != canonical {
                    bail!("\"{v}\" is listed under both \"{other}\" and \"{canonical}\".");
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct UserText {
    pub dictionary: Vec<DictEntry>,
    pub snippets: Vec<Snippet>,
    pub roman_urdu: Vec<UrduRow>,
    pub roman_urdu_map: HashMap<String, String>,
}

pub struct TextStore {
    dir: PathBuf,
    data: RwLock<UserText>,
}

fn read<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(err) => {
            log::warn!("ignoring {}: {err}", path.display());
            None
        }
    }
}

/// Writes through a temporary file so a crash never leaves half a file.
fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(&tmp, json).with_context(|| format!("could not write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("could not save {}", path.display()))
}

impl TextStore {
    pub fn open(dir: PathBuf) -> Self {
        let dictionary = read(&dir.join(DICTIONARY)).unwrap_or_default();
        let snippets = read(&dir.join(SNIPPETS)).unwrap_or_default();
        let roman_urdu: Vec<UrduRow> =
            read(&dir.join(ROMAN_URDU)).unwrap_or_else(default_urdu_rows);
        let roman_urdu_map = urdu_map(&roman_urdu);
        Self {
            dir,
            data: RwLock::new(UserText {
                dictionary,
                snippets,
                roman_urdu,
                roman_urdu_map,
            }),
        }
    }

    pub fn get(&self) -> UserText {
        self.data.read().map(|d| d.clone()).unwrap_or_default()
    }

    fn update(&self, f: impl FnOnce(&mut UserText)) -> Result<()> {
        let mut data = self
            .data
            .write()
            .map_err(|_| anyhow::anyhow!("text store poisoned"))?;
        f(&mut data);
        Ok(())
    }

    pub fn set_dictionary(&self, entries: Vec<DictEntry>) -> Result<()> {
        validate_dictionary(&entries)?;
        write(&self.dir.join(DICTIONARY), &entries)?;
        self.update(|d| d.dictionary = entries)
    }

    /// Adds entries learned from the user's corrections. Returns the ones
    /// that were new.
    pub fn learn(&self, learned: Vec<DictEntry>) -> Result<Vec<DictEntry>> {
        let mut dictionary = self.get().dictionary;
        let added = super::learn::merge(&mut dictionary, learned);
        if !added.is_empty() {
            self.set_dictionary(dictionary)?;
        }
        Ok(added)
    }

    pub fn set_snippets(&self, snippets: Vec<Snippet>) -> Result<()> {
        validate_snippets(&snippets)?;
        write(&self.dir.join(SNIPPETS), &snippets)?;
        self.update(|d| d.snippets = snippets)
    }

    pub fn set_roman_urdu(&self, rows: Vec<UrduRow>) -> Result<()> {
        validate_urdu(&rows)?;
        write(&self.dir.join(ROMAN_URDU), &rows)?;
        let map = urdu_map(&rows);
        self.update(|d| {
            d.roman_urdu = rows;
            d.roman_urdu_map = map;
        })
    }

    /// Goes back to the shipped table.
    pub fn reset_roman_urdu(&self) -> Result<()> {
        let path = self.dir.join(ROMAN_URDU);
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("could not remove {}", path.display()))?;
        }
        let rows = default_urdu_rows();
        let map = urdu_map(&rows);
        self.update(|d| {
            d.roman_urdu = rows;
            d.roman_urdu_map = map;
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("murmur-text-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(s: &str, w: &str) -> DictEntry {
        DictEntry {
            spoken: s.into(),
            written: w.into(),
            learned: false,
        }
    }

    #[test]
    fn default_rows_match_the_shipped_map() {
        let rows = default_urdu_rows();
        assert!(rows.len() >= 50);
        assert_eq!(urdu_map(&rows), roman_urdu::default_map());
        assert!(validate_urdu(&rows).is_ok());
    }

    #[test]
    fn saves_and_reloads() {
        let dir = temp("reload");
        let store = TextStore::open(dir.clone());
        store
            .set_dictionary(vec![entry("clari tree", "ClariTree")])
            .unwrap();
        store
            .set_snippets(vec![Snippet {
                trigger: "my sig".into(),
                text: "Best,\nMaaz".into(),
            }])
            .unwrap();
        store
            .set_roman_urdu(vec![UrduRow {
                canonical: "acha".into(),
                variants: vec!["accha".into()],
            }])
            .unwrap();

        let again = TextStore::open(dir.clone()).get();
        assert_eq!(again.dictionary, vec![entry("clari tree", "ClariTree")]);
        assert_eq!(again.snippets[0].text, "Best,\nMaaz");
        assert_eq!(again.roman_urdu_map.len(), 1);

        store.reset_roman_urdu().unwrap();
        assert!(TextStore::open(dir).get().roman_urdu.len() > 1);
    }

    #[test]
    fn rejects_duplicates_and_blanks() {
        assert!(validate_dictionary(&[entry("a b", "x"), entry("A  B", "y")]).is_err());
        assert!(validate_dictionary(&[entry(" ", "x")]).is_err());
        assert!(validate_snippets(&[Snippet {
            trigger: "...".into(),
            text: "x".into()
        }])
        .is_err());
        assert!(validate_urdu(&[
            UrduRow {
                canonical: "acha".into(),
                variants: vec!["accha".into()]
            },
            UrduRow {
                canonical: "achha".into(),
                variants: vec!["Accha".into()]
            }
        ])
        .is_err());
    }

    #[test]
    fn a_broken_file_falls_back() {
        let dir = temp("broken");
        std::fs::write(dir.join(DICTIONARY), "not json").unwrap();
        std::fs::write(dir.join(ROMAN_URDU), "[]").unwrap();
        let data = TextStore::open(dir).get();
        assert!(data.dictionary.is_empty());
        // An empty but valid table is the user's choice and is kept.
        assert!(data.roman_urdu.is_empty());
    }
}
