//! Whisper on the Snapdragon X NPU, from Qualcomm AI Hub's ONNX exports
//! (Distil-Whisper, Whisper Large V3 Turbo): encoder.onnx turns 30 s of log
//! mel features into each decoder layer's cross attention keys and values;
//! decoder.onnx takes one token at a time with a fixed self attention cache
//! of 199 slots that the model shifts along itself, a mask of 200 that opens
//! one slot per step from the end, and the position. Both run on the NPU.
//!
//! A model that is not precompiled (Distil-Whisper ships as plain ONNX) is
//! compiled for this NPU on its first load and kept as *_compiled.onnx.
//! Audio over 30 s is transcribed in 30 s pieces. Floats are fp32 or fp16,
//! whichever the build takes (the precompiled Turbo is fp16 throughout).

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use ort::session::{Session, SessionInputValue};
use half::f16;
use ort::value::{DynValue, Tensor, TensorElementType};
use tokenizers::Tokenizer;

use super::onnx::{npu_session, npu_session_compiled};
use super::whisper::is_prompt_echo;
use super::whisper_mel::{log_mel, FRAMES, WINDOW_SAMPLES};
use super::{EngineOptions, Segment, SpeechEngine, Transcript};

pub const ENCODER: &str = "encoder.onnx";
const DECODER: &str = "decoder.onnx";
const TOKENIZER: &str = "tokenizer.json";
const MASK_NEG: f32 = -100.0;

/// One decoder layer's self attention cache names.
struct Layer {
    k_in: String,
    v_in: String,
    k_out: String,
    v_out: String,
}

struct Loaded {
    encoder: Session,
    decoder: Session,
    tokenizer: Tokenizer,
    mels: usize,
    /// The encoder's features, and the decoder's floats, are fp16.
    features_fp16: bool,
    fp16: bool,
    layers: Vec<Layer>,
    /// Cross attention names, in the order the encoder returns them.
    cross: Vec<String>,
    /// [heads, 1, head_dim, slots] and [heads, 1, slots, head_dim].
    k_shape: [usize; 4],
    v_shape: [usize; 4],
    /// Mask length: the most tokens one piece can take, prompt included.
    positions: usize,
    sot: u32,
    eot: u32,
    no_timestamps: u32,
    transcribe: Option<u32>,
    prev: Option<u32>,
    multilingual: bool,
    prompt: Vec<u32>,
    opts: EngineOptions,
}

#[derive(Default)]
pub struct WhisperNpuEngine {
    loaded: Option<Mutex<Loaded>>,
}

impl WhisperNpuEngine {
    pub fn new() -> Self {
        Self::default()
    }
}

fn shape4(s: &Session, name: &str) -> Result<[usize; 4]> {
    let input = s
        .inputs()
        .iter()
        .find(|i| i.name() == name)
        .with_context(|| format!("decoder has no {name}"))?;
    let shape = input.dtype().tensor_shape().with_context(|| format!("{name} is not a tensor"))?;
    if shape.len() != 4 || shape.iter().any(|&d| d <= 0) {
        bail!("{name} has an unexpected shape {shape:?}");
    }
    Ok([shape[0] as usize, shape[1] as usize, shape[2] as usize, shape[3] as usize])
}

/// A precompiled build keeps each graph's QNN context in a .bin beside it;
/// anything else is compiled for this NPU once.
fn load_session(dir: &Path, file: &str) -> Result<Session> {
    if dir.join(file.replace(".onnx", ".bin")).is_file() {
        npu_session(dir, file)
    } else {
        let compiled = file.replace(".onnx", "_compiled.onnx");
        npu_session_compiled(dir, file, &compiled)
    }
}

