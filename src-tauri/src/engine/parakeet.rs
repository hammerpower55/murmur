//! NVIDIA Parakeet through ONNX Runtime on the CPU. Two exports are read:
//!
//! - TDT 0.6B v2 (English) and v3 (25 European languages), istupakov's
//!   export: encoder-model, decoder_joint-model and vocab.txt.
//! - Parakeet Unified EN 0.6B, an RNN-T model, in the sherpa-onnx export:
//!   encoder, decoder and joiner, tokens.txt. Its features match the TDT
//!   models', so it ships with the same nemo128.onnx.
//!
//! The pipeline follows istupakov/onnx-asr, as Handy's transcribe-rs does:
//!
//! 1. nemo128.onnx turns 16 kHz audio into 128 bin log mel features.
//! 2. The encoder turns features into one vector per 80 ms frame.
//! 3. Greedy decoding. TDT: the joint network predicts a token and how many
//!    frames to skip, so most frames are never visited. RNN-T: every frame
//!    is visited and emits tokens until the joiner picks blank.

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use ort::session::Session;
use ort::value::Tensor;

use super::onnx::{argmax, session};
use super::{EngineOptions, Segment, SpeechEngine, Transcript};

pub const PREPROCESSOR: &str = "nemo128.onnx";
pub const ENCODER: &str = "encoder-model.int8.onnx";
pub const DECODER: &str = "decoder_joint-model.int8.onnx";
pub const VOCAB: &str = "vocab.txt";
/// The RNN-T export's files.
pub const RNNT_ENCODER: &str = "encoder.int8.onnx";
pub const RNNT_DECODER: &str = "decoder.int8.onnx";
pub const RNNT_JOINER: &str = "joiner.int8.onnx";
pub const RNNT_VOCAB: &str = "tokens.txt";

/// Decoder state: 2 LSTM layers, batch 1, 640 hidden units.
const STATE_SHAPE: [usize; 3] = [2, 1, 640];
/// Safety valve from onnx-asr: at most this many tokens per frame.
const MAX_TOKENS_PER_STEP: usize = 10;
/// The encoder subsamples 10 ms feature frames by 8.
const FRAME_MS: u64 = 80;

/// How tokens are read off the encoder frames.
enum Decoding {
    /// decoder_joint-model: tokens and frame skips in one network.
    Tdt(Mutex<Session>),
    /// Separate prediction network and joiner.
    Rnnt {
        decoder: Mutex<Session>,
        joiner: Mutex<Session>,
    },
}

struct Loaded {
    preprocessor: Mutex<Session>,
    encoder: Mutex<Session>,
    decoding: Decoding,
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

impl SpeechEngine for ParakeetEngine {
    fn id(&self) -> &'static str {
        "parakeet"
    }

