//! Text in the focused field on macOS, read through the Accessibility API
//! (AXUIElement), the interface VoiceOver uses. Same job and interface as
//! the UI Automation reader in field.rs: context for the word fixer and
//! watching pasted text for corrections. Nothing read here leaves the
//! process or is stored.
//!
//! Accessibility calls go to the target app and can stall while it is busy,
//! so they all run on one worker thread with a short timeout, and callers
//! never wait on it for long. Secure (password) fields and Murmur's own
//! windows are never read. Without the Accessibility permission every read
//! comes back empty.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::macos::Element;
use crate::text::context::Around;

/// Words read before the cursor for context.
const BEFORE_WORDS: usize = 120;
/// Words read on each side of the cursor while watching for corrections.
const AROUND_WORDS: usize = 150;
/// Words read after the cursor to fit a dictation to what follows it.
const AFTER_WORDS: usize = 3;
/// Longest text kept from one read, in characters from the end.
const MAX_CHARS: usize = 20_000;
/// Characters asked for per word wanted, so the word count fits.
const CHARS_PER_WORD: usize = 16;
/// An app that does not answer within this is given up on.
const AX_TIMEOUT_SECS: f32 = 0.8;
/// How often a watched field is read again.
const WATCH_EVERY: Duration = Duration::from_millis(1000);
/// Text pasted by Murmur needs a moment to show up in the field.
const WATCH_SETTLE: Duration = Duration::from_millis(300);

/// Receives what a watched field holds after a paste.
pub trait Watcher: Send {
    /// The text around the cursor, each time it changes.
    fn snapshot(&mut self, text: &str);
    /// The watch is over: focus moved, time ran out or it was cancelled.
    fn end(self: Box<Self>);
}

enum Job {
    Around(Sender<Option<Around>>),
    Watch {
        watcher: Box<dyn Watcher>,
        cancel: Arc<AtomicBool>,
        until: Instant,
    },
}

fn worker() -> Option<&'static Mutex<Sender<Job>>> {
    static WORKER: OnceLock<Option<Mutex<Sender<Job>>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Job>();
            std::thread::Builder::new()
                .name("murmur-field".into())
                .spawn(move || run(rx))
                .ok()
                .map(|_| Mutex::new(tx))
        })
        .as_ref()
}

fn send(job: Job) -> bool {
    worker()
        .and_then(|w| w.lock().ok())
        .is_some_and(|tx| tx.send(job).is_ok())
}

/// Starts reading the text on each side of the cursor in the focused field.
/// The answer arrives on the returned channel, or never if the app is slow:
/// wait on it with a timeout.
pub fn read_around_cursor() -> Receiver<Option<Around>> {
    let (tx, rx) = mpsc::channel();
    send(Job::Around(tx));
    rx
}

/// Watches the focused field for `for_how_long`, passing what it holds to
/// `watcher` whenever it changes. Set `cancel` to stop early.
pub fn watch(watcher: Box<dyn Watcher>, cancel: Arc<AtomicBool>, for_how_long: Duration) {
    send(Job::Watch {
        watcher,
        cancel,
        until: Instant::now() + for_how_long,
    });
}

fn run(rx: Receiver<Job>) {
    let system = Element::system_wide();
    if let Some(system) = &system {
        system.set_timeout(AX_TIMEOUT_SECS);
    }
    for job in rx {
        let Some(system) = &system else {
            // Answer every request with nothing so callers never wait.
            match job {
                Job::Around(reply) => {
                    let _ = reply.send(None);
                }
                Job::Watch { watcher, .. } => watcher.end(),
            }
            continue;
        };
        match job {
            Job::Around(reply) => {
                let around = focused(system).and_then(|e| {
                    let before = read(&e, BEFORE_WORDS, 0)?;
                    let after = read_after(&e).unwrap_or_default();
                    Some(Around { before, after })
                });
                let _ = reply.send(around);
            }
            Job::Watch {
                mut watcher,
                cancel,
                until,
            } => {
                std::thread::sleep(WATCH_SETTLE);
                if let Some(element) = focused(system) {
                    let mut last = String::new();
                    loop {
                        if cancel.load(Ordering::SeqCst) {
                            break;
                        }
                        let still_focused = focused(system).is_some_and(|f| f == element);
                        if let Some(text) = read(&element, AROUND_WORDS, AROUND_WORDS) {
                            if text != last {
                                watcher.snapshot(&text);
                                last = text;
                            }
                        }
                        if !still_focused || Instant::now() >= until {
                            break;
                        }
                        std::thread::sleep(WATCH_EVERY);
                    }
                }
                watcher.end();
            }
        }
    }
}

/// The focused element, unless it is a secure text field or belongs to one
/// of Murmur's own windows.
fn focused(system: &Element) -> Option<Element> {
    let element = system.element("AXFocusedUIElement")?;
    let secure = |name: &str| {
        element
            .string(name)
            .is_some_and(|v| v == "AXSecureTextField")
    };
    if secure("AXRole") || secure("AXSubrole") {
        return None;
    }
    if element.pid()? as u32 == std::process::id() {
        return None;
    }
    Some(element)
}

