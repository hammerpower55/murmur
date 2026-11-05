//! Holds the active speech engine and swaps it without a restart.
//!
//! Switching unloads the old model before loading the new one, so two large
//! models never sit in memory together (spec: never above 3 GB). A dictation
//! that arrives mid switch waits for the new model instead of failing.
//!
//! After a quiet spell the model can be parked: unloaded but still the
//! active choice. The next hotkey press wakes it, so loading overlaps with
//! the user speaking.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use anyhow::{anyhow, bail, Result};

use super::whisper_npu;
use super::moonshine::MoonshineEngine;
use super::parakeet::ParakeetEngine;
use super::sensevoice::SenseVoiceEngine;
use super::whisper::WhisperEngine;
use super::{EngineOptions, SpeechEngine, Transcript};
use crate::models::catalog::Family;
use crate::models::registry::LoadSpec;

type Loader = fn(&LoadSpec, &EngineOptions) -> Result<Arc<dyn SpeechEngine>>;

#[derive(Default)]
struct Inner {
    engine: Option<Arc<dyn SpeechEngine>>,
    active: Option<String>,
    loading: Option<String>,
    /// What the active model was loaded with, for waking and reloading.
    current: Option<(LoadSpec, EngineOptions)>,
    /// Unloaded after being idle; `current` says what to bring back.
    parked: bool,
    /// Bumped on every switch so a slow, superseded load is thrown away.
    generation: u64,
}

pub struct EngineHost {
    inner: Mutex<Inner>,
    changed: Condvar,
    loader: Loader,
}

impl EngineHost {
    pub fn new() -> Arc<Self> {
        Self::with_loader(load)
    }

    fn with_loader(loader: Loader) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner::default()),
            changed: Condvar::new(),
            loader,
        })
    }

    /// Model id in use, if one is loaded or parked.
    pub fn active(&self) -> Option<String> {
        self.inner.lock().ok().and_then(|i| {
            if i.parked {
                i.current.as_ref().map(|(spec, _)| spec.id.clone())
            } else {
                i.active.clone()
            }
        })
    }

    pub fn active_family(&self) -> Option<Family> {
        let inner = self.inner.lock().ok()?;
        inner.current.as_ref().map(|(spec, _)| spec.family)
    }

    pub fn loading(&self) -> Option<String> {
        self.inner.lock().ok().and_then(|i| i.loading.clone())
    }

    /// True while a model sits in memory.
    pub fn is_loaded(&self) -> bool {
        self.inner.lock().is_ok_and(|i| i.engine.is_some())
    }

    /// Loads `spec` on a background thread, replacing whatever was active.
    /// `done` runs on that thread with the outcome.
    pub fn switch(
        self: &Arc<Self>,
        spec: LoadSpec,
        opts: EngineOptions,
        done: impl FnOnce(Result<()>) + Send + 'static,
    ) {
        let generation = match self.inner.lock() {
            Ok(mut inner) => {
                inner.generation += 1;
                // Drop the old model now so its memory is free for the new one.
                inner.engine = None;
                inner.active = None;
                inner.parked = false;
                inner.loading = Some(spec.id.clone());
                inner.current = Some((spec.clone(), opts.clone()));
                inner.generation
            }
            Err(_) => {
                done(Err(anyhow!("engine host poisoned")));
                return;
            }
        };

        let host = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("murmur-model-load".into())
            .spawn(move || {
                let started = Instant::now();
                let result = (host.loader)(&spec, &opts);
                let outcome = match host.inner.lock() {
                    Ok(mut inner) if inner.generation == generation => {
                        inner.loading = None;
                        let outcome = result.map(|engine| {
                            inner.engine = Some(engine);
                            inner.active = Some(spec.id.clone());
                        });
                        if outcome.is_err() {
                            inner.current = None;
                        }
                        host.changed.notify_all();
                        outcome
                    }
                    // A newer switch took over; it owns the state now.
                    Ok(_) => Ok(()),
                    Err(_) => Err(anyhow!("engine host poisoned")),
                };
                match &outcome {
                    Ok(()) => log::info!(
                        "model ready in {} ms: {} ({:?}, {} threads)",
                        started.elapsed().as_millis(),
                        spec.id,
                        spec.family,
                        opts.n_threads
                    ),
                    Err(err) => log::error!("could not load {}: {err:#}", spec.id),
                }
                done(outcome);
            });
        if let Err(err) = spawned {
            log::error!("could not start model loading: {err}");
        }
    }

    /// Reloads the active model with new options (the Roman Urdu prompt).
    pub fn reload(self: &Arc<Self>, opts: EngineOptions) {
        let spec = match self.inner.lock() {
            Ok(inner) if inner.loading.is_none() => inner.current.as_ref().map(|(s, _)| s.clone()),
            _ => None,
        };
        if let Some(spec) = spec {
            self.switch(spec, opts, |_| {});
        }
    }

    /// Unloads the model to free memory, keeping it as the active choice.
    /// Returns true if something was unloaded.
    pub fn park(&self) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        if inner.engine.is_none() || inner.loading.is_some() {
            return false;
        }
        inner.engine = None;
        inner.active = None;
        inner.parked = true;
        if let Some((spec, _)) = &inner.current {
            log::info!("unloaded {} after being idle", spec.id);
        }
        true
    }

    /// Starts loading a parked model again. Does nothing otherwise.
    pub fn wake(self: &Arc<Self>) {
        let current = match self.inner.lock() {
            Ok(inner) if inner.parked => inner.current.clone(),
            _ => None,
        };
        if let Some((spec, opts)) = current {
            log::info!("loading {} again", spec.id);
            self.switch(spec, opts, |_| {});
        }
    }

    /// Transcribes with the active engine, waiting for a load in progress
    /// and bringing back a parked model.
    pub fn transcribe(
        self: &Arc<Self>,
        pcm16k_mono: &[f32],
        language: Option<&str>,
    ) -> Result<(String, Transcript)> {
        self.wake();
        let (engine, id) = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| anyhow!("engine host poisoned"))?;
            if inner.loading.is_some() {
                log::info!("waiting for the model to finish loading");
            }
            while inner.loading.is_some() {
                inner = self
                    .changed
                    .wait(inner)
                    .map_err(|_| anyhow!("engine host poisoned"))?;
            }
            match (&inner.engine, &inner.active) {
                (Some(e), Some(id)) => (Arc::clone(e), id.clone()),
                _ => bail!("no model loaded"),
            }
        };
        let transcript = engine.transcribe(pcm16k_mono, language)?;
        Ok((id, transcript))
    }
}

