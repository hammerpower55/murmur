//! A small language model through ONNX Runtime, used to score text: how
//! likely each token is given the ones before it (text/fixup.rs uses it to
//! judge whether a user's word fits better than what was heard). Built for
//! onnxruntime-genai exports (onnx-community/Qwen3-1.7B-ONNX, cpu-int4): one
//! model.onnx taking input_ids, attention_mask and past_key_values.N.{key,
//! value}, returning logits and present.N.{key,value}. Layer and head counts
//! come from the model.
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

use super::onnx::session;

pub const MODEL: &str = "model.onnx";
pub const TOKENIZER: &str = "tokenizer.json";

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

pub struct Llm {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    layers: Vec<Layer>,
    kv_heads: usize,
    head_dim: usize,
    position_ids: bool,
}

impl Llm {
    pub fn load(dir: &Path, threads: usize) -> Result<Self> {
        let tokenizer = Tokenizer::from_file(dir.join(TOKENIZER))
            .map_err(|e| anyhow!("could not read {TOKENIZER}: {e}"))?;
        let session = session(dir, MODEL, threads)?;
        let (layers, kv_heads, head_dim) = layers(&session)?;
        let position_ids = session.inputs().iter().any(|i| i.name() == "position_ids");
        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            layers,
            kv_heads,
            head_dim,
            position_ids,
        })
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
    /// positions. Returns the grown cache.
    fn run_with(
        &self,
        s: &mut Session,
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
        if self.position_ids {
            let positions = (past.len as i64..total as i64).collect::<Vec<_>>();
            inputs.push((
                "position_ids".into(),
                Tensor::from_array(([1usize, n], positions))?.into(),
            ));
        }
        for (layer, (k, v)) in self.layers.iter().zip(&past.values) {
            inputs.push((layer.key_in.clone(), SessionInputValue::from(k)));
            inputs.push((layer.value_in.clone(), SessionInputValue::from(v)));
        }
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
        Ok(Cache { values, len: total })
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
        let mut s = self.session.lock().map_err(|_| anyhow!("model poisoned"))?;
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
        let lock = || self.session.lock().map_err(|_| anyhow!("model poisoned"));
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
        let mut s = self.session.lock().map_err(|_| anyhow!("model poisoned"))?;
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
