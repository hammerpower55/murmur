//! Ties the catalogue, downloads and the engine host together for the UI.
//!
//! Events: `models-changed` (refetch the list), `model-download` (progress)
//! and `model-error` (one line for the user).

use super::speed;
use crate::db::Db;
use tauri::Manager;
use crate::hardware;
use crate::engine::onnx::npu_available;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::catalog::{self, Family};
use super::download::{self, is_cancelled};
use super::registry::{ModelInfo, Registry};
use crate::engine::fixer::Fixer;
use crate::engine::host::EngineHost;
use crate::engine::whisper::ROMAN_URDU_PROMPT;
use crate::engine::EngineOptions;
use crate::settings;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsView {
    pub models: Vec<ModelInfo>,
    pub active: Option<String>,
    pub loading: Option<String>,
    pub downloads: HashMap<String, Progress>,
    pub roman_urdu: bool,
    /// The word fixer in use, if one is installed.
    pub fixer: Option<String>,
    /// This PC has an NPU that NPU builds can run on.
    pub npu: bool,
    /// The chip speeds are shown for ("Apple M2 Pro").
    pub device: String,
}

#[derive(Clone, Serialize)]
struct DownloadEvent<'a> {
    id: &'a str,
    done: u64,
    total: u64,
}

#[derive(Clone, Serialize)]
struct ErrorEvent<'a> {
    id: &'a str,
    message: &'a str,
}

struct Download {
    cancel: Arc<AtomicBool>,
    progress: Progress,
}

pub struct ModelManager {
    app: AppHandle,
    registry: Registry,
    host: Arc<EngineHost>,
    fixer: Arc<Fixer>,
    /// Writes screenplays; never an NPU build (see dictation.rs).
    writer: Arc<Fixer>,
    downloads: Mutex<HashMap<String, Download>>,
}

/// Only Whisper takes a prompt; it keeps Roman Urdu in Latin script. The
/// English only builds never get it (custom models are assumed multilingual).
pub fn options_for(family: Family, id: &str, roman_urdu: bool) -> EngineOptions {
    let knows_urdu = catalog::find(id)
        .is_none_or(|m| m.languages.iter().any(|l| l == "*" || l == "ur"));
    EngineOptions {
        initial_prompt: (family == Family::Whisper && roman_urdu && knows_urdu)
            .then(|| ROMAN_URDU_PROMPT.to_string()),
        ..EngineOptions::default()
    }
}

impl ModelManager {
    pub fn new(
        app: AppHandle,
        models_dir: PathBuf,
        host: Arc<EngineHost>,
        fixer: Arc<Fixer>,
        writer: Arc<Fixer>,
    ) -> Arc<Self> {
        let registry = Registry::new(models_dir);
        fixer.set_model(registry.fixup(settings::current(&app).use_npu));
        writer.set_model(registry.fixup(false));
        Arc::new(Self {
            app,
            registry,
            host,
            fixer,
            writer,
            downloads: Mutex::new(HashMap::new()),
        })
    }

    pub fn view(&self) -> ModelsView {
        let downloads = self
            .downloads
            .lock()
            .map(|d| d.iter().map(|(id, dl)| (id.clone(), dl.progress)).collect())
            .unwrap_or_default();
        let mut models = self.registry.list();
        let measured = self
            .app
            .try_state::<Arc<Db>>()
            .and_then(|db| db.model_timings().ok())
            .map(|rows| speed::measured(&rows, |id| catalog::find(id).map(|m| m.family)))
            .unwrap_or_default();
        for m in &mut models {
            if let Some(&ms) = measured.get(&m.id) {
                m.clip_ms = ms;
                m.speed = speed::dots(ms);
                m.measured = true;
            }
            m.note = (m.clip_ms > speed::SLOW_MS).then(|| "Slow on this device".to_string());
        }
        ModelsView {
            models,
            active: self.host.active(),
            loading: self.host.loading(),
            downloads,
            roman_urdu: settings::current(&self.app).roman_urdu,
            fixer: self.registry.fixup_id(settings::current(&self.app).use_npu),
            npu: npu_available(),
            device: hardware::device().name.clone(),
        }
    }

