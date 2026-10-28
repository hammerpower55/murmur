//! Holds the active speech engine and swaps it without a restart.
//!
//! Switching unloads the old model before loading the new one, so two large
//! models never sit in memory together (spec: never above 3 GB). A dictation
//! that arrives mid switch waits for the new model instead of failing.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use anyhow::{anyhow, bail, Result};

use super::parakeet::ParakeetEngine;
use super::whisper::WhisperEngine;
use super::{EngineOptions, SpeechEngine, Transcript};
use crate::models::catalog::Family;
use crate::models::registry::LoadSpec;

#[derive(Default)]
struct Inner {
    engine: Option<Arc<dyn SpeechEngine>>,
    active: Option<String>,
    loading: Option<String>,
    /// Bumped on every switch so a slow, superseded load is thrown away.
    generation: u64,
}

#[derive(Default)]
pub struct EngineHost {
    inner: Mutex<Inner>,
    changed: Condvar,
}

impl EngineHost {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Model id in use, if one is loaded.
    pub fn active(&self) -> Option<String> {
        self.inner.lock().ok().and_then(|i| i.active.clone())
    }

    pub fn loading(&self) -> Option<String> {
        self.inner.lock().ok().and_then(|i| i.loading.clone())
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
                inner.loading = Some(spec.id.clone());
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
                let result = load(&spec, &opts);
                let outcome = match host.inner.lock() {
                    Ok(mut inner) if inner.generation == generation => {
                        inner.loading = None;
                        let outcome = result.map(|engine| {
                            inner.engine = Some(engine);
                            inner.active = Some(spec.id.clone());
                        });
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

    /// Transcribes with the active engine, waiting for a load in progress.
    pub fn transcribe(
        &self,
        pcm16k_mono: &[f32],
        language: Option<&str>,
    ) -> Result<(String, Transcript)> {
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

fn load(spec: &LoadSpec, opts: &EngineOptions) -> Result<Arc<dyn SpeechEngine>> {
    let mut engine: Box<dyn SpeechEngine> = match spec.family {
        Family::Whisper => Box::new(WhisperEngine::new()),
        Family::Parakeet => Box::new(ParakeetEngine::new()),
    };
    engine.load(&spec.path, opts)?;
    if !engine.is_loaded() {
        bail!("{} engine did not load {}", engine.id(), spec.id);
    }
    Ok(Arc::from(engine))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::Duration;

    fn missing(id: &str, family: Family) -> LoadSpec {
        LoadSpec {
            id: id.into(),
            family,
            path: PathBuf::from("Z:/murmur/does/not/exist"),
        }
    }

    #[test]
    fn empty_host_refuses_to_transcribe() {
        let host = EngineHost::new();
        assert!(host.active().is_none());
        assert!(host.transcribe(&[0.0; 16000], None).is_err());
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
}
