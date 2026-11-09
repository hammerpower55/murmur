//! Text in the focused field, read through UI Automation, the interface
//! screen readers use. Used for context (the words before the cursor help
//! the fix-up model) and to notice corrections the user makes to pasted text
//! (text/learn.rs). Nothing read here leaves the process or is stored.
//!
//! UI Automation calls go to the target app and can stall while it is busy,
//! so they all run on one worker thread with short timeouts, and callers
//! never wait on it for long. Password fields and Murmur's own windows are
//! never read.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::text::context::Around;
use windows::core::Interface;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomation2, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationTextRange, IUIAutomationValuePattern, TextPatternRangeEndpoint_End,
    TextPatternRangeEndpoint_Start, TextUnit_Word, UIA_TextPatternId, UIA_ValuePatternId,
};

/// Words read before the cursor for context.
const BEFORE_WORDS: i32 = 120;
/// Words read on each side of the cursor while watching for corrections.
const AROUND_WORDS: i32 = 150;
/// Words read after the cursor to fit a dictation to what follows it.
const AFTER_WORDS: i32 = 3;
/// Longest text kept from one read, in characters from the end.
const MAX_CHARS: usize = 20_000;
/// UI Automation gives up on an app that does not answer within this.
const UIA_TIMEOUT_MS: u32 = 800;
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
    // SAFETY: COM is initialised once for this thread, which owns every
    // UI Automation object it creates.
    let automation = unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
    };
    let automation = match automation {
        Ok(a) => a,
        Err(err) => {
            log::warn!("UI Automation is not available: {err}");
            // Answer every request with nothing so callers never wait.
            for job in rx {
                match job {
                    Job::Around(reply) => {
                        let _ = reply.send(None);
                    }
                    Job::Watch { watcher, .. } => watcher.end(),
                }
            }
            return;
        }
    };
    if let Ok(a2) = automation.cast::<IUIAutomation2>() {
        // SAFETY: plain property setters on a live COM object.
        unsafe {
            let _ = a2.SetConnectionTimeout(UIA_TIMEOUT_MS);
            let _ = a2.SetTransactionTimeout(UIA_TIMEOUT_MS);
        }
    }
    for job in rx {
        match job {
            Job::Around(reply) => {
                let around = focused(&automation).and_then(|e| {
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
                if let Some(element) = focused(&automation) {
                    let mut last = String::new();
                    loop {
                        if cancel.load(Ordering::SeqCst) {
                            break;
                        }
                        // SAFETY: comparing two live elements.
                        let still_focused = focused(&automation).is_some_and(|f| unsafe {
                            automation.CompareElements(&f, &element).is_ok_and(|b| b.as_bool())
                        });
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

/// The focused element, unless it is a password field or one of Murmur's
/// own windows.
fn focused(automation: &IUIAutomation) -> Option<IUIAutomationElement> {
    // SAFETY: UI Automation queries on live COM objects owned by this thread.
    unsafe {
        let element = automation.GetFocusedElement().ok()?;
        if element.CurrentIsPassword().is_ok_and(|b| b.as_bool()) {
            return None;
        }
        if element.CurrentProcessId().ok()? as u32 == std::process::id() {
            return None;
        }
        Some(element)
    }
}

/// Up to `before` words before the cursor and `after` words after it, or
/// the field's whole value when it has no text pattern.
fn read(element: &IUIAutomationElement, before: i32, after: i32) -> Option<String> {
    // SAFETY: UI Automation queries on a live element owned by this thread.
    let text = unsafe {
        match element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
            Ok(pattern) => around_caret(&pattern, before, after),
            Err(_) => element
                .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .ok()
                .and_then(|v| v.CurrentValue().ok())
                .map(|v| v.to_string()),
        }
    }?;
    let skip = text.chars().count().saturating_sub(MAX_CHARS);
    Some(text.chars().skip(skip).collect())
}

/// A few words after the cursor (after any selection, which the dictation
/// replaces). Fields with no text pattern report nothing here.
fn read_after(element: &IUIAutomationElement) -> Option<String> {
    // SAFETY: UI Automation queries on a live element owned by this thread.
    unsafe {
        let pattern = element
            .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
            .ok()?;
        let selection = pattern.GetSelection().ok()?;
        if selection.Length().ok()? == 0 {
            return None;
        }
        let caret = selection.GetElement(0).ok()?;
        let range = caret.Clone().ok()?;
        range
            .MoveEndpointByRange(TextPatternRangeEndpoint_Start, &caret, TextPatternRangeEndpoint_End)
            .ok()?;
        let _ = range.MoveEndpointByUnit(TextPatternRangeEndpoint_End, TextUnit_Word, AFTER_WORDS);
        range.GetText(-1).ok().map(|t| t.to_string())
    }
}

unsafe fn around_caret(pattern: &IUIAutomationTextPattern, before: i32, after: i32) -> Option<String> {
    unsafe {
        let selection = pattern.GetSelection().ok()?;
        let caret: IUIAutomationTextRange = if selection.Length().ok()? > 0 {
            selection.GetElement(0).ok()?
        } else {
            // No caret: read the end of the document.
            let doc = pattern.DocumentRange().ok()?;
            let end = doc.Clone().ok()?;
            end.MoveEndpointByRange(TextPatternRangeEndpoint_Start, &doc, TextPatternRangeEndpoint_End)
                .ok()?;
            end
        };
        let range = caret.Clone().ok()?;
        range
            .MoveEndpointByRange(TextPatternRangeEndpoint_End, &caret, TextPatternRangeEndpoint_Start)
            .ok()?;
        let _ = range.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Word, -before);
        if after > 0 {
            let _ = range.MoveEndpointByUnit(TextPatternRangeEndpoint_End, TextUnit_Word, after);
        }
        range.GetText(-1).ok().map(|t| t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads whatever field has focus. Put the cursor at the end of some text
    /// in Notepad first:
    ///
    ///   cargo test --release reads_the_focused_field -- --ignored --nocapture
    #[test]
    #[ignore = "needs a focused text field in another app"]
    fn reads_the_focused_field() {
        let around = read_around_cursor().recv_timeout(Duration::from_secs(3));
        println!("around the cursor: {around:?}");
        assert!(matches!(around, Ok(Some(ref a)) if !a.before.is_empty()));
    }

    struct Collect {
        pasted: &'static str,
        found: std::sync::mpsc::Sender<Option<Vec<crate::text::dictionary::DictEntry>>>,
        latest: Option<Vec<crate::text::dictionary::DictEntry>>,
    }

    impl Watcher for Collect {
        fn snapshot(&mut self, text: &str) {
            println!("snapshot: {text:?}");
            if let Some(found) = crate::text::learn::corrections(self.pasted, text) {
                self.latest = Some(found);
            }
        }
        fn end(self: Box<Self>) {
            let _ = self.found.send(self.latest);
        }
    }

    /// Watches the focused field while someone fixes "graph fauna" to
    /// "Grafana" in it (scripted with SendKeys in the developer's check).
    #[test]
    #[ignore = "needs a focused text field and someone editing it"]
    fn learns_from_an_edit_in_the_focused_field() {
        let (tx, rx) = mpsc::channel();
        let watcher = Collect {
            pasted: "Ask Ayesha about the graph fauna dashboards today.",
            found: tx,
            latest: None,
        };
        watch(Box::new(watcher), Arc::new(AtomicBool::new(false)), Duration::from_secs(8));
        let found = rx.recv_timeout(Duration::from_secs(15)).unwrap();
        println!("learned: {found:?}");
        let found = found.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].spoken.as_str(), found[0].written.as_str()), ("graph fauna", "Grafana"));
    }
}