    fn load(&mut self, model_path: &Path, opts: &EngineOptions) -> Result<()> {
        self.unload();
        let threads = opts.n_threads;
        let rnnt = model_path.join(RNNT_JOINER).is_file();
        let vocab_file = if rnnt { RNNT_VOCAB } else { VOCAB };
        let vocab_text = std::fs::read_to_string(model_path.join(vocab_file))
            .with_context(|| format!("could not read {vocab_file}"))?;
        let (vocab, blank) = parse_vocab(&vocab_text)?;
        let decoding = if rnnt {
            Decoding::Rnnt {
                decoder: Mutex::new(session(model_path, RNNT_DECODER, threads)?),
                joiner: Mutex::new(session(model_path, RNNT_JOINER, threads)?),
            }
        } else {
            Decoding::Tdt(Mutex::new(session(model_path, DECODER, threads)?))
        };
        let encoder = if rnnt { RNNT_ENCODER } else { ENCODER };
        self.loaded = Some(Loaded {
            preprocessor: Mutex::new(session(model_path, PREPROCESSOR, threads)?),
            encoder: Mutex::new(session(model_path, encoder, threads)?),
            decoding,
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

        // 3. Greedy decoding.
        let (tokens, token_frames) = match &m.decoding {
            Decoding::Tdt(decoder) => {
                let mut decoder = decoder.lock().map_err(|_| lock("decoder"))?;
                decode_tdt(&mut decoder, m, &encoded, dim, frames)?
            }
            Decoding::Rnnt { decoder, joiner } => {
                let mut decoder = decoder.lock().map_err(|_| lock("decoder"))?;
                let mut joiner = joiner.lock().map_err(|_| lock("joiner"))?;
                decode_rnnt(&mut decoder, &mut joiner, m.blank, &encoded, dim, frames)?
            }
        };

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

/// Token ids and the frame each was emitted at.
type Decoded = (Vec<usize>, Vec<usize>);

/// The prediction network's LSTM state: hidden and cell.
type State = (Vec<f32>, Vec<f32>);

/// Greedy TDT: each step predicts a token and how many frames to skip.
fn decode_tdt(decoder: &mut Session, m: &Loaded, encoded: &[f32], dim: usize, frames: usize) -> Result<Decoded> {
    let vocab_size = m.vocab.len();
    let state_len: usize = STATE_SHAPE.iter().product();
    let mut state = (vec![0.0f32; state_len], vec![0.0f32; state_len]);
    let mut tokens: Vec<usize> = Vec::new();
    let mut token_frames: Vec<usize> = Vec::new();
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
    Ok((tokens, token_frames))
}

/// One step of the RNN-T prediction network: the output for `token` and
/// the next state.
fn predict(decoder: &mut Session, token: usize, state: &State) -> Result<(Vec<f32>, State)> {
    let out = decoder.run(ort::inputs![
        "targets" => Tensor::from_array(([1usize, 1], vec![token as i32]))?,
        "target_length" => Tensor::from_array(([1usize], vec![1i32]))?,
        "states.1" => Tensor::from_array((STATE_SHAPE, state.0.clone()))?,
        "onnx::Slice_3" => Tensor::from_array((STATE_SHAPE, state.1.clone()))?,
    ])?;
    let (_, output) = out["outputs"].try_extract_tensor::<f32>()?;
    let (_, h) = out["states"].try_extract_tensor::<f32>()?;
    let (_, c) = out["162"].try_extract_tensor::<f32>()?;
    Ok((output.to_vec(), (h.to_vec(), c.to_vec())))
}

/// Greedy RNN-T: every frame emits tokens until the joiner picks blank.
/// Decoding starts from blank, as NeMo and sherpa-onnx do.
fn decode_rnnt(
    decoder: &mut Session,
    joiner: &mut Session,
    blank: usize,
    encoded: &[f32],
    dim: usize,
    frames: usize,
) -> Result<Decoded> {
    let state_len: usize = STATE_SHAPE.iter().product();
    let zero = (vec![0.0f32; state_len], vec![0.0f32; state_len]);
    let (mut predicted, mut state) = predict(decoder, blank, &zero)?;
    let mut tokens = Vec::new();
    let mut token_frames = Vec::new();
    for t in 0..frames {
        let frame = encoded[t * dim..(t + 1) * dim].to_vec();
        for _ in 0..MAX_TOKENS_PER_STEP {
            let hidden = predicted.len();
            let out = joiner.run(ort::inputs![
                "encoder_outputs" => Tensor::from_array(([1usize, dim, 1], frame.clone()))?,
                "decoder_outputs" => Tensor::from_array(([1usize, hidden, 1], predicted.clone()))?,
            ])?;
            let (_, logits) = out["outputs"].try_extract_tensor::<f32>()?;
            let token = argmax(logits);
            if token == blank {
                break;
            }
            tokens.push(token);
            token_frames.push(t);
            (predicted, state) = predict(decoder, token, &state)?;
        }
    }
    Ok((tokens, token_frames))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::onnx;

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

    /// Prints the transducer files of the Parakeet Unified download.
    #[test]
    #[ignore = "needs the Parakeet Unified download and ONNX Runtime"]
    fn probe_unified_io() {
        onnx::use_vendored_runtime();
        let dir = onnx::installed_model_dir("parakeet-unified-en-0.6b");
        for file in ["decoder.int8.onnx", "joiner.int8.onnx"] {
            onnx::print_io(&dir, file);
        }
    }

    /// Prints every model's real input and output names and types.
    #[test]
    #[ignore = "needs the Parakeet v2 download and ONNX Runtime"]
    fn probe_model_io() {
        onnx::use_vendored_runtime();
        let dir = onnx::installed_model_dir("parakeet-tdt-0.6b-v2");
        for file in [PREPROCESSOR, ENCODER, DECODER] {
            onnx::print_io(&dir, file);
        }
    }
}