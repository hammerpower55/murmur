//! Whisper GGML models through whisper.cpp (whisper-rs), CPU with NEON.
//!
//! The encoder always runs its full 30 s window. Shrinking it with audio_ctx
//! is about 3x faster on short clips, but on real recordings it garbled Roman
//! Urdu and made the decoder repeat whole sentences, even at a 20 s window.
//! Do not reintroduce it without rerunning the fixture WER check.

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

use super::{EngineOptions, Segment, SpeechEngine, Transcript};
use crate::audio::TARGET_RATE;

/// Primes the decoder so Urdu comes out in Latin script. Tune here, and
/// rerun `cargo test roman_urdu_strategies -- --ignored --nocapture`.
///
/// It must be Roman Urdu throughout. With English sentences in it (the
/// spec's original prompt) Whisper took the English language token as a cue
/// to translate: on the mixed fixture clips it translated or dropped every
/// Urdu heavy sentence, 71% WER. This prompt brings that to 18%, and the
/// remaining errors are spelling variants the Roman Urdu table cleans up.
pub const ROMAN_URDU_PROMPT: &str = "Assalam o alaikum. Aaj ka din bohat busy tha, main abhi ghar pohncha hoon. \
Acha theek hai, mujhe lagta hai ke hum kal tak yeh kaam khatam kar lenge. \
Tum ne meri email dekhi? Bhai please jaldi bhej do, client wait kar raha hai. \
Koi baat nahi, main shaam ko call karta hoon aur sab samjha deta hoon.";

/// whisper.cpp returns nothing for clips under one second, so short clips
/// are padded with silence up to this length.
const MIN_SAMPLES: usize = TARGET_RATE as usize + TARGET_RATE as usize / 10;

struct Loaded {
    // The state holds its own reference to the context; both are kept so the
    // model stays alive for as long as the engine is loaded.
    _context: WhisperContext,
    state: Mutex<WhisperState>,
    opts: EngineOptions,
}

#[derive(Default)]
pub struct WhisperEngine {
    loaded: Option<Loaded>,
}

impl WhisperEngine {
    pub fn new() -> Self {
        // Route whisper.cpp and ggml output into the log crate.
        whisper_rs::install_logging_hooks();
        Self::default()
    }
}

impl SpeechEngine for WhisperEngine {
    fn id(&self) -> &'static str {
        "whisper"
    }

    fn load(&mut self, model_path: &Path, opts: &EngineOptions) -> Result<()> {
        self.unload();
        let path = model_path
            .to_str()
            .ok_or_else(|| anyhow!("model path is not valid UTF-8"))?;
        let mut params = WhisperContextParameters::default();
        params.use_gpu(false);
        let context = WhisperContext::new_with_params(path, params)
            .with_context(|| format!("could not load {}", model_path.display()))?;
        let state = context
            .create_state()
            .context("could not allocate whisper state")?;
        self.loaded = Some(Loaded {
            _context: context,
            state: Mutex::new(state),
            opts: opts.clone(),
        });
        Ok(())
    }

    fn unload(&mut self) {
        self.loaded = None;
    }

    fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }

    fn transcribe(&self, pcm16k_mono: &[f32], language: Option<&str>) -> Result<Transcript> {
        let loaded = self.loaded.as_ref().context("no model loaded")?;
        let mut state = loaded
            .state
            .lock()
            .map_err(|_| anyhow!("whisper state poisoned"))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(loaded.opts.n_threads as i32);
        params.set_language(Some(language.unwrap_or("auto")));
        if let Some(prompt) = &loaded.opts.initial_prompt {
            params.set_initial_prompt(prompt);
        }
        // Each dictation stands alone.
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        let padded;
        let audio = if pcm16k_mono.len() < MIN_SAMPLES {
            padded = [pcm16k_mono, &vec![0.0; MIN_SAMPLES - pcm16k_mono.len()]].concat();
            &padded[..]
        } else {
            pcm16k_mono
        };

        let started = Instant::now();
        state.full(params, audio).context("transcription failed")?;
        let inference_ms = started.elapsed().as_millis() as u64;

        let mut segments = Vec::new();
        for segment in state.as_iter() {
            let text = segment
                .to_str_lossy()
                .context("could not read segment text")?
                .trim()
                .to_string();
            if text.is_empty() {
                continue;
            }
            // whisper.cpp timestamps are in centiseconds.
            segments.push(Segment {
                start_ms: segment.start_timestamp().max(0) as u64 * 10,
                end_ms: segment.end_timestamp().max(0) as u64 * 10,
                text,
            });
        }
        let mut text = segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if let Some(prompt) = &loaded.opts.initial_prompt {
            if is_prompt_echo(&text, prompt) {
                log::info!("dropped output that repeats the initial prompt");
                text.clear();
                segments.clear();
            }
        }
        let language = whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(String::from);

        Ok(Transcript {
            text,
            language,
            segments,
            inference_ms,
        })
    }
}

/// On silence or speech it cannot follow, Whisper sometimes returns part of
/// the initial prompt instead ("Okay, let me check and get back to you.").
/// True when the output is a run of three or more words copied from the
/// prompt; shorter matches ("okay") can be real speech.
fn is_prompt_echo(text: &str, prompt: &str) -> bool {
    fn words(s: &str) -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|w| !w.is_empty())
            .map(String::from)
            .collect()
    }
    let out = words(text);
    let prompt = words(prompt);
    out.len() >= 3 && prompt.windows(out.len()).any(|w| w == out.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_echo_is_detected() {
        assert!(is_prompt_echo(
            "Koi baat nahi, main shaam ko call karta hoon.",
            ROMAN_URDU_PROMPT
        ));
        assert!(is_prompt_echo("acha theek hai", ROMAN_URDU_PROMPT));
    }

    #[test]
    fn real_speech_is_not_an_echo() {
        assert!(!is_prompt_echo(
            "Yaar, kal meeting hai, can you send me the deck before lunch?",
            ROMAN_URDU_PROMPT
        ));
        // Too short to call: could be real.
        assert!(!is_prompt_echo("Okay.", ROMAN_URDU_PROMPT));
        assert!(!is_prompt_echo("Let me check.", "Okay, let me see"));
        assert!(!is_prompt_echo("", ROMAN_URDU_PROMPT));
    }

    #[test]
    fn longer_speech_that_contains_prompt_words_survives() {
        assert!(!is_prompt_echo(
            "Acha, main kal invoice check karke aapko bata deta hoon.",
            ROMAN_URDU_PROMPT
        ));
    }
}
