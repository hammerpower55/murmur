//! A small language model through ONNX Runtime, used to score text: how
//! likely each token is given the ones before it (text/fixup.rs uses it to
//! judge whether a user's word fits better than what was heard). Two kinds
//! of export:
//!
//! - CPU: onnxruntime-genai exports (onnx-community/Qwen3-1.7B-ONNX,
//!   cpu-int4): one model.onnx taking input_ids, attention_mask and
//!   past_key_values.N.{key,value}, returning logits and present.N.{key,value}.
//! - NPU: Foundry Local's QNN split (llmware's *-onnx-qnn builds). The
//!   embeddings and lm_head run on the CPU; context_ctx.onnx and
//!   iterator_ctx.onnx hold the transformer as precompiled QNN graphs for the
//!   Hexagon NPU, with attention and its cache on the CPU between them. The
//!   context graph takes a fixed window of tokens (64), the iterator one.
//!   Their cache has the same [1, heads, length, head_dim] layout as the CPU
//!   export's, so everything above `run_with` is shared.
//!
//! Layer and head counts come from the model.
//!
//! A scored text keeps its cache, so an alternative only runs from the point
//! where it differs. The same model also writes replies for the Chat page
//! (`generate`), so the word fixer can be tried out directly.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{anyhow, bail, Context, Result};
use ort::session::{Session, SessionInputValue};
use ort::value::{DynValue, Tensor};
use tokenizers::Tokenizer;

use super::onnx::{npu_session, session};

pub const MODEL: &str = "model.onnx";
pub const TOKENIZER: &str = "tokenizer.json";
/// Only NPU builds have it.
pub const NPU_CONTEXT: &str = "context_ctx.onnx";
const NPU_ITERATOR: &str = "iterator_ctx.onnx";
const NPU_EMBEDDINGS: &str = "embeddings.onnx";
const NPU_HEAD: &str = "lm_head.onnx";

/// One layer's cache inputs and the outputs that feed them.
struct Layer {
    key_in: String,
    value_in: String,
    key_out: String,
    value_out: String,
}

/// Keys and values for every layer, `len` positions long.
struct Cache {
    values: Vec<(DynValue, DynValue)>,
    len: usize,
}

enum Backend {
    Cpu { session: Session, position_ids: bool },
    Npu(Box<Npu>),
}

/// The four sessions of an NPU build.
struct Npu {
    embed: Session,
    context: Session,
    iterator: Session,
    head: Session,
    /// Tokens the context graph takes per run.
    window: usize,
    /// The embeddings' output, which both graphs take.
    embedded: String,
    /// The graphs' last hidden state, which the head takes.
    hidden: String,
    head_in: String,
}

impl Npu {
    fn load(dir: &Path, threads: usize) -> Result<Self> {
        let context = npu_session(dir, NPU_CONTEXT)?;
        let iterator = npu_session(dir, NPU_ITERATOR)?;
        let embed = session(dir, NPU_EMBEDDINGS, threads)?;
        let head = session(dir, NPU_HEAD, threads)?;
        let embedded = context
            .inputs()
            .iter()
            .find(|i| !i.name().starts_with("past_") && !i.name().ends_with("_seq_len"))
            .context("context graph has no embeddings input")?;
        let window = embedded
            .dtype()
            .tensor_shape()
            .and_then(|shape| shape.get(1).copied())
            .filter(|&w| w > 0)
            .context("context graph has no fixed window")? as usize;
        let embedded = embedded.name().to_string();
        let hidden = context
            .outputs()
            .iter()
            .find(|o| !o.name().starts_with("present."))
            .context("context graph has no hidden state output")?
            .name()
            .to_string();
        let head_in = head.inputs().first().context("lm_head has no input")?.name().to_string();
        Ok(Self {
            embed,
            context,
            iterator,
            head,
            window,
            embedded,
            hidden,
            head_in,
        })
    }
}

pub struct Llm {
    backend: Mutex<Backend>,
    tokenizer: Tokenizer,
    layers: Vec<Layer>,
    kv_heads: usize,
    head_dim: usize,
}

