//! What is installed under %APPDATA%\Murmur\models. Catalogue models live
//! in models\<id>\; any ggml-*.bin dropped into models\custom\ shows up as a
//! Whisper model named after the file.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use super::catalog::{self, CatalogModel, Family};
use super::download::file_complete;

pub const CUSTOM_DIR: &str = "custom";
const CUSTOM_PREFIX: &str = "custom:";

/// One row on the Models page.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub family: Family,
    /// 0 for custom models: unknown.
    pub accuracy: u8,
    pub speed: u8,
    pub languages: Vec<String>,
    pub languages_label: String,
    pub ram_mb: u32,
    pub size_bytes: u64,
    pub note: Option<String>,
    pub default: bool,
    pub custom: bool,
    pub installed: bool,
}

impl ModelInfo {
    fn from_catalog(m: &CatalogModel, installed: bool) -> Self {
        Self {
            id: m.id.clone(),
            name: m.name.clone(),
            family: m.family,
            accuracy: m.accuracy,
            speed: m.speed,
            languages: m.languages.clone(),
            languages_label: m.languages_label.clone(),
            ram_mb: m.ram_mb,
            size_bytes: m.size(),
            note: m.note.clone(),
            default: m.default,
            custom: false,
            installed,
        }
    }
}

/// What an engine needs to load a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadSpec {
    pub id: String,
    pub family: Family,
    /// Whisper: the .bin file. Parakeet: the model folder.
    pub path: PathBuf,
}

pub struct Registry {
    models_dir: PathBuf,
}

impl Registry {
    pub fn new(models_dir: PathBuf) -> Self {
        Self { models_dir }
    }

    pub fn model_dir(&self, id: &str) -> PathBuf {
        self.models_dir.join(id)
    }

    pub fn is_installed(&self, model: &CatalogModel) -> bool {
        let dir = self.model_dir(&model.id);
        model
            .files
            .iter()
            .all(|f| file_complete(&dir.join(&f.name), f))
    }

    /// Catalogue models in catalogue order, then custom models by name.
    pub fn list(&self) -> Vec<ModelInfo> {
        let mut out: Vec<ModelInfo> = catalog::catalog()
            .iter()
            .map(|m| ModelInfo::from_catalog(m, self.is_installed(m)))
            .collect();
        out.extend(self.custom_models());
        out
    }

    pub fn installed(&self) -> Vec<ModelInfo> {
        self.list().into_iter().filter(|m| m.installed).collect()
    }

    fn custom_models(&self) -> Vec<ModelInfo> {
        let Ok(entries) = fs::read_dir(self.models_dir.join(CUSTOM_DIR)) else {
            return Vec::new();
        };
        let mut models: Vec<ModelInfo> = entries
            .flatten()
            .filter_map(|e| {
                let file = e.file_name().to_string_lossy().to_string();
                let lower = file.to_lowercase();
                if !lower.starts_with("ggml-") || !lower.ends_with(".bin") {
                    return None;
                }
                let size = e.metadata().ok()?.len();
                Some(ModelInfo {
                    id: format!("{CUSTOM_PREFIX}{file}"),
                    name: file.trim_end_matches(".bin").to_string(),
                    family: Family::Whisper,
                    accuracy: 0,
                    speed: 0,
                    languages: vec!["*".into()],
                    languages_label: "Custom Whisper model".into(),
                    ram_mb: 0,
                    size_bytes: size,
                    note: None,
                    default: false,
                    custom: true,
                    installed: true,
                })
            })
            .collect();
        models.sort_by_key(|m| m.name.to_lowercase());
        models
    }

    pub fn resolve(&self, id: &str) -> Result<LoadSpec> {
        if let Some(file) = id.strip_prefix(CUSTOM_PREFIX) {
            let path = self.models_dir.join(CUSTOM_DIR).join(file);
            if !path.is_file() {
                bail!("{file} is no longer in the custom models folder");
            }
            return Ok(LoadSpec {
                id: id.into(),
                family: Family::Whisper,
                path,
            });
        }
        let model = catalog::find(id).with_context(|| format!("unknown model {id}"))?;
        if !self.is_installed(&model) {
            bail!("{} is not downloaded", model.name);
        }
        let dir = self.model_dir(id);
        let path = match model.family {
            Family::Whisper => dir.join(&model.files[0].name),
            // ONNX engines take the folder and find their files in it.
            Family::Parakeet | Family::Moonshine | Family::Sensevoice => dir,
        };
        Ok(LoadSpec {
            id: id.into(),
            family: model.family,
            path,
        })
    }