    pub fn has_installed_model(&self) -> bool {
        !self.registry.installed().is_empty()
    }

    /// Points the word fixer at the one to use now, after a download, a
    /// delete or a change to "Use the NPU".
    pub fn refresh_fixer(&self) {
        self.fixer
            .set_model(self.registry.fixup(settings::current(&self.app).use_npu));
        self.writer.set_model(self.registry.fixup(false));
    }

    /// Tells the page the models view changed.
    pub fn changed_view(&self) {
        self.changed();
    }

    fn changed(&self) {
        let _ = self.app.emit("models-changed", ());
    }

    fn error(&self, id: &str, message: &str) {
        let _ = self.app.emit("model-error", ErrorEvent { id, message });
    }

    /// Picks the model to load at startup: the saved choice if it is still
    /// installed, else the default, else any installed model.
    pub fn load_at_startup(self: &Arc<Self>) {
        let installed = self.registry.installed();
        let saved = settings::active_model(&self.app);
        let pick = saved
            .filter(|id| installed.iter().any(|m| &m.id == id))
            .or_else(|| {
                catalog::default_model()
                    .map(|d| d.id)
                    .filter(|id| installed.iter().any(|m| &m.id == id))
            })
            .or_else(|| installed.first().map(|m| m.id.clone()));
        match pick {
            Some(id) => {
                if let Err(err) = self.activate(&id) {
                    log::error!("could not load {id} at startup: {err:#}");
                }
            }
            None => log::info!("no model installed yet"),
        }
    }

    pub fn activate(self: &Arc<Self>, id: &str) -> Result<()> {
        let spec = self.registry.resolve(id)?;
        if !spec.family.is_speech() {
            bail!("This model fixes words and cannot transcribe on its own.");
        }
        settings::set_active_model(&self.app, Some(id))?;
        let me = Arc::clone(self);
        let model_id = id.to_string();
        let opts = options_for(spec.family, id, settings::current(&self.app).roman_urdu);
        self.host
            .switch(spec.clone(), opts, move |result| {
                if result.is_err() {
                    // Fresh files will not help an NPU that cannot run the model.
                    let npu = catalog::find(&model_id).is_some_and(|m| m.npu);
                    me.error(
                        &model_id,
                        if npu {
                            "Could not start this model on the NPU. Restart Murmur to try again."
                        } else {
                            "Could not load this model. Delete it and download it again."
                        },
                    );
                }
                me.changed();
            });
        self.changed();
        Ok(())
    }

    /// Reloads the active model so a changed Roman Urdu setting takes effect.
    /// Only Whisper uses it.
    pub fn reload_active(&self) {
        if let (Some(Family::Whisper), Some(id)) = (self.host.active_family(), self.host.active()) {
            let opts = options_for(Family::Whisper, &id, settings::current(&self.app).roman_urdu);
            self.host.reload(opts);
            self.changed();
        }
    }

    /// Installed models for the tray menu: (id, name).
    pub fn installed(&self) -> Vec<(String, String)> {
        self.registry
            .installed()
            .into_iter()
            .map(|m| (m.id, m.name))
            .collect()
    }

    pub fn active(&self) -> Option<String> {
        self.host.active()
    }

    pub fn start_download(self: &Arc<Self>, id: &str) -> Result<()> {
        let model = catalog::find(id).with_context(|| format!("unknown model {id}"))?;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut downloads = self
                .downloads
                .lock()
                .map_err(|_| anyhow!("download list poisoned"))?;
            if downloads.contains_key(id) {
                return Ok(());
            }
            downloads.insert(
                id.to_string(),
                Download {
                    cancel: Arc::clone(&cancel),
                    progress: Progress {
                        done: 0,
                        total: model.size(),
                    },
                },
            );
        }
        self.changed();

