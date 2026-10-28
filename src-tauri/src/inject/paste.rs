//! Pastes text into the focused app through the clipboard: save the
//! clipboard, write the text, send Ctrl+V (Ctrl+Shift+V in terminals), wait
//! 120 ms, then put the old clipboard back.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use arboard::{Clipboard, ImageData, SetExtWindows};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

use super::focus::FocusedApp;
use crate::hotkey::listener::MURMUR_INPUT_TAG;

/// How long the target app gets to read the clipboard before it is restored.
const PASTE_SETTLE: Duration = Duration::from_millis(120);
/// How long to wait for the user to let go of modifier keys. A held Win key
/// would turn Ctrl+V into a Windows shortcut.
const MODIFIER_WAIT: Duration = Duration::from_secs(1);
/// Virtual key code for V, independent of keyboard layout.
const VK_V: u32 = 0x56;

pub fn paste(text: &str, target: &FocusedApp) -> Result<()> {
    let mut clipboard =
        Clipboard::new().map_err(|e| anyhow!("could not open the clipboard: {e}"))?;
    let saved = Saved::take(&mut clipboard);
    clipboard
        .set()
        .exclude_from_history()
        .text(text)
        .map_err(|e| anyhow!("could not write to the clipboard: {e}"))?;

    wait_for_modifiers_released();
    let sent = send_paste_keys(target.is_terminal());
    std::thread::sleep(PASTE_SETTLE);
    saved.restore(&mut clipboard);
    sent
}

/// What was on the clipboard before, richest format first.
enum Saved {
    Files(Vec<PathBuf>),
    Html { html: String, alt: Option<String> },
    Image(ImageData<'static>),
    Text(String),
    Empty,
}

impl Saved {
    fn take(clipboard: &mut Clipboard) -> Self {
        if let Ok(files) = clipboard.get().file_list() {
            if !files.is_empty() {
                return Saved::Files(files);
            }
        }
        let text = clipboard.get_text().ok();
        if let Ok(html) = clipboard.get().html() {
            return Saved::Html { html, alt: text };
        }
        if let Ok(image) = clipboard.get_image() {
            return Saved::Image(image);
        }
        match text {
            Some(text) => Saved::Text(text),
            None => Saved::Empty,
        }
    }

    fn restore(self, clipboard: &mut Clipboard) {
        let set = clipboard.set().exclude_from_history();
        let result = match self {
            Saved::Files(files) => set.file_list(&files),
            Saved::Html { html, alt } => set.html(html, alt),
            Saved::Image(image) => set.image(image),
            Saved::Text(text) => set.text(text),
            // Anything else could not be read, so it cannot be put back;
            // do not leave the dictation lying on the clipboard instead.
            Saved::Empty => clipboard.clear(),
        };
        if let Err(err) = result {
            log::warn!("could not restore the clipboard: {err}");
        }
    }
}

fn wait_for_modifiers_released() {
    let started = Instant::now();
    while started.elapsed() < MODIFIER_WAIT && any_modifier_down() {
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn any_modifier_down() -> bool {
    [VK_LWIN, VK_RWIN, VK_CONTROL, VK_SHIFT, VK_MENU]
        .iter()
        .any(|&vk: &VIRTUAL_KEY| {
            // SAFETY: GetAsyncKeyState only reads global key state.
            let state = unsafe { GetAsyncKeyState(vk.0 as i32) };
            state as u16 & 0x8000 != 0
        })
}

fn send_paste_keys(terminal: bool) -> Result<()> {
    let settings = Settings {
        // Tagged so Murmur's own hotkey hook ignores these keys.
        windows_dw_extra_info: Some(MURMUR_INPUT_TAG),
        ..Settings::default()
    };
    let mut enigo = Enigo::new(&settings).context("could not start key input")?;
    let mut modifiers = vec![Key::Control];
    if terminal {
        modifiers.push(Key::Shift);
    }

    let mut result = Ok(());
    for m in &modifiers {
        result = result.and(enigo.key(*m, Direction::Press));
    }
    result = result.and(enigo.key(Key::Other(VK_V), Direction::Click));
    // Always release, even if a press failed.
    for m in modifiers.iter().rev() {
        let released = enigo.key(*m, Direction::Release);
        result = result.and(released);
    }
    result.context("could not send the paste keys")
}
