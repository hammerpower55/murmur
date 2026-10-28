//! Useful Sensors Moonshine Base (English) through ONNX Runtime, using the
//! int8 "merged" export from UsefulSensors/moonshine:
//!
//! 1. The encoder takes raw 16 kHz audio and returns hidden states.
//! 2. The merged decoder generates tokens greedily. Its first step runs the
//!    branch without a cache; later steps feed back the self attention cache
//!    and reuse the cross attention cache from the first step.
//!
//! Cache tensor names and shapes are read from the model, so nothing here
//! depends on the layer count.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use ort::session::{Session, SessionInputValue};
use ort::value::{DynValue, Tensor};

use super::onnx::{argmax, session};
use super::{EngineOptions, Segment, SpeechEngine, Transcript};

pub const ENCODER: &str = "encoder_model.onnx";
pub const DECODER: &str = "decoder_model_merged.onnx";
pub const TOKENIZER: &str = "tokenizer.json";

const START_TOKEN: i64 = 1;
const END_TOKEN: i64 = 2;
/// Moonshine's reference code allows about 6 tokens per second of audio.
const TOKENS_PER_SECOND: f64 = 6.5;

/// One past_key_values input and the present output that feeds it.
struct CacheSlot {
    input: String,
    output: String,
    /// Cross attention: computed once, on the first step.
    encoder: bool,
    heads: usize,
    head_dim: usize,
}

struct Loaded {
    encoder: Mutex<Session>,
    encoder_mask: bool,
    decoder: Mutex<Session>,
    decoder_mask: bool,
    cache: Vec<CacheSlot>,
    tokens: Tokenizer,
}

#[derive(Default)]
pub struct MoonshineEngine {
    loaded: Option<Loaded>,
}

impl MoonshineEngine {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Decodes ids with tokenizer.json: sentencepiece style pieces where "▁" is a
/// space and <0xNN> pieces are raw bytes.
pub struct Tokenizer {
    pieces: HashMap<u32, String>,
    special: Vec<u32>,
}

impl Tokenizer {
    pub fn parse(json: &str) -> Result<Self> {
        let v: serde_json::Value = serde_json::from_str(json).context("bad tokenizer.json")?;
        let vocab = v["model"]["vocab"]
            .as_object()
            .context("tokenizer.json has no vocabulary")?;
        let mut pieces: HashMap<u32, String> = vocab
            .iter()
            .filter_map(|(piece, id)| Some((id.as_u64()? as u32, piece.clone())))
            .collect();
        let mut special = Vec::new();
        for t in v["added_tokens"].as_array().into_iter().flatten() {
            let (Some(id), Some(content)) = (t["id"].as_u64(), t["content"].as_str()) else {
                continue;
            };
            pieces.insert(id as u32, content.to_string());
            if t["special"].as_bool().unwrap_or(false) {
                special.push(id as u32);
            }
        }
        if pieces.is_empty() {
            bail!("tokenizer.json vocabulary is empty");
        }
        Ok(Self { pieces, special })
    }

