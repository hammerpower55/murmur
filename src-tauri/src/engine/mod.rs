//! Speech engines. Every engine takes 16 kHz mono f32 and returns text.

mod fbank;
pub mod fixer;
pub mod host;
pub mod llm;
pub mod moonshine;
pub mod onnx;
pub mod parakeet;
pub mod sensevoice;
pub mod whisper;
mod whisper_mel;
pub mod whisper_npu;

#[cfg(test)]
mod eval;

use std::path::Path;

use anyhow::Result;

#[derive(Debug, Clone)]
pub struct EngineOptions {
    pub n_threads: usize,
    /// Text the decoder is primed with, used to steer spelling and script.
    pub initial_prompt: Option<String>,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            n_threads: default_threads(),
            initial_prompt: None,
        }
    }
}

/// max(4, physical cores minus 2): leaves room for the UI and the focused
/// app. Snapdragon X has no SMT, so logical cores are physical cores.
pub fn default_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    cores.saturating_sub(2).max(4)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
    pub segments: Vec<Segment>,
    pub inference_ms: u64,
}

pub trait SpeechEngine: Send + Sync {
    fn id(&self) -> &'static str;
    fn load(&mut self, model_path: &Path, opts: &EngineOptions) -> Result<()>;
    fn unload(&mut self);
    fn is_loaded(&self) -> bool;
    fn transcribe(&self, pcm16k_mono: &[f32], language: Option<&str>) -> Result<Transcript>;
    /// True when "Unload model when idle" should leave this engine alone:
    /// it holds little memory of its own and takes long to load again.
    fn stays_loaded_when_idle(&self) -> bool {
        false
    }
}

/// True if the text contains Arabic script (Urdu Nastaliq included). Output
/// must stay Latin unless the user explicitly picks Urdu.
pub fn has_arabic_script(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c,
            '\u{0600}'..='\u{06FF}'
            | '\u{0750}'..='\u{077F}'
            | '\u{08A0}'..='\u{08FF}'
            | '\u{FB50}'..='\u{FDFF}'
            | '\u{FE70}'..='\u{FEFF}')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threads_follow_the_spec_formula() {
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        assert_eq!(default_threads(), cores.saturating_sub(2).max(4));
        assert!(default_threads() >= 4);
    }

    #[test]
    fn detects_urdu_script() {
        assert!(has_arabic_script("یار کل میٹنگ ہے"));
        assert!(has_arabic_script("Yaar kal meeting ہے"));
    }

    #[test]
    fn latin_roman_urdu_is_not_arabic_script() {
        assert!(!has_arabic_script(
            "Yaar kal meeting hai, can you send me the deck before lunch?"
        ));
        assert!(!has_arabic_script("Café, naïve, Zürich"));
    }
}