/// The engine that runs `spec`'s model, not loaded yet.
pub(crate) fn engine_for(spec: &LoadSpec) -> Result<Box<dyn SpeechEngine>> {
    Ok(match spec.family {
        Family::Whisper if spec.path.join(whisper_npu::ENCODER).is_file() => {
            Box::new(whisper_npu::WhisperNpuEngine::new())
        }
        Family::Whisper => Box::new(WhisperEngine::new()),
        Family::Parakeet => Box::new(ParakeetEngine::new()),
        Family::Moonshine => Box::new(MoonshineEngine::new()),
        Family::Sensevoice => Box::new(SenseVoiceEngine::new()),
        Family::Fixup => anyhow::bail!("{} is not a speech model", spec.id),
    })
}

fn load(spec: &LoadSpec, opts: &EngineOptions) -> Result<Arc<dyn SpeechEngine>> {
    let mut engine = engine_for(spec)?;
    engine.load(&spec.path, opts)?;
    if !engine.is_loaded() {
        bail!("{} engine did not load {}", engine.id(), spec.id);
    }
    Ok(Arc::from(engine))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::time::Duration;

    fn missing(id: &str, family: Family) -> LoadSpec {
        LoadSpec {
            id: id.into(),
            family,
            path: PathBuf::from("Z:/murmur/does/not/exist"),
        }
    }

    /// Echoes the prompt it was loaded with.
    struct Fake(String);

    impl SpeechEngine for Fake {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn load(&mut self, _: &Path, _: &EngineOptions) -> Result<()> {
            Ok(())
        }
        fn unload(&mut self) {}
        fn is_loaded(&self) -> bool {
            true
        }
        fn transcribe(&self, _: &[f32], _: Option<&str>) -> Result<Transcript> {
            Ok(Transcript {
                text: self.0.clone(),
                language: None,
                segments: Vec::new(),
                inference_ms: 0,
            })
        }
    }

    fn fake(_: &LoadSpec, opts: &EngineOptions) -> Result<Arc<dyn SpeechEngine>> {
        Ok(Arc::new(Fake(opts.initial_prompt.clone().unwrap_or_default())))
    }

    fn loaded(host: &Arc<EngineHost>, prompt: &str) {
        let (tx, rx) = mpsc::channel();
        host.switch(
            missing("whisper-small", Family::Whisper),
            EngineOptions {
                initial_prompt: Some(prompt.into()),
                ..EngineOptions::default()
            },
            move |r| {
                let _ = tx.send(r.is_ok());
            },
        );
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(true));
    }

    #[test]
    fn empty_host_refuses_to_transcribe() {
        let host = EngineHost::new();
        assert!(host.active().is_none());
        assert!(host.transcribe(&[0.0; 16000], None).is_err());
        assert!(!host.park());
    }

    #[test]
    fn failed_load_leaves_nothing_active_and_reports_the_error() {
        let host = EngineHost::new();
        let (tx, rx) = mpsc::channel();
        host.switch(
            missing("whisper-small", Family::Whisper),
            EngineOptions::default(),
            move |r| {
                let _ = tx.send(r.is_err());
            },
        );
        assert_eq!(rx.recv_timeout(Duration::from_secs(30)), Ok(true));
        assert!(host.active().is_none());
        assert!(host.loading().is_none());
        assert!(host.transcribe(&[0.0; 16000], None).is_err());
    }

    #[test]
    fn parked_model_stays_active_and_comes_back_to_transcribe() {
        let host = EngineHost::with_loader(fake);
        loaded(&host, "one");
        assert!(host.park());
        assert!(!host.is_loaded());
        assert_eq!(host.active().as_deref(), Some("whisper-small"));
        assert_eq!(host.active_family(), Some(Family::Whisper));
        let (id, t) = host.transcribe(&[0.0; 16000], None).unwrap();
        assert_eq!((id.as_str(), t.text.as_str()), ("whisper-small", "one"));
        assert!(host.is_loaded());
    }

    #[test]
    fn reload_applies_new_options() {
        let host = EngineHost::with_loader(fake);
        loaded(&host, "one");
        host.reload(EngineOptions {
            initial_prompt: Some("two".into()),
            ..EngineOptions::default()
        });
        let (_, t) = host.transcribe(&[0.0; 16000], None).unwrap();
        assert_eq!(t.text, "two");
    }
}