impl Loaded {
    fn new(dir: &Path, opts: &EngineOptions) -> Result<Self> {
        let tokenizer = Tokenizer::from_file(dir.join(TOKENIZER))
            .map_err(|e| anyhow!("could not read {TOKENIZER}: {e}"))?;
        let started = Instant::now();
        let encoder = load_session(dir, ENCODER)?;
        let decoder = load_session(dir, DECODER)?;
        log::info!("NPU Whisper ready in {} ms", started.elapsed().as_millis());

        let features = encoder.inputs().first().context("encoder has no input")?;
        let mels = features
            .dtype()
            .tensor_shape()
            .and_then(|s| s.get(1).copied())
            .filter(|&m| m > 0)
            .context("encoder input has no mel size")? as usize;
        let cross: Vec<String> = encoder.outputs().iter().map(|o| o.name().to_string()).collect();
        let mut layers = Vec::new();
        while decoder.inputs().iter().any(|i| i.name() == format!("k_cache_self_{}_in", layers.len())) {
            let n = layers.len();
            layers.push(Layer {
                k_in: format!("k_cache_self_{n}_in"),
                v_in: format!("v_cache_self_{n}_in"),
                k_out: format!("k_cache_self_{n}_out"),
                v_out: format!("v_cache_self_{n}_out"),
            });
        }
        if layers.is_empty() {
            bail!("decoder has no self attention cache inputs");
        }
        let k_shape = shape4(&decoder, &layers[0].k_in)?;
        let v_shape = shape4(&decoder, &layers[0].v_in)?;
        let positions = shape4(&decoder, "attention_mask")?[3];
        let is_fp16 = |s: &Session, name: &str| {
            s.inputs()
                .iter()
                .find(|i| i.name() == name)
                .and_then(|i| i.dtype().tensor_type())
                == Some(TensorElementType::Float16)
        };
        let features_fp16 = is_fp16(&encoder, features.name());
        let fp16 = is_fp16(&decoder, "attention_mask");

        let id = |t: &str| tokenizer.token_to_id(t);
        let sot = id("<|startoftranscript|>").context("tokenizer has no start token")?;
        let eot = id("<|endoftext|>").context("tokenizer has no end token")?;
        let no_timestamps = id("<|notimestamps|>").context("tokenizer has no notimestamps token")?;
        let multilingual = id("<|en|>").is_some();
        let prompt = match &opts.initial_prompt {
            Some(text) if multilingual => tokenizer
                .encode(format!(" {}", text.trim()), false)
                .map_err(|e| anyhow!("could not tokenize the prompt: {e}"))?
                .get_ids()
                .to_vec(),
            _ => Vec::new(),
        };
        Ok(Self {
            encoder,
            decoder,
            mels,
            features_fp16,
            fp16,
            layers,
            cross,
            k_shape,
            v_shape,
            positions,
            sot,
            eot,
            no_timestamps,
            transcribe: id("<|transcribe|>"),
            prev: id("<|startofprev|>"),
            multilingual,
            prompt,
            tokenizer,
            opts: opts.clone(),
        })
    }

    /// The tokens fed before the model writes: the prompt (as earlier
    /// text), the start token and, for multilingual models, the language
    /// and task, then no timestamps. Leaves at least half the positions
    /// for the transcript.
    fn prefix(&self, language: Option<&str>) -> Vec<u32> {
        let mut out = Vec::new();
        if let (Some(prev), false) = (self.prev, self.prompt.is_empty()) {
            let room = self.positions / 2;
            let keep = &self.prompt[self.prompt.len().saturating_sub(room)..];
            out.push(prev);
            out.extend_from_slice(keep);
        }
        out.push(self.sot);
        if self.multilingual {
            let lang = language
                .filter(|l| *l != "auto")
                .and_then(|l| self.tokenizer.token_to_id(&format!("<|{l}|>")));
            if let Some(lang) = lang {
                out.push(lang);
            }
            if let Some(t) = self.transcribe {
                out.push(t);
            }
        }
        out.push(self.no_timestamps);
        out
    }

