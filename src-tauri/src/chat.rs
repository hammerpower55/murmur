//! The Chat page: talk to the word fixer's language model directly, to see
//! what it can do. Replies are written on a background thread and streamed
//! to the page as `chat-text` events (the reply so far), ending with
//! `chat-done` (null, or one line saying what went wrong). Nothing is saved:
//! the conversation lives in the page.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Result};
use serde::Deserialize;
use tauri::{AppHandle, Emitter};

use crate::engine::fixer::Fixer;
use crate::engine::llm::Llm;

/// Qwen3 is told what it is and to keep it short; thinking is switched off
/// by an empty think block, as the model card describes.
const SYSTEM: &str = "You are a helpful assistant running offline on this computer inside Murmur, a voice dictation app. Answer briefly and plainly.";
/// Longest reply, in tokens.
const MAX_REPLY: usize = 512;
/// Oldest messages are dropped until the prompt fits in this many tokens.
const MAX_PROMPT: usize = 3000;
/// How long to wait for the model to load before giving up.
const LOAD_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    /// "user" or "assistant".
    pub role: String,
    pub content: String,
}

/// Qwen chat markup for `messages`, ending where the assistant's reply goes.
pub fn prompt(messages: &[Message]) -> String {
    let mut p = format!("<|im_start|>system\n{SYSTEM}<|im_end|>\n");
    for m in messages {
        p.push_str(&format!("<|im_start|>{}\n{}<|im_end|>\n", m.role, m.content.trim()));
    }
    p.push_str("<|im_start|>assistant\n<think>\n\n</think>\n\n");
    p
}

#[derive(Default)]
pub struct Chat {
    busy: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Chat {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts writing a reply to the last message.
    pub fn send(&self, app: AppHandle, fixer: Arc<Fixer>, messages: Vec<Message>) -> Result<()> {
        if messages.last().is_none_or(|m| m.role != "user" || m.content.trim().is_empty()) {
            bail!("Type a message first.");
        }
        if messages.iter().any(|m| m.role != "user" && m.role != "assistant") {
            bail!("Start a new chat and try again.");
        }
        if !fixer.has_model() {
            bail!("Download the word fixer in Models to chat with it.");
        }
        if self.busy.swap(true, Ordering::SeqCst) {
            bail!("Wait for the reply to finish.");
        }
        self.stop.store(false, Ordering::SeqCst);
        let (busy, stop) = (self.busy.clone(), self.stop.clone());
        let spawned = std::thread::Builder::new()
            .name("murmur-chat".into())
            .spawn(move || {
                if !fixer.is_loaded() {
                    let _ = app.emit("chat-loading", ());
                }
                let error = match fixer.model(LOAD_WAIT) {
                    Some(llm) => reply(&app, &llm, messages, &stop).err().map(|err| {
                        log::error!("chat failed: {err:#}");
                        "Could not write a reply. Try again.".to_string()
                    }),
                    None => Some("The model did not load. Try again.".to_string()),
                };
                busy.store(false, Ordering::SeqCst);
                let _ = app.emit("chat-done", error);
            });
        if spawned.is_err() {
            self.busy.store(false, Ordering::SeqCst);
            bail!("Could not start the reply.");
        }
        Ok(())
    }

    /// Stops the reply being written; what is there so far stays.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn reply(app: &AppHandle, llm: &Llm, mut messages: Vec<Message>, stop: &AtomicBool) -> Result<()> {
    let mut ids = llm.encode(&prompt(&messages))?;
    while ids.len() > MAX_PROMPT && messages.len() > 1 {
        messages.remove(0);
        ids = llm.encode(&prompt(&messages))?;
    }
    llm.generate(&ids, MAX_REPLY, |text| {
        let _ = app.emit("chat-text", text);
        !stop.load(Ordering::SeqCst)
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> Message {
        Message {
            role: role.into(),
            content: content.into(),
        }
    }

    #[test]
    fn prompt_uses_qwen_markup_with_thinking_off() {
        let p = prompt(&[msg("user", " Hi "), msg("assistant", "Hello."), msg("user", "How are you?")]);
        assert!(p.starts_with("<|im_start|>system\n"));
        assert!(p.contains("<|im_start|>user\nHi<|im_end|>\n<|im_start|>assistant\nHello.<|im_end|>\n"));
        assert!(p.ends_with("<|im_start|>user\nHow are you?<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"));
    }
}

/// Writes one reply with the installed word fixer and prints its speed:
///
///   cargo test --release chat_reply -- --ignored --nocapture
#[cfg(test)]
mod eval {
    use super::*;
    use crate::engine::{default_threads, onnx};

    #[test]
    #[ignore = "needs the downloaded word fixer"]
    fn chat_reply() {
        onnx::use_vendored_runtime();
        let llm = Llm::load(&onnx::installed_model_dir("qwen3-1.7b-int4"), default_threads()).unwrap();
        for question in [
            "In one sentence, what is Monopoly?",
            "Fix the misheard word: I want dance cards to be something else in the monopoly game.",
        ] {
            let ids = llm.encode(&prompt(&[Message { role: "user".into(), content: question.into() }])).unwrap();
            let started = std::time::Instant::now();
            let mut tokens = 0;
            let text = llm.generate(&ids, 120, |_| {
                tokens += 1;
                true
            }).unwrap();
            let secs = started.elapsed().as_secs_f64();
            println!("\nQ: {question}\nA: {text}\n{tokens} tokens in {secs:.1} s, {:.1} tokens/s", tokens as f64 / secs);
        }
    }
}