    pub fn decode(&self, ids: &[i64]) -> String {
        let mut bytes = Vec::new();
        for &id in ids {
            let id = id as u32;
            if self.special.contains(&id) {
                continue;
            }
            let Some(piece) = self.pieces.get(&id) else {
                continue;
            };
            let byte = piece
                .strip_prefix("<0x")
                .and_then(|h| h.strip_suffix('>'))
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            match byte {
                Some(b) => bytes.push(b),
                None => bytes.extend_from_slice(piece.replace('\u{2581}', " ").as_bytes()),
            }
        }
        let text = String::from_utf8_lossy(&bytes);
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

fn has_input(s: &Session, name: &str) -> bool {
    s.inputs().iter().any(|i| i.name() == name)
}

/// past_key_values.N.{decoder,encoder}.{key,value} inputs, with their
/// present.* outputs and [batch, heads, length, head_dim] sizes.
fn cache_slots(decoder: &Session) -> Result<Vec<CacheSlot>> {
    let mut slots = Vec::new();
    for input in decoder.inputs() {
        let name = input.name();
        let Some(rest) = name.strip_prefix("past_key_values.") else {
            continue;
        };
        let shape = input
            .dtype()
            .tensor_shape()
            .with_context(|| format!("{name} is not a tensor"))?;
        if shape.len() != 4 || shape[1] <= 0 || shape[3] <= 0 {
            bail!("{name} has an unexpected shape {shape:?}");
        }
        let output = format!("present.{rest}");
        if !decoder.outputs().iter().any(|o| o.name() == output) {
            bail!("decoder has no {output} output");
        }
        slots.push(CacheSlot {
            input: name.to_string(),
            output,
            encoder: rest.contains(".encoder."),
            heads: shape[1] as usize,
            head_dim: shape[3] as usize,
        });
    }
    if slots.is_empty() {
        bail!("decoder has no cache inputs");
    }
    Ok(slots)
}

impl SpeechEngine for MoonshineEngine {
    fn id(&self) -> &'static str {
        "moonshine"
    }

    fn load(&mut self, model_path: &Path, opts: &EngineOptions) -> Result<()> {
        self.unload();
        let json = std::fs::read_to_string(model_path.join(TOKENIZER))
            .with_context(|| format!("could not read {TOKENIZER}"))?;
        let tokens = Tokenizer::parse(&json)?;
        let encoder = session(model_path, ENCODER, opts.n_threads)?;
        let decoder = session(model_path, DECODER, opts.n_threads)?;
        let cache = cache_slots(&decoder)?;
        self.loaded = Some(Loaded {
            encoder_mask: has_input(&encoder, "attention_mask"),
            encoder: Mutex::new(encoder),
            decoder_mask: has_input(&decoder, "encoder_attention_mask"),
            decoder: Mutex::new(decoder),
            cache,
            tokens,
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
        let n = pcm16k_mono.len();
        let empty = |started: Instant| Transcript {
            text: String::new(),
            language: None,
            segments: Vec::new(),
            inference_ms: started.elapsed().as_millis() as u64,
        };
        // The encoder needs a few frames to work with.
        if n < 1600 {
            return Ok(empty(started));
        }

        // 1. Encoder: [1, frames, hidden].
        let (hidden, hidden_shape) = {
            let mut enc = m.encoder.lock().map_err(|_| anyhow!("encoder poisoned"))?;
            let mut inputs: Vec<(String, SessionInputValue)> = vec![(
                "input_values".into(),
                Tensor::from_array(([1usize, n], pcm16k_mono.to_vec()))?.into(),
            )];
            if m.encoder_mask {
                inputs.push((
                    "attention_mask".into(),
                    Tensor::from_array(([1usize, n], vec![1i64; n]))?.into(),
                ));
            }
            let out = enc.run(inputs)?;
            let (shape, data) = out["last_hidden_state"].try_extract_tensor::<f32>()?;
            if shape.len() != 3 {
                bail!("unexpected encoder output shape {shape:?}");
            }
            (data.to_vec(), [1usize, shape[1] as usize, shape[2] as usize])
        };
        let frames = hidden_shape[1];

        // 2. Greedy decoding.
        let max_tokens = ((n as f64 / 16_000.0) * TOKENS_PER_SECOND).ceil() as usize + 4;
        let mut decoder = m.decoder.lock().map_err(|_| anyhow!("decoder poisoned"))?;
        // Self attention cache, replaced every step, and cross attention
        // cache, set once on the first step.
        let mut cache: HashMap<String, DynValue> = HashMap::new();
        let mut cross: HashMap<String, DynValue> = HashMap::new();
        let mut generated: Vec<i64> = Vec::new();
        let mut last = START_TOKEN;
        for step in 0..max_tokens {
            let first = step == 0;
            let mut inputs: Vec<(String, SessionInputValue)> = vec![
                (
                    "input_ids".into(),
                    Tensor::from_array(([1usize, 1], vec![last]))?.into(),
                ),
                (
                    "encoder_hidden_states".into(),
                    Tensor::from_array((hidden_shape, hidden.clone()))?.into(),
                ),
                (
                    "use_cache_branch".into(),
                    Tensor::from_array(([1usize], vec![!first]))?.into(),
                ),
            ];
            if m.decoder_mask {
                inputs.push((
                    "encoder_attention_mask".into(),
                    Tensor::from_array(([1usize, frames], vec![1i64; frames]))?.into(),
                ));
            }
            for slot in &m.cache {
                let value: SessionInputValue = match if slot.encoder {
                    cross.get(&slot.input).map(SessionInputValue::from)
                } else {
                    cache.remove(&slot.input).map(SessionInputValue::from)
                } {
                    Some(v) => v,
                    // The first step takes the branch without a cache and
                    // never reads these; any well formed tensor will do.
                    None => Tensor::from_array((
                        [1usize, slot.heads, 1, slot.head_dim],
                        vec![0.0f32; slot.heads * slot.head_dim],
                    ))?
                    .into(),
                };
                inputs.push((slot.input.clone(), value));
            }

            let mut out = decoder.run(inputs)?;
            let (shape, logits) = out["logits"].try_extract_tensor::<f32>()?;
            let vocab = *shape.last().context("logits have no shape")? as usize;
            if vocab == 0 || logits.len() < vocab {
                bail!("unexpected logits shape {shape:?}");
            }
            let token = argmax(&logits[logits.len() - vocab..]) as i64;

            for slot in &m.cache {
                // Cross attention keys come from the first step only; later
                // steps may return placeholders for them.
                if slot.encoder && !first {
                    continue;
                }
                let value = out
                    .remove(&slot.output)
                    .with_context(|| format!("decoder did not return {}", slot.output))?;
                if slot.encoder {
                    cross.insert(slot.input.clone(), value);
                } else {
                    cache.insert(slot.input.clone(), value);
                }
            }

            if token == END_TOKEN {
                break;
            }
            generated.push(token);
            last = token;
        }
        drop(decoder);

        let text = m.tokens.decode(&generated);
        let segments = if text.is_empty() {
            Vec::new()
        } else {
            vec![Segment {
                start_ms: 0,
                end_ms: n as u64 * 1000 / 16_000,
                text: text.clone(),
            }]
        };
        Ok(Transcript {
            text,
            language: Some("en".into()),
            segments,
            inference_ms: started.elapsed().as_millis() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::onnx;

    const TOKENIZER_JSON: &str = r#"{
        "added_tokens": [
            {"id": 0, "content": "<unk>", "special": true},
            {"id": 1, "content": "<s>", "special": true},
            {"id": 2, "content": "</s>", "special": true}
        ],
        "model": {"type": "BPE", "vocab": {
            "<unk>": 0, "<s>": 1, "</s>": 2,
            "<0xC3>": 3, "<0xA9>": 4,
            "▁hel": 5, "lo": 6, "▁caf": 7, ".": 8
        }}
    }"#;

    #[test]
    fn tokenizer_joins_pieces_and_bytes() {
        let t = Tokenizer::parse(TOKENIZER_JSON).unwrap();
        assert_eq!(t.decode(&[1, 5, 6, 7, 3, 4, 8, 2]), "hello café.");
        assert_eq!(t.decode(&[]), "");
    }

    #[test]
    fn tokenizer_rejects_garbage() {
        assert!(Tokenizer::parse("{}").is_err());
        assert!(Tokenizer::parse("not json").is_err());
    }

    #[test]
    #[ignore = "needs the Moonshine download and ONNX Runtime"]
    fn probe_model_io() {
        onnx::use_vendored_runtime();
        let dir = onnx::installed_model_dir("moonshine-base");
        for file in [ENCODER, DECODER] {
            onnx::print_io(&dir, file);
        }
    }
}