/// Up to `before` words before the cursor and `after` words after it, or
/// the end of the field's value when it reports no cursor.
fn read(element: &Element, before: usize, after: usize) -> Option<String> {
    let text = match element.range("AXSelectedTextRange") {
        Some((caret, _)) => around(element, caret, before, after)?,
        None => last_words(&element.string("AXValue")?, before).to_string(),
    };
    let skip = text.chars().count().saturating_sub(MAX_CHARS);
    Some(text.chars().skip(skip).collect())
}

/// A few words after the cursor and any selection, which the dictation
/// replaces.
fn read_after(element: &Element) -> Option<String> {
    let (caret, selected) = element.range("AXSelectedTextRange")?;
    let from = caret + selected;
    let total = element.number("AXNumberOfCharacters").map(|n| n.max(0) as usize);
    let want = match total {
        Some(total) => (AFTER_WORDS * CHARS_PER_WORD).min(total.saturating_sub(from)),
        None => AFTER_WORDS * CHARS_PER_WORD,
    };
    if want == 0 {
        return Some(String::new());
    }
    let text = element.string_for_range(from, want)?;
    Some(first_words(&text, AFTER_WORDS).to_string())
}

fn around(element: &Element, caret: usize, before: usize, after: usize) -> Option<String> {
    let want_before = before * CHARS_PER_WORD;
    let start = caret.saturating_sub(want_before);
    let total = element
        .number("AXNumberOfCharacters")
        .map(|n| n.max(0) as usize);
    let want_after = match total {
        Some(total) => (after * CHARS_PER_WORD).min(total.saturating_sub(caret)),
        None => after * CHARS_PER_WORD,
    };
    match element.string_for_range(start, caret - start + want_after) {
        Some(text) => {
            let split = utf16_to_byte(&text, caret - start);
            let (head, tail) = text.split_at(split);
            Some(format!("{}{}", last_words(head, before), first_words(tail, after)))
        }
        // Fields without ranged reads: cut the whole value at the cursor.
        None => {
            let value = element.string("AXValue")?;
            let split = utf16_to_byte(&value, caret);
            let (head, tail) = value.split_at(split);
            Some(format!("{}{}", last_words(head, before), first_words(tail, after)))
        }
    }
}

/// The byte offset of a UTF-16 offset, clamped to the string.
fn utf16_to_byte(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (i, c) in text.char_indices() {
        if seen >= units {
            return i;
        }
        seen += c.len_utf16();
    }
    text.len()
}

/// The end of `text` holding its last `n` words, with their spacing.
fn last_words(text: &str, n: usize) -> &str {
    if n == 0 {
        return "";
    }
    let mut words = 0;
    let mut in_word = false;
    for (i, c) in text.char_indices().rev() {
        if c.is_whitespace() {
            if in_word {
                words += 1;
                if words == n {
                    return &text[i + c.len_utf8()..];
                }
            }
            in_word = false;
        } else {
            in_word = true;
        }
    }
    text
}

/// The start of `text` holding its first `n` words, with their spacing.
fn first_words(text: &str, n: usize) -> &str {
    if n == 0 {
        return "";
    }
    let mut words = 0;
    let mut in_word = false;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if in_word {
                words += 1;
                if words == n {
                    return &text[..i];
                }
            }
            in_word = false;
        } else {
            in_word = true;
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_windows_keep_spacing() {
        assert_eq!(last_words("one two  three four", 2), "three four");
        assert_eq!(last_words("one two ", 1), "two ");
        assert_eq!(last_words("one two", 5), "one two");
        assert_eq!(last_words("one", 0), "");
        assert_eq!(first_words(" five six seven", 2), " five six");
        assert_eq!(first_words("five", 3), "five");
    }

    #[test]
    fn utf16_offsets_become_byte_offsets() {
        assert_eq!(utf16_to_byte("abc", 1), 1);
        // "é" is one UTF-16 unit and two bytes; the emoji is two units and
        // four bytes.
        assert_eq!(utf16_to_byte("é😀x", 1), 2);
        assert_eq!(utf16_to_byte("é😀x", 3), 6);
        assert_eq!(utf16_to_byte("ab", 9), 2);
    }

    /// Reads whatever field has focus. Put the cursor at the end of some text
    /// in TextEdit first:
    ///
    ///   cargo test --release reads_the_focused_field -- --ignored --nocapture
    #[test]
    #[ignore = "needs a focused text field in another app"]
    fn reads_the_focused_field() {
        let before = read_around_cursor().recv_timeout(Duration::from_secs(3)).map(|a| a.map(|a| a.before));
        println!("before the cursor: {before:?}");
        assert!(matches!(before, Ok(Some(ref t)) if !t.is_empty()));
    }
}
