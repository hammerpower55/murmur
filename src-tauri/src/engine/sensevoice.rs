//! Alibaba SenseVoice Small (Chinese, English, Japanese, Korean, Cantonese)
//! through ONNX Runtime, using the sherpa-onnx int8 export. The pipeline
//! follows sherpa-onnx and FunASR:
//!
//! 1. 80 bin Kaldi filterbank on 16 bit scale audio.
//! 2. Low frame rate stacking (7 frames every 6) and CMVN, both read from the
//!    model's metadata.
//! 3. One encoder pass gives CTC logits. The first four outputs are tags
//!    (language, emotion, event, text normalisation); greedy CTC decodes the
//!    rest.

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use ort::session::Session;
use ort::value::Tensor;

use super::fbank::{stack_frames, Fbank};
use super::onnx::{argmax, session};
use super::{EngineOptions, Segment, SpeechEngine, Transcript};

pub const MODEL: &str = "model.int8.onnx";
pub const TOKENS: &str = "tokens.txt";

/// The tags SenseVoice emits before the text.
const TAG_FRAMES: usize = 4;
/// Each stacked frame covers 6 x 10 ms.
const FRAME_MS: u64 = 60;

struct Meta {
    lfr_m: usize,
    lfr_n: usize,
    neg_mean: Vec<f32>,
    inv_stddev: Vec<f32>,
    /// Scale audio to 16 bit range first (normalize_samples = 0).
    scale_samples: bool,
    with_itn: i32,
    lang_auto: i32,
    languages: Vec<(String, i32)>,
}

struct Loaded {
    model: Mutex<Session>,
    meta: Meta,
    fbank: Fbank,
    tokens: Vec<String>,
    blank: usize,
}

#[derive(Default)]
pub struct SenseVoiceEngine {
    loaded: Option<Loaded>,
}

impl SenseVoiceEngine {
    pub fn new() -> Self {
        Self::default()
    }
}

fn read_meta(model: &Session) -> Result<Meta> {
    let meta = model.metadata().context("model has no metadata")?;
    let text = |key: &str| meta.custom(key).ok_or_else(|| anyhow!("model metadata lacks {key}"));
    let int = |key: &str, fallback: i32| {
        meta.custom(key)
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(fallback)
    };
    let floats = |key: &str| -> Result<Vec<f32>> {
        text(key)?
            .split(',')
            .map(|v| v.trim().parse::<f32>().context("bad number in metadata"))
            .collect()
    };
    // Defaults are sherpa-onnx's values for this export.
    let languages = [("zh", 3), ("en", 4), ("yue", 7), ("ja", 11), ("ko", 12)]
        .iter()
        .map(|(code, id)| (code.to_string(), int(&format!("lang_{code}"), *id)))
        .collect();
    let m = Meta {
        lfr_m: int("lfr_window_size", 7) as usize,
        lfr_n: int("lfr_window_shift", 6) as usize,
        neg_mean: floats("neg_mean")?,
        inv_stddev: floats("inv_stddev")?,
        scale_samples: int("normalize_samples", 0) == 0,
        with_itn: int("with_itn", 14),
        lang_auto: int("lang_auto", 0),
        languages,
    };
    if m.lfr_m == 0 || m.lfr_n == 0 || m.neg_mean.len() != m.inv_stddev.len() {
        bail!("model metadata is inconsistent");
    }
    Ok(m)
}

/// tokens.txt: one "token id" pair per line. The blank is "<blk>" or "<blank>",
/// else id 0.
pub fn parse_tokens(text: &str) -> Result<(Vec<String>, usize)> {
    let mut pairs = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let (token, id) = line
            .rsplit_once(' ')
            .ok_or_else(|| anyhow!("bad token line: {line}"))?;
        let id: usize = id.trim().parse().context("bad token id")?;
        pairs.push((id, token.to_string()));
    }
    let size = pairs.iter().map(|(id, _)| id + 1).max().unwrap_or(0);
    if size == 0 {
        bail!("no tokens");
    }
    let mut tokens = vec![String::new(); size];
    for (id, token) in pairs {
        tokens[id] = token;
    }
    let blank = tokens
        .iter()
        .position(|t| t == "<blk>" || t == "<blank>")
        .unwrap_or(0);
    Ok((tokens, blank))
}

/// Greedy CTC: best token per frame, repeats merged, blanks dropped.
pub fn ctc_greedy(best: &[usize], blank: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut previous = None;
    for &t in best {
        if Some(t) != previous && t != blank {
            out.push(t);
        }
        previous = Some(t);
    }
    out
}

