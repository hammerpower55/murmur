//! NVIDIA Parakeet TDT 0.6B (v2: English, v3: 25 European languages) through
//! ONNX Runtime on the CPU, using istupakov's ONNX export. The pipeline
//! follows istupakov/onnx-asr, as Handy's transcribe-rs does:
//!
//! 1. nemo128.onnx turns 16 kHz audio into 128 bin log mel features.
//! 2. The encoder turns features into one vector per 80 ms frame.
//! 3. Greedy TDT decoding: the joint network predicts a token and how many
//!    frames to skip, so most frames are never visited.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;

use super::{EngineOptions, Segment, SpeechEngine, Transcript};

pub const PREPROCESSOR: &str = "nemo128.onnx";
pub const ENCODER: &str = "encoder-model.int8.onnx";
pub const DECODER: &str = "decoder_joint-model.int8.onnx";
pub const VOCAB: &str = "vocab.txt";

/// Decoder state: 2 LSTM layers, batch 1, 640 hidden units.
const STATE_SHAPE: [usize; 3] = [2, 1, 640];
/// Safety valve from onnx-asr: at most this many tokens per frame.
const MAX_TOKENS_PER_STEP: usize = 10;
/// The encoder subsamples 10 ms feature frames by 8.
const FRAME_MS: u64 = 80;

static RUNTIME_PATH: OnceLock<PathBuf> = OnceLock::new();
static ENVIRONMENT: OnceLock<std::result::Result<ort::environment::Environment, String>> =
    OnceLock::new();

/// Where onnxruntime.dll lives. Set once at startup, before any load. The
/// DLL is loaded from this exact path so the older copy Windows keeps in
/// System32 is never picked up.
pub fn set_runtime_path(path: PathBuf) {
    let _ = RUNTIME_PATH.set(path);
}

fn environment() -> Result<&'static ort::environment::Environment> {
    let env = ENVIRONMENT.get_or_init(|| {
        let path = RUNTIME_PATH
            .get()
            .ok_or_else(|| "ONNX Runtime location not set".to_string())?;
        ort::init_from(path)
            .map_err(|e| format!("could not load {}: {e}", path.display()))?
            .with_name("murmur")
            .build()
            .map_err(|e| format!("could not start ONNX Runtime: {e}"))
    });
    env.as_ref().map_err(|e| anyhow!("{e}"))
}

struct Loaded {
    preprocessor: Mutex<Session>,
    encoder: Mutex<Session>,
    decoder: Mutex<Session>,
    vocab: Vec<String>,
    blank: usize,
}

#[derive(Default)]
pub struct ParakeetEngine {
    loaded: Option<Loaded>,
}

impl ParakeetEngine {
    pub fn new() -> Self {
        Self::default()
    }
}

fn session(dir: &Path, file: &str, threads: usize) -> Result<Session> {
    let env = environment()?;
    let path = dir.join(file);
    Session::builder(env)
        .and_then(|b| Ok(b.with_optimization_level(GraphOptimizationLevel::Level3)?))
        .and_then(|b| Ok(b.with_intra_threads(threads)?))
        .and_then(|mut b| b.commit_from_file(&path))
        .map_err(|e| anyhow!("could not load {}: {e}", path.display()))
}

/// vocab.txt has one "token id" pair per line; "<blk>" is the blank.
pub fn parse_vocab(text: &str) -> Result<(Vec<String>, usize)> {
    let mut pairs = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let (token, id) = line
            .rsplit_once(' ')
            .ok_or_else(|| anyhow!("bad vocabulary line: {line}"))?;
        let id: usize = id.trim().parse().context("bad vocabulary id")?;
        pairs.push((id, token.to_string()));
    }
    let size = pairs.iter().map(|(id, _)| id + 1).max().unwrap_or(0);
    let mut vocab = vec![String::new(); size];
    for (id, token) in pairs {
        vocab[id] = token;
    }
    let blank = vocab
        .iter()
        .position(|t| t == "<blk>")
        .ok_or_else(|| anyhow!("vocabulary has no <blk> token"))?;
    Ok((vocab, blank))
}