impl Llm {
    /// Loads a CPU export, or an NPU build when the folder holds one (which
    /// fails on a PC without the NPU it was compiled for).
    pub fn load(dir: &Path, threads: usize) -> Result<Self> {
        let tokenizer = Tokenizer::from_file(dir.join(TOKENIZER))
            .map_err(|e| anyhow!("could not read {TOKENIZER}: {e}"))?;
        let backend = if dir.join(NPU_CONTEXT).is_file() {
            Backend::Npu(Box::new(Npu::load(dir, threads)?))
        } else {
            let session = session(dir, MODEL, threads)?;
            let position_ids = session.inputs().iter().any(|i| i.name() == "position_ids");
            Backend::Cpu { session, position_ids }
        };
        let (layers, kv_heads, head_dim) = match &backend {
            Backend::Cpu { session, .. } => layers(session)?,
            Backend::Npu(npu) => layers(&npu.context)?,
        };
        Ok(Self {
            backend: Mutex::new(backend),
            tokenizer,
            layers,
            kv_heads,
            head_dim,
        })
    }

    /// Whether this model runs on the NPU.
    pub fn on_npu(&self) -> bool {
        self.backend.lock().is_ok_and(|b| matches!(*b, Backend::Npu(_)))
    }

    fn empty_cache(&self) -> Result<Cache> {
        let shape = [1usize, self.kv_heads, 0, self.head_dim];
        let values = self
            .layers
            .iter()
            .map(|_| {
                Ok((
                    Tensor::from_array((shape, Vec::<f32>::new()))?.into_dyn(),
                    Tensor::from_array((shape, Vec::<f32>::new()))?.into_dyn(),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Cache { values, len: 0 })
    }

    /// Runs `ids` after `past`, calling `row` with the position (counted
    /// from the first of `ids`) and logits of each of the last `rows`
    /// positions, in order. Returns the grown cache.
    fn run_with(
        &self,
        backend: &mut Backend,
        ids: &[u32],
        past: &Cache,
        rows: usize,
        row: impl FnMut(usize, &[f32]),
    ) -> Result<Cache> {
        match backend {
            Backend::Cpu { session, position_ids } => {
                self.run_cpu(session, *position_ids, ids, past, rows, row)
            }
            Backend::Npu(npu) => self.run_npu(npu, ids, past, rows, row),
        }
    }

    fn run_cpu(
        &self,
        s: &mut Session,
        position_ids: bool,
        ids: &[u32],
        past: &Cache,
        rows: usize,
        mut row: impl FnMut(usize, &[f32]),
    ) -> Result<Cache> {
        let n = ids.len();
        let total = past.len + n;
        let mut inputs: Vec<(String, SessionInputValue)> = vec![
            (
                "input_ids".into(),
                Tensor::from_array(([1usize, n], ids.iter().map(|&i| i as i64).collect::<Vec<_>>()))?
                    .into(),
            ),
            (
                "attention_mask".into(),
                Tensor::from_array(([1usize, total], vec![1i64; total]))?.into(),
            ),
        ];
        if position_ids {
            let positions = (past.len as i64..total as i64).collect::<Vec<_>>();
            inputs.push((
                "position_ids".into(),
                Tensor::from_array(([1usize, n], positions))?.into(),
            ));
        }
        self.push_cache(&mut inputs, past);
        let mut out = s.run(inputs)?;

        if rows > 0 {
            let (shape, logits) = out["logits"].try_extract_tensor::<f32>()?;
            let vocab = *shape.last().context("logits have no shape")? as usize;
            let positions = logits.len().checked_div(vocab).unwrap_or(0);
            if positions < rows {
                bail!("unexpected logits shape {shape:?}");
            }
            for p in positions - rows..positions {
                row(p + n - positions, &logits[p * vocab..(p + 1) * vocab]);
            }
        }
        self.take_cache(&mut out, total)
    }

    /// The NPU split: each chunk of `ids` is embedded on the CPU, run through
    /// the context graph (padded to its window) or, for a lone token, the
    /// iterator, and the wanted rows of its hidden state go through lm_head.
    /// Padding only ever follows real tokens, so it never changes their
    /// results, and it is cut from the cache.
    fn run_npu(
        &self,
        npu: &mut Npu,
        ids: &[u32],
        past: &Cache,
        rows: usize,
        mut row: impl FnMut(usize, &[f32]),
    ) -> Result<Cache> {
        let n = ids.len();
        if n == 0 {
            bail!("nothing to run");
        }
        let first_row = n.saturating_sub(rows);
        let mut grown: Option<Cache> = None;
        let mut start = 0;
        while start < n {
            let past = grown.as_ref().unwrap_or(past);
            let base = past.len;
            let width = if n - start == 1 { 1 } else { npu.window };
            let take = (n - start).min(width);
            let mut chunk: Vec<i64> = ids[start..start + take].iter().map(|&i| i as i64).collect();
            chunk.resize(width, 0);
            let embedded = npu
                .embed
                .run(ort::inputs!["input_ids" => Tensor::from_array(([1usize, width], chunk))?])?
                .remove(&npu.embedded)
                .context("embeddings returned nothing")?;
            let total = base + width;
            let mut inputs: Vec<(String, SessionInputValue)> = vec![(npu.embedded.clone(), embedded.into())];
            self.push_cache(&mut inputs, past);
            inputs.push((
                "past_seq_len".into(),
                Tensor::from_array(([1usize, 1], vec![total as i32 - 1]))?.into(),
            ));
            inputs.push((
                "total_seq_len".into(),
                Tensor::from_array(((), vec![total as i32]))?.into(),
            ));
            let graph = if width == 1 { &mut npu.iterator } else { &mut npu.context };
            let mut out = graph.run(inputs)?;

            let (lo, hi) = (first_row.max(start), start + take);
            if lo < hi {
                let hidden = out.remove(&npu.hidden).context("model returned no hidden state")?;
                let hidden = if hi - lo == width { hidden } else { rows_of(&hidden, lo - start, hi - start)? };
                let logits = npu.head.run(ort::inputs![npu.head_in.as_str() => hidden])?;
                let (shape, logits) = logits["logits"].try_extract_tensor::<f32>()?;
                let vocab = *shape.last().context("logits have no shape")? as usize;
                if logits.len() < (hi - lo) * vocab {
                    bail!("unexpected logits shape {shape:?}");
                }
                for (k, p) in (lo..hi).enumerate() {
                    row(p, &logits[k * vocab..(k + 1) * vocab]);
                }
            }
            let cache = self.take_cache(&mut out, total)?;
            grown = Some(if take < width { self.prefix_of(&cache, base + take)? } else { cache });
            start += take;
        }
        grown.context("nothing was run")
    }

    fn push_cache<'a>(&self, inputs: &mut Vec<(String, SessionInputValue<'a>)>, past: &'a Cache) {
        for (layer, (k, v)) in self.layers.iter().zip(&past.values) {
            inputs.push((layer.key_in.clone(), SessionInputValue::from(k)));
            inputs.push((layer.value_in.clone(), SessionInputValue::from(v)));
        }
    }

    /// Every layer's present keys and values from a run's outputs.
    fn take_cache(&self, out: &mut ort::session::SessionOutputs, len: usize) -> Result<Cache> {
        let mut values = Vec::with_capacity(self.layers.len());
        for layer in &self.layers {
            let k = out
                .remove(&layer.key_out)
                .with_context(|| format!("model did not return {}", layer.key_out))?;
            let v = out
                .remove(&layer.value_out)
                .with_context(|| format!("model did not return {}", layer.value_out))?;
            values.push((k, v));
        }
        Ok(Cache { values, len })
    }

    /// A copy of the first `keep` positions of every layer's cache.
    fn prefix_of(&self, cache: &Cache, keep: usize) -> Result<Cache> {
        let (heads, dim, len) = (self.kv_heads, self.head_dim, cache.len);
        let cut = |value: &DynValue| -> Result<DynValue> {
            let (_, data) = value.try_extract_tensor::<f32>()?;
            if data.len() != heads * len * dim {
                bail!("cache has {} values, expected {}", data.len(), heads * len * dim);
            }
            let mut kept = Vec::with_capacity(heads * keep * dim);
            for h in 0..heads {
                let start = h * len * dim;
                kept.extend_from_slice(&data[start..start + keep * dim]);
            }
            Ok(Tensor::from_array(([1usize, heads, keep, dim], kept))?.into_dyn())
        };
        let values = cache
            .values
            .iter()
            .map(|(k, v)| Ok((cut(k)?, cut(v)?)))
            .collect::<Result<Vec<_>>>()?;
        Ok(Cache { values, len: keep })
    }

    /// Tokens of `text` with the byte range each one covers.
    pub fn encode_offsets(&self, text: &str) -> Result<(Vec<u32>, Vec<Span>)> {
        let enc = self
            .tokenizer
            .encode(text, false)
            .map_err(|e| anyhow!("could not tokenize: {e}"))?;
        Ok((enc.get_ids().to_vec(), enc.get_offsets().to_vec()))
    }

    /// Scores every token of `ids` in one pass. `top_at` lists positions
    /// whose `k` most likely tokens are wanted too.
    pub fn score(&self, ids: &[u32], top_at: &[usize], k: usize) -> Result<Scored> {
        let mut s = self.backend.lock().map_err(|_| anyhow!("model poisoned"))?;
        let empty = self.empty_cache()?;
        let mut logprobs = vec![0.0f32; ids.len()];
        let mut top = Vec::new();
        // Row p predicts token p + 1.
        let cache = self.run_with(&mut s, ids, &empty, ids.len(), |p, logits| {
            let norm = log_sum_exp(logits);
            if let Some(&next) = ids.get(p + 1) {
                logprobs[p + 1] = logits[next as usize] - norm;
            }
            if top_at.contains(&(p + 1)) {
                top.push((p + 1, top_k(logits, k, norm)));
            }
        })?;
        Ok(Scored {
            ids: ids.to_vec(),
            logprobs,
            top,
            cache,
        })
    }

    /// The text of one token, with its leading space if it has one.
    pub fn token_text(&self, id: u32) -> String {
        self.tokenizer.decode(&[id], false).unwrap_or_default()
    }

    pub fn encode(&self, text: &str) -> Result<Vec<u32>> {
        Ok(self.encode_offsets(text)?.0)
    }

    pub fn decode(&self, ids: &[u32]) -> Result<String> {
        self.tokenizer
            .decode(ids, true)
            .map_err(|e| anyhow!("could not detokenize: {e}"))
    }

    /// Writes a reply after `prompt`, one token at a time, picking the most
    /// likely token with recent tokens made less likely so it does not loop.
    /// `on_text` gets the reply so far after every token and returns false
    /// to stop. Ends at an end of turn token or after `max_new` tokens. The
    /// model is locked one step at a time, so a dictation can still be
    /// scored while a reply is being written.
    pub fn generate(&self, prompt: &[u32], max_new: usize, mut on_text: impl FnMut(&str) -> bool) -> Result<String> {
        let stop: Vec<u32> = ["<|im_end|>", "<|endoftext|>"]
            .iter()
            .filter_map(|t| self.tokenizer.token_to_id(t))
            .collect();
        let lock = || self.backend.lock().map_err(|_| anyhow!("model poisoned"));
        let mut out: Vec<u32> = Vec::new();
        let mut next = 0u32;
        let empty = self.empty_cache()?;
        let mut cache = self.run_with(&mut *lock()?, prompt, &empty, 1, |_, logits| {
            next = pick(logits, &[]);
        })?;
        while out.len() < max_new && !stop.contains(&next) {
            out.push(next);
            if !on_text(&self.decode(&out)?) {
                break;
            }
            let recent = &out[out.len().saturating_sub(RECENT)..];
            cache = self.run_with(&mut *lock()?, &[next], &cache, 1, |_, logits| {
                next = pick(logits, recent);
            })?;
        }
        self.decode(&out)
    }

    /// Log probability of `tokens` following the first `keep` tokens of a
    /// scored sequence.
    pub fn continuation(&self, scored: &Scored, keep: usize, tokens: &[u32]) -> Result<f32> {
        if keep == 0 || keep > scored.ids.len() || tokens.is_empty() {
            bail!("nothing to continue from");
        }
        let mut s = self.backend.lock().map_err(|_| anyhow!("model poisoned"))?;
        let past = self.prefix_of(&scored.cache, keep - 1)?;
        let mut fed = vec![scored.ids[keep - 1]];
        fed.extend_from_slice(tokens);
        let mut total = 0.0f32;
        self.run_with(&mut s, &fed, &past, fed.len(), |p, logits| {
            if let Some(&next) = fed.get(p + 1) {
                total += logits[next as usize] - log_sum_exp(logits);
            }
        })?;
        Ok(total)
    }

}

/// A byte range in the tokenized text.
pub type Span = (usize, usize);

/// A sequence scored by the model, with its cache for scoring alternatives.
pub struct Scored {
    pub ids: Vec<u32>,
    /// Log probability of each token given the ones before it (0 for the
    /// first).
    pub logprobs: Vec<f32>,
    /// (position, most likely tokens there with their log probabilities).
    pub top: Vec<(usize, Vec<(u32, f32)>)>,
    cache: Cache,
}

/// Rows `from..to` of a [1, rows, width] hidden state.
fn rows_of(hidden: &DynValue, from: usize, to: usize) -> Result<DynValue> {
    let (shape, data) = hidden.try_extract_tensor::<u16>()?;
    let width = *shape.last().context("hidden state has no shape")? as usize;
    let rows = data.get(from * width..to * width).context("hidden state is too short")?;
    Ok(Tensor::from_array(([1usize, to - from, width], rows.to_vec()))?.into_dyn())
}

fn log_sum_exp(logits: &[f32]) -> f32 {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    max + logits.iter().map(|&x| (x - max).exp()).sum::<f32>().ln()
}

/// The `k` most likely tokens with their log probabilities, best first.
fn top_k(logits: &[f32], k: usize, norm: f32) -> Vec<(u32, f32)> {
    let mut all: Vec<(u32, f32)> = logits.iter().enumerate().map(|(i, &x)| (i as u32, x)).collect();
    let k = k.min(all.len());
    if k == 0 {
        return Vec::new();
    }
    all.select_nth_unstable_by(k - 1, |a, b| b.1.total_cmp(&a.1));
    all.truncate(k);
    all.sort_by(|a, b| b.1.total_cmp(&a.1));
    all.into_iter().map(|(i, x)| (i, x - norm)).collect()
}

/// Tokens a reply looks back over when discouraging repeats.
const RECENT: usize = 64;
/// How much less likely a recent token is made (as in the CTRL paper).
const REPEAT_PENALTY: f32 = 1.15;

/// The most likely token, with `recent` tokens penalised.
fn pick(logits: &[f32], recent: &[u32]) -> u32 {
    let mut best = (0u32, f32::NEG_INFINITY);
    for (i, &x) in logits.iter().enumerate() {
        let x = if recent.contains(&(i as u32)) {
            if x > 0.0 {
                x / REPEAT_PENALTY
            } else {
                x * REPEAT_PENALTY
            }
        } else {
            x
        };
        if x > best.1 {
            best = (i as u32, x);
        }
    }
    best.0
}

/// past_key_values.N.key/value inputs in layer order, with the key and value
/// head count and size from their [batch, heads, length, head_dim] shapes.
fn layers(s: &Session) -> Result<(Vec<Layer>, usize, usize)> {
    let mut found: Vec<(usize, Layer)> = Vec::new();
    let mut dims = None;
    for input in s.inputs() {
        let name = input.name();
        let Some(n) = name
            .strip_prefix("past_key_values.")
            .and_then(|r| r.strip_suffix(".key"))
            .and_then(|n| n.parse::<usize>().ok())
        else {
            continue;
        };
        let shape = input
            .dtype()
            .tensor_shape()
            .with_context(|| format!("{name} is not a tensor"))?;
        if shape.len() != 4 || shape[1] <= 0 || shape[3] <= 0 {
            bail!("{name} has an unexpected shape {shape:?}");
        }
        dims = Some((shape[1] as usize, shape[3] as usize));
        let layer = Layer {
            key_in: name.to_string(),
            value_in: format!("past_key_values.{n}.value"),
            key_out: format!("present.{n}.key"),
            value_out: format!("present.{n}.value"),
        };
        if !s.inputs().iter().any(|i| i.name() == layer.value_in.as_str()) {
            bail!("model has no {} input", layer.value_in);
        }
        for wanted in [&layer.key_out, &layer.value_out] {
            if !s.outputs().iter().any(|o| o.name() == wanted.as_str()) {
                bail!("model has no {wanted} output");
            }
        }
        found.push((n, layer));
    }
    let (heads, dim) = dims.context("model has no cache inputs")?;
    found.sort_by_key(|(n, _)| *n);
    if found.iter().enumerate().any(|(i, (n, _))| i != *n) {
        bail!("model cache layers are not numbered 0 to {}", found.len() - 1);
    }
    Ok((found.into_iter().map(|(_, l)| l).collect(), heads, dim))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_takes_the_best_and_discourages_repeats() {
        let logits = [1.0f32, 3.0, 2.9];
        assert_eq!(pick(&logits, &[]), 1);
        // 3.0 / 1.15 falls below 2.9.
        assert_eq!(pick(&logits, &[1]), 2);
        assert_eq!(top_k(&logits, 2, 0.0), vec![(1, 3.0), (2, 2.9)]);
    }

    #[test]
    fn log_sum_exp_is_stable_and_exact() {
        let x = [1000.0f32, 1000.0];
        assert!((log_sum_exp(&x) - (1000.0 + 2f32.ln())).abs() < 1e-3);
        let y = [0.0f32, (3f32).ln()];
        assert!((log_sum_exp(&y) - 4f32.ln()).abs() < 1e-5);
    }
}

/// Scores a long passage in one go (windows on the NPU) and token by token,
/// which must agree, and prints how far apart they are:
///
///   MURMUR_FIXUP_ID=<model> cargo test --release long_text_scores_agree -- --ignored --nocapture
#[cfg(test)]
mod eval {
    use super::*;
    use crate::engine::{default_threads, onnx};