    /// Removes a model's files, including partial downloads. Returns the
    /// bytes freed.
    pub fn delete(&self, id: &str) -> Result<u64> {
        let path = match id.strip_prefix(CUSTOM_PREFIX) {
            Some(file) => self.models_dir.join(CUSTOM_DIR).join(file),
            None => {
                catalog::find(id).with_context(|| format!("unknown model {id}"))?;
                self.model_dir(id)
            }
        };
        let freed = disk_usage(&path);
        if path.is_dir() {
            fs::remove_dir_all(&path)
        } else if path.is_file() {
            fs::remove_file(&path)
        } else {
            return Ok(0);
        }
        .with_context(|| format!("could not delete {}", path.display()))?;
        Ok(freed)
    }
}

fn disk_usage(path: &Path) -> u64 {
    if path.is_file() {
        return fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    fs::read_dir(path)
        .map(|entries| entries.flatten().map(|e| disk_usage(&e.path())).sum())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_models(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("murmur-reg-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes files of the right size (content is irrelevant to the check).
    fn fake_install(dir: &Path, id: &str) {
        let m = catalog::find(id).unwrap();
        let d = dir.join(id);
        fs::create_dir_all(&d).unwrap();
        for f in &m.files {
            let file = fs::File::create(d.join(&f.name)).unwrap();
            file.set_len(f.size).unwrap();
        }
    }

    #[test]
    fn nothing_installed_in_an_empty_folder() {
        let r = Registry::new(temp_models("empty"));
        assert!(r.installed().is_empty());
        assert_eq!(r.list().len(), catalog::catalog().len());
    }

    #[test]
    fn complete_model_is_installed_partial_is_not() {
        let dir = temp_models("partial");
        fake_install(&dir, "parakeet-tdt-0.6b-v2");
        let r = Registry::new(dir.clone());
        assert_eq!(r.installed().len(), 1);
        // Truncate one file: no longer installed.
        let f = fs::OpenOptions::new()
            .write(true)
            .open(dir.join("parakeet-tdt-0.6b-v2").join("vocab.txt"))
            .unwrap();
        f.set_len(10).unwrap();
        assert!(r.installed().is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_gives_engines_the_right_path() {
        let dir = temp_models("resolve");
        fake_install(&dir, "parakeet-tdt-0.6b-v2");
        fake_install(&dir, "whisper-small");
        let r = Registry::new(dir.clone());
        let p = r.resolve("parakeet-tdt-0.6b-v2").unwrap();
        assert_eq!(p.family, Family::Parakeet);
        assert_eq!(p.path, dir.join("parakeet-tdt-0.6b-v2"));
        let w = r.resolve("whisper-small").unwrap();
        assert_eq!(w.path, dir.join("whisper-small").join("ggml-small.bin"));
        assert!(r.resolve("whisper-medium-q8").is_err());
        assert!(r.resolve("nope").is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn custom_ggml_files_appear_as_whisper_models() {
        let dir = temp_models("custom");
        fs::create_dir_all(dir.join(CUSTOM_DIR)).unwrap();
        fs::write(dir.join(CUSTOM_DIR).join("ggml-tiny.en.bin"), b"x").unwrap();
        fs::write(dir.join(CUSTOM_DIR).join("notes.txt"), b"x").unwrap();
        fs::write(dir.join(CUSTOM_DIR).join("model.bin"), b"x").unwrap();
        let r = Registry::new(dir.clone());
        let custom: Vec<_> = r.list().into_iter().filter(|m| m.custom).collect();
        assert_eq!(custom.len(), 1);
        assert_eq!(custom[0].id, "custom:ggml-tiny.en.bin");
        assert_eq!(custom[0].name, "ggml-tiny.en");
        assert!(custom[0].installed);
        let spec = r.resolve("custom:ggml-tiny.en.bin").unwrap();
        assert_eq!(spec.family, Family::Whisper);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_frees_the_files() {
        let dir = temp_models("delete");
        fake_install(&dir, "parakeet-tdt-0.6b-v2");
        fs::write(dir.join("parakeet-tdt-0.6b-v2").join("x.part"), b"12345").unwrap();
        let r = Registry::new(dir.clone());
        let expected = catalog::find("parakeet-tdt-0.6b-v2").unwrap().size() + 5;
        assert_eq!(r.delete("parakeet-tdt-0.6b-v2").unwrap(), expected);
        assert!(!dir.join("parakeet-tdt-0.6b-v2").exists());
        assert_eq!(r.delete("parakeet-tdt-0.6b-v2").unwrap(), 0);
        fs::remove_dir_all(&dir).ok();
    }
}