/// Joins sentencepiece tokens, dropping tags such as <|en|>.
pub fn detokenize(tokens: &[String], ids: &[usize]) -> String {
    let text: String = ids
        .iter()
        .filter_map(|&i| tokens.get(i))
        .filter(|t| !(t.starts_with("<|") && t.ends_with("|>")))
        .map(|t| t.replace('\u{2581}', " "))
        .collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl SpeechEngine for SenseVoiceEngine {
    fn id(&self) -> &'static str {
        "sensevoice"
    }

    fn load(&mut self, model_path: &Path, opts: &EngineOptions) -> Result<()> {
        self.unload();
        let text = std::fs::read_to_string(model_path.join(TOKENS))
            .with_context(|| format!("could not read {TOKENS}"))?;
        let (tokens, blank) = parse_tokens(&text)?;
        let model = session(model_path, MODEL, opts.n_threads)?;
        let meta = read_meta(&model)?;
        self.loaded = Some(Loaded {
            model: Mutex::new(model),
            meta,
            fbank: Fbank::new(80),
            tokens,
            blank,
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
        let m = self.loaded.as_ref().context("no model loaded")?;
        let started = Instant::now();

        let scaled: Vec<f32>;
        let samples = if m.meta.scale_samples {
            scaled = pcm16k_mono.iter().map(|s| s * 32768.0).collect();
            &scaled
        } else {
            pcm16k_mono
        };
        let mut frames = stack_frames(&m.fbank.compute(samples), m.meta.lfr_m, m.meta.lfr_n);
        let dim = m.meta.neg_mean.len();
        for frame in &mut frames {
            if frame.len() != dim {
                bail!("features have {} values, the model wants {dim}", frame.len());
            }
            for ((v, mean), scale) in frame.iter_mut().zip(&m.meta.neg_mean).zip(&m.meta.inv_stddev) {
                *v = (*v + mean) * scale;
            }
        }
        if frames.is_empty() {
            return Ok(Transcript {
                text: String::new(),
                language: None,
                segments: Vec::new(),
                inference_ms: started.elapsed().as_millis() as u64,
            });
        }

        let count = frames.len();
        let lang_id = language
            .and_then(|code| m.meta.languages.iter().find(|(c, _)| c == code))
            .map_or(m.meta.lang_auto, |(_, id)| *id);
        let (best, vocab) = {
            let mut model = m.model.lock().map_err(|_| anyhow!("model session poisoned"))?;
            let out = model.run(ort::inputs![
                "x" => Tensor::from_array(([1usize, count, dim], frames.concat()))?,
                "x_length" => Tensor::from_array(([1usize], vec![count as i32]))?,
                "language" => Tensor::from_array(([1usize], vec![lang_id]))?,
                "text_norm" => Tensor::from_array(([1usize], vec![m.meta.with_itn]))?,
            ])?;
            let (shape, logits) = out["logits"].try_extract_tensor::<f32>()?;
            if shape.len() != 3 {
                bail!("unexpected output shape {shape:?}");
            }
            let vocab = shape[2] as usize;
            let best: Vec<usize> = logits.chunks(vocab).map(argmax).collect();
            (best, vocab)
        };
        if vocab != m.tokens.len() {
            log::debug!("model has {vocab} outputs, tokens.txt {}", m.tokens.len());
        }

        // The language tag, e.g. "<|en|>", comes first.
        let detected = best
            .first()
            .and_then(|&t| m.tokens.get(t))
            .and_then(|t| t.strip_prefix("<|")?.strip_suffix("|>"))
            .filter(|code| m.meta.languages.iter().any(|(c, _)| c == code))
            .map(String::from);
        let body = best.get(TAG_FRAMES..).unwrap_or(&[]);
        let text = detokenize(&m.tokens, &ctc_greedy(body, m.blank));
        let segments = if text.is_empty() {
            Vec::new()
        } else {
            vec![Segment {
                start_ms: 0,
                end_ms: count as u64 * FRAME_MS,
                text: text.clone(),
            }]
        };
        Ok(Transcript {
            text,
            language: detected,
            segments,
            inference_ms: started.elapsed().as_millis() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::onnx;

    #[test]
    fn tokens_parse_and_find_the_blank() {
        let (tokens, blank) = parse_tokens("<blank> 0\n<s> 1\n\u{2581}hello 2\n").unwrap();
        assert_eq!((tokens.len(), blank), (3, 0));
        let (_, blank) = parse_tokens("a 0\n<blk> 1\n").unwrap();
        assert_eq!(blank, 1);
        assert!(parse_tokens("").is_err());
    }

    #[test]
    fn ctc_merges_repeats_and_drops_blanks() {
        assert_eq!(ctc_greedy(&[0, 5, 5, 0, 5, 7, 7, 0], 0), vec![5, 5, 7]);
        assert!(ctc_greedy(&[0, 0], 0).is_empty());
    }

    #[test]
    fn detokenize_drops_tags() {
        let tokens: Vec<String> = ["<blank>", "<|en|>", "\u{2581}hel", "lo", "\u{2581}there", "."]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(detokenize(&tokens, &[1, 2, 3, 4, 5]), "hello there.");
    }

    #[test]
    #[ignore = "needs the SenseVoice download and ONNX Runtime"]
    fn probe_model_io() {
        onnx::use_vendored_runtime();
        onnx::print_io(&onnx::installed_model_dir("sensevoice-small"), MODEL);
    }
}