    #[test]
    #[ignore = "needs a downloaded word fixer"]
    fn long_text_scores_agree() {
        onnx::use_vendored_runtime();
        let id = std::env::var("MURMUR_FIXUP_ID").unwrap_or_else(|_| "qwen2.5-1.5b-instruct-npu".into());
        let llm = Llm::load(&onnx::installed_model_dir(&id), default_threads()).unwrap();
        let text = "Okay, so I want some changes to the Monopoly game. The board should be bigger, the properties should be more colourful, and the dice should have their own place on the table. I also want a sixty second timer for every turn, proper illustrations for each card, and a way to play with friends online. When a player lands on a property they do not own, show the rent clearly and let them pay with one click. At the end of the game, show who won and how much money everyone had left, and offer a rematch button.";
        let ids = llm.encode(text).unwrap();
        let whole = llm.score(&ids, &[], 0).unwrap().logprobs;
        let mut stepped = vec![0.0f32; ids.len()];
        let mut b = llm.backend.lock().unwrap();
        let mut cache = llm.empty_cache().unwrap();
        for (i, &id) in ids.iter().enumerate() {
            let next = ids.get(i + 1).copied();
            cache = llm
                .run_with(&mut b, &[id], &cache, 1, |_, logits| {
                    if let Some(next) = next {
                        stepped[i + 1] = logits[next as usize] - log_sum_exp(logits);
                    }
                })
                .unwrap();
        }
        let worst = (1..ids.len()).map(|i| (i, (whole[i] - stepped[i]).abs())).fold((0, 0.0f32), |a, b| if b.1 > a.1 { b } else { a });
        let by_window: Vec<String> = ids.chunks(64).enumerate().map(|(w, c)| {
            let start = w * 64;
            let diff: f32 = (start.max(1)..start + c.len()).map(|i| (whole[i] - stepped[i]).abs()).sum::<f32>() / c.len() as f32;
            format!("window {w}: {diff:.3}")
        }).collect();
        println!("{} tokens, total {:.1} whole vs {:.1} stepped, worst token {} off by {:.2}; mean difference {}", ids.len(), whole.iter().sum::<f32>(), stepped.iter().sum::<f32>(), worst.0, worst.1, by_window.join(", "));
        assert!(worst.1 < 1.0, "windowed and stepped scores disagree");
    }
}