        let me = Arc::clone(self);
        let dir = self.registry.model_dir(id);
        std::thread::Builder::new()
            .name("murmur-download".into())
            .spawn(move || {
                let id = model.id.clone();
                let result = download::download_model(&model, &dir, &cancel, |done, total| {
                    if let Ok(mut d) = me.downloads.lock() {
                        if let Some(dl) = d.get_mut(&id) {
                            dl.progress = Progress { done, total };
                        }
                    }
                    let _ = me.app.emit(
                        "model-download",
                        DownloadEvent {
                            id: &id,
                            done,
                            total,
                        },
                    );
                });
                if let Ok(mut d) = me.downloads.lock() {
                    d.remove(&id);
                }
                match result {
                    Ok(()) if !model.family.is_speech() => {
                        log::info!("downloaded {id}");
                        me.refresh_fixer();
                    }
                    Ok(()) => {
                        log::info!("downloaded {id}");
                        // First model on this machine: start using it.
                        if me.host.active().is_none() && me.host.loading().is_none() {
                            if let Err(err) = me.activate(&id) {
                                log::error!("could not load {id}: {err:#}");
                            }
                        }
                    }
                    Err(err) if is_cancelled(&err) => log::info!("download of {id} cancelled"),
                    Err(err) if err.downcast_ref::<download::NoSpace>().is_some() => {
                        let short = err.downcast_ref::<download::NoSpace>().map_or(0, |n| n.short);
                        log::error!("download of {id} failed: {err:#}");
                        me.error(&id, &format!("Free up {} of disk space to download this model.", gigabytes(short)));
                    }
                    Err(err) => {
                        log::error!("download of {id} failed: {err:#}");
                        me.error(&id, "Download failed. Check your connection and try again.");
                    }
                }
                me.changed();
            })
            .context("could not start the download")?;
        Ok(())
    }

    pub fn cancel_download(&self, id: &str) {
        if let Ok(d) = self.downloads.lock() {
            if let Some(dl) = d.get(id) {
                dl.cancel.store(true, Ordering::SeqCst);
            }
        }
    }

    pub fn delete(&self, id: &str) -> Result<u64> {
        if self.host.active().as_deref() == Some(id) || self.host.loading().as_deref() == Some(id) {
            bail!("Switch to another model before deleting this one.");
        }
        if self.downloads.lock().is_ok_and(|d| d.contains_key(id)) {
            bail!("Cancel the download before deleting this model.");
        }
        if catalog::find(id).is_some_and(|m| !m.family.is_speech()) {
            self.fixer.set_model(None);
            self.writer.set_model(None);
        }
        let freed = self.registry.delete(id)?;
        self.refresh_fixer();
        log::info!("deleted {id}, freed {} MB", freed / 1_000_000);
        self.changed();
        Ok(freed)
    }
}

/// Bytes as gigabytes for the user, rounded up to a tenth.
fn gigabytes(bytes: u64) -> String {
    let tenths = bytes.div_ceil(100_000_000).max(1);
    format!("{}.{} GB", tenths / 10, tenths % 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roman_urdu_prompt_only_reaches_whisper_models_that_know_urdu() {
        let prompt = |family, id| options_for(family, id, true).initial_prompt.is_some();
        assert!(prompt(Family::Whisper, "whisper-small"));
        assert!(prompt(Family::Whisper, "my-custom-ggml"));
        assert!(!prompt(Family::Whisper, "whisper-small-en-q8"));
        assert!(!prompt(Family::Moonshine, "moonshine-tiny"));
        assert!(options_for(Family::Whisper, "whisper-small", false).initial_prompt.is_none());
    }

    #[test]
    fn gigabytes_round_up_to_a_tenth() {
        assert_eq!(gigabytes(1), "0.1 GB");
        assert_eq!(gigabytes(1_250_000_000), "1.3 GB");
        assert_eq!(gigabytes(3_000_000_000), "3.0 GB");
    }
}