/// Joins sentencepiece tokens: "▁" marks the start of a word.
pub fn detokenize(vocab: &[String], tokens: &[usize]) -> String {
    let text: String = tokens
        .iter()
        .filter_map(|&t| vocab.get(t))
        .map(|t| t.replace('\u{2581}', " "))
        .collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn argmax(values: &[f32]) -> usize {
    values
        .iter()
        .enumerate()
        .fold((0, f32::NEG_INFINITY), |best, (i, &v)| {
            if v > best.1 {
                (i, v)
            } else {
                best
            }
        })
        .0
}

impl SpeechEngine for ParakeetEngine {
    fn id(&self) -> &'static str {
        "parakeet"
    }

    fn load(&mut self, model_path: &Path, opts: &EngineOptions) -> Result<()> {
        self.unload();
        let threads = opts.n_threads;
        let vocab_text = std::fs::read_to_string(model_path.join(VOCAB))
            .with_context(|| format!("could not read {VOCAB}"))?;
        let (vocab, blank) = parse_vocab(&vocab_text)?;
        self.loaded = Some(Loaded {
            preprocessor: Mutex::new(session(model_path, PREPROCESSOR, threads)?),
            encoder: Mutex::new(session(model_path, ENCODER, threads)?),
            decoder: Mutex::new(session(model_path, DECODER, threads)?),
            vocab,
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

    fn transcribe(&self, pcm16k_mono: &[f32], _language: Option<&str>) -> Result<Transcript> {
        let m = self.loaded.as_ref().context("no model loaded")?;
        let started = Instant::now();
        let lock = |s: &'static str| anyhow!("{s} session poisoned");

        // 1. Features.
        let n = pcm16k_mono.len();
        let (features, feature_shape, feature_len) = {
            let mut pre = m.preprocessor.lock().map_err(|_| lock("preprocessor"))?;
            let out = pre.run(ort::inputs![
                "waveforms" => Tensor::from_array(([1usize, n], pcm16k_mono.to_vec()))?,
                "waveforms_lens" => Tensor::from_array(([1usize], vec![n as i64]))?,
            ])?;
            let (shape, data) = out["features"].try_extract_tensor::<f32>()?;
            let (_, lens) = out["features_lens"].try_extract_tensor::<i64>()?;
            (data.to_vec(), shape.to_vec(), lens[0])
        };

        // 2. Encoder: [1, dim, frames].
        let (encoded, dim, frames) = {
            let mut enc = m.encoder.lock().map_err(|_| lock("encoder"))?;
            let out = enc.run(ort::inputs![
                "audio_signal" => Tensor::from_array((feature_shape.clone(), features))?,
                "length" => Tensor::from_array(([1usize], vec![feature_len]))?,
            ])?;
            let (shape, data) = out["outputs"].try_extract_tensor::<f32>()?;
            let (_, lens) = out["encoded_lengths"].try_extract_tensor::<i64>()?;
            if shape.len() != 3 {
                bail!("unexpected encoder output shape {shape:?}");
            }
            let dim = shape[1] as usize;
            let total = shape[2] as usize;
            let frames = (lens[0].max(0) as usize).min(total);
            // Gather each frame's vector (stride `total` between dims).
            let mut by_frame = vec![0.0f32; frames * dim];
            for t in 0..frames {
                for d in 0..dim {
                    by_frame[t * dim + d] = data[d * total + t];
                }
            }
            (by_frame, dim, frames)
        };

        // 3. Greedy TDT decoding.
        let vocab_size = m.vocab.len();
        let state_len: usize = STATE_SHAPE.iter().product();
        let mut state = (vec![0.0f32; state_len], vec![0.0f32; state_len]);
        let mut tokens: Vec<usize> = Vec::new();
        let mut token_frames: Vec<usize> = Vec::new();
        let mut decoder = m.decoder.lock().map_err(|_| lock("decoder"))?;
        let mut t = 0usize;
        let mut emitted = 0usize;
        while t < frames {
            let frame = encoded[t * dim..(t + 1) * dim].to_vec();
            let previous = tokens.last().copied().unwrap_or(m.blank) as i32;
            let out = decoder.run(ort::inputs![
                "encoder_outputs" => Tensor::from_array(([1usize, dim, 1], frame))?,
                "targets" => Tensor::from_array(([1usize, 1], vec![previous]))?,
                "target_length" => Tensor::from_array(([1usize], vec![1i32]))?,
                "input_states_1" => Tensor::from_array((STATE_SHAPE, state.0.clone()))?,
                "input_states_2" => Tensor::from_array((STATE_SHAPE, state.1.clone()))?,
            ])?;
            let (_, logits) = out["outputs"].try_extract_tensor::<f32>()?;
            if logits.len() <= vocab_size {
                bail!(
                    "decoder returned {} values for {vocab_size} tokens",
                    logits.len()
                );
            }
            let token = argmax(&logits[..vocab_size]);
            let skip = argmax(&logits[vocab_size..]);
            if token != m.blank {
                let (_, s1) = out["output_states_1"].try_extract_tensor::<f32>()?;
                let (_, s2) = out["output_states_2"].try_extract_tensor::<f32>()?;
                state = (s1.to_vec(), s2.to_vec());
                tokens.push(token);
                token_frames.push(t);
                emitted += 1;
            }
            if skip > 0 {
                t += skip;
                emitted = 0;
            } else if token == m.blank || emitted == MAX_TOKENS_PER_STEP {
                t += 1;
                emitted = 0;
            }
        }

        let text = detokenize(&m.vocab, &tokens);
        let segments = if text.is_empty() {
            Vec::new()
        } else {
            vec![Segment {
                start_ms: token_frames.first().map_or(0, |&f| f as u64 * FRAME_MS),
                end_ms: frames as u64 * FRAME_MS,
                text: text.clone(),
            }]
        };
        Ok(Transcript {
            text,
            language: None,
            segments,
            inference_ms: started.elapsed().as_millis() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocab_parsing_finds_the_blank() {
        let (vocab, blank) = parse_vocab("<unk> 0\n\u{2581}the 1\ns 2\n<blk> 3\n").unwrap();
        assert_eq!(vocab.len(), 4);
        assert_eq!(blank, 3);
        assert_eq!(vocab[1], "\u{2581}the");
    }

    #[test]
    fn vocab_without_blank_is_rejected() {
        assert!(parse_vocab("a 0\nb 1\n").is_err());
    }

    #[test]
    fn detokenize_joins_word_pieces() {
        let vocab: Vec<String> = ["\u{2581}hel", "lo", "\u{2581}world", "<blk>", "."]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(detokenize(&vocab, &[0, 1, 2, 4]), "hello world.");
        assert_eq!(detokenize(&vocab, &[]), "");
    }

    #[test]
    fn argmax_picks_the_largest() {
        assert_eq!(argmax(&[0.1, 3.0, -1.0, 2.9]), 1);
        assert_eq!(argmax(&[5.0]), 0);
    }

    fn model_dir() -> PathBuf {
        crate::paths::data_dir_named(&crate::paths::product_name())
            .unwrap()
            .join("models")
            .join("parakeet-tdt-0.6b-v2")
    }

    fn use_vendored_runtime() {
        set_runtime_path(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("vendor/onnxruntime-win-arm64-1.31.0/lib/onnxruntime.dll"),
        );
    }

    /// Prints every model's real input and output names and types.
    #[test]
    #[ignore = "needs the Parakeet v2 download and ONNX Runtime"]
    fn probe_model_io() {
        use_vendored_runtime();
        for file in [PREPROCESSOR, ENCODER, DECODER] {
            let s = session(&model_dir(), file, 2).unwrap();
            println!("== {file}");
            for i in s.inputs() {
                println!("  in  {} {:?}", i.name(), i.dtype());
            }
            for o in s.outputs() {
                println!("  out {} {:?}", o.name(), o.dtype());
            }
        }
    }
}