    /// Text tokens for up to 30 s of audio.
    fn piece(&mut self, pcm: &[f32], language: Option<&str>) -> Result<Vec<u32>> {
        let mel = log_mel(pcm, self.mels);
        let features = floats(vec![1, self.mels, FRAMES], mel, self.features_fp16)?;
        let mut encoded = self.encoder.run(ort::inputs![features])?;
        let cross: Vec<(String, DynValue)> = self
            .cross
            .iter()
            .map(|name| {
                encoded
                    .remove(name)
                    .map(|v| (name.clone(), v))
                    .with_context(|| format!("encoder did not return {name}"))
            })
            .collect::<Result<_>>()?;
        drop(encoded);

        let fp16 = self.fp16;
        let zeros = |shape: [usize; 4]| floats(shape.to_vec(), vec![0f32; shape.iter().product()], fp16);
        let mut cache: Vec<(DynValue, DynValue)> = self
            .layers
            .iter()
            .map(|_| Ok((zeros(self.k_shape)?, zeros(self.v_shape)?)))
            .collect::<Result<_>>()?;
        let mut mask = vec![MASK_NEG; self.positions];
        let prefix = self.prefix(language);
        let mut out: Vec<u32> = Vec::new();
        let mut token = prefix[0];

        for step in 0..self.positions - 1 {
            mask[self.positions - 1 - step] = 0.0;
            let mut inputs: Vec<(String, SessionInputValue)> = vec![
                (
                    "input_ids".into(),
                    Tensor::from_array(([1usize, 1], vec![token as i32]))?.into(),
                ),
                (
                    "attention_mask".into(),
                    floats(vec![1, 1, 1, self.positions], mask.clone(), fp16)?.into(),
                ),
            ];
            for (layer, (k, v)) in self.layers.iter().zip(&cache) {
                inputs.push((layer.k_in.clone(), SessionInputValue::from(k)));
                inputs.push((layer.v_in.clone(), SessionInputValue::from(v)));
            }
            for (name, value) in &cross {
                inputs.push((name.clone(), SessionInputValue::from(value)));
            }
            inputs.push((
                "position_ids".into(),
                Tensor::from_array(([1usize], vec![step as i32]))?.into(),
            ));
            let mut result = self.decoder.run(inputs)?;
            let next = if step + 1 < prefix.len() {
                prefix[step + 1]
            } else {
                let logits: Vec<f32> = if fp16 {
                    let (_, l) = result["logits"].try_extract_tensor::<f16>()?;
                    l.iter().take(self.eot as usize + 1).map(|x| x.to_f32()).collect()
                } else {
                    let (_, l) = result["logits"].try_extract_tensor::<f32>()?;
                    l.iter().take(self.eot as usize + 1).copied().collect()
                };
                // Text tokens and the end token only; nothing blank first.
                let mut best = (self.eot, f32::NEG_INFINITY);
                for (i, &x) in logits.iter().enumerate() {
                    if out.is_empty() && i as u32 == self.eot {
                        continue;
                    }
                    if x > best.1 {
                        best = (i as u32, x);
                    }
                }
                best.0
            };
            cache = self
                .layers
                .iter()
                .map(|layer| {
                    let k = result.remove(&layer.k_out).with_context(|| format!("decoder did not return {}", layer.k_out))?;
                    let v = result.remove(&layer.v_out).with_context(|| format!("decoder did not return {}", layer.v_out))?;
                    Ok((k, v))
                })
                .collect::<Result<_>>()?;
            if step + 1 >= prefix.len() {
                if next == self.eot {
                    break;
                }
                out.push(next);
            }
            token = next;
        }
        Ok(out)
    }
}

/// A float tensor in the precision the model takes.
fn floats(shape: Vec<usize>, data: Vec<f32>, fp16: bool) -> Result<DynValue> {
    Ok(if fp16 {
        let half: Vec<f16> = data.into_iter().map(f16::from_f32).collect();
        Tensor::from_array((shape, half))?.into_dyn()
    } else {
        Tensor::from_array((shape, data))?.into_dyn()
    })
}

impl SpeechEngine for WhisperNpuEngine {
    fn id(&self) -> &'static str {
        "whisper-npu"
    }

    fn load(&mut self, model_path: &Path, opts: &EngineOptions) -> Result<()> {
        self.loaded = None;
        self.loaded = Some(Mutex::new(Loaded::new(model_path, opts)?));
        Ok(())
    }

    fn unload(&mut self) {
        self.loaded = None;
    }

    fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }

    fn transcribe(&self, pcm16k_mono: &[f32], language: Option<&str>) -> Result<Transcript> {
        let mut loaded = self
            .loaded
            .as_ref()
            .context("no model loaded")?
            .lock()
            .map_err(|_| anyhow!("whisper state poisoned"))?;
        let started = Instant::now();
        let mut tokens = Vec::new();
        for piece in pcm16k_mono.chunks(WINDOW_SAMPLES) {
            tokens.extend(loaded.piece(piece, language)?);
        }
        let mut text = loaded
            .tokenizer
            .decode(&tokens, true)
            .map_err(|e| anyhow!("could not detokenize: {e}"))?
            .trim()
            .to_string();
        if let Some(prompt) = &loaded.opts.initial_prompt {
            if is_prompt_echo(&text, prompt) {
                log::info!("dropped output that repeats the initial prompt");
                text.clear();
            }
        }
        let end_ms = pcm16k_mono.len() as u64 * 1000 / 16_000;
        let segments = if text.is_empty() {
            Vec::new()
        } else {
            vec![Segment {
                start_ms: 0,
                end_ms,
                text: text.clone(),
            }]
        };
        Ok(Transcript {
            text,
            language: language.filter(|l| *l != "auto").map(String::from),
            segments,
            inference_ms: started.elapsed().as_millis() as u64,
        })
    }
}

/// Transcribes the fixture clips with an NPU Whisper folder and prints the
/// text and time for each:
///
///   MURMUR_NPU_WHISPER=<folder> cargo test --release npu_whisper_clips -- --ignored --nocapture
#[cfg(test)]
mod probe {
    use super::*;
    use crate::engine::onnx;

    #[test]
    #[ignore = "needs an NPU Whisper build and the fixture clips"]
    fn npu_whisper_clips() {
        onnx::use_vendored_runtime();
        let dir = std::path::PathBuf::from(std::env::var("MURMUR_NPU_WHISPER").unwrap());
        let mut engine = WhisperNpuEngine::new();
        let started = Instant::now();
        let opts = EngineOptions {
            initial_prompt: std::env::var_os("MURMUR_PROMPT").map(|_| super::super::whisper::ROMAN_URDU_PROMPT.to_string()),
            ..EngineOptions::default()
        };
        engine.load(&dir, &opts).unwrap();
        println!("loaded in {} ms, prompt {}", started.elapsed().as_millis(), opts.initial_prompt.is_some());
        let clips = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/audio");
        let mut names: Vec<_> = std::fs::read_dir(&clips).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "wav")).collect();
        names.sort();
        for path in names.iter().take(std::env::var("MURMUR_CLIPS").ok().and_then(|n| n.parse().ok()).unwrap_or(3)) {
            let mut reader = hound::WavReader::open(path).unwrap();
            let spec = reader.spec();
            assert_eq!((spec.sample_rate, spec.channels), (16_000, 1), "fixture clips are 16 kHz mono");
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            let pcm: Vec<f32> = reader.samples::<i32>().map(|s| s.unwrap() as f32 / scale).collect();
            let t = engine.transcribe(&pcm, Some("en")).unwrap();
            println!("{} {} ms: {}", path.file_name().unwrap().to_string_lossy(), t.inference_ms, t.text);
        }
    }
}
