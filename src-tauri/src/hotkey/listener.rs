//! Global keyboard hook (WH_KEYBOARD_LL). Turns raw key events into hotkey
//! events and swallows the hotkey so it never reaches the focused app.
//!
//! The hook proc must stay fast and must never wait on another thread: if it
//! overruns LowLevelHooksTimeout, Windows skips the hook and the key reaches
//! the app unseen. So it only updates small trackers and sends on a channel.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_RWIN, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION,
    KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

pub type Key = VIRTUAL_KEY;

/// Unassigned virtual key. Sent while Win or Alt is held so releasing them
/// does not open the Start menu or focus a menu bar.
const MASK_KEY: Key = VIRTUAL_KEY(0xE8);
/// Tags input Murmur injects so the hook can ignore it.
pub(crate) const MURMUR_INPUT_TAG: usize = 0x4D55_524D; // "MURM"

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    MainDown,
    MainUp,
    ToggleDown,
    Escape,
}

/// A key combination. Each slot is satisfied by any one of its keys, so
/// "Ctrl" matches either Ctrl key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combo {
    slots: Vec<Vec<Key>>,
}

impl Combo {
    pub fn new(slots: Vec<Vec<Key>>) -> Self {
        Self { slots }
    }

    pub fn ctrl_win() -> Self {
        Self::new(vec![vec![VK_LCONTROL, VK_RCONTROL], vec![VK_LWIN, VK_RWIN]])
    }

    pub fn ctrl_shift_space() -> Self {
        Self::new(vec![
            vec![VK_LCONTROL, VK_RCONTROL],
            vec![VK_LSHIFT, VK_RSHIFT],
            vec![VK_SPACE],
        ])
    }

    fn contains(&self, key: Key) -> bool {
        self.slots.iter().any(|slot| slot.contains(&key))
    }

    fn satisfied_by(&self, pressed: &[Key]) -> bool {
        self.slots
            .iter()
            .all(|slot| slot.iter().any(|k| pressed.contains(k)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Down,
    Up,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub swallow: bool,
    pub signal: Option<Signal>,
    /// A Win or Alt key already reached the system; send the mask key.
    pub send_mask: bool,
}

/// Tracks one combo. A key is swallowed from the moment it completes the
/// combo, and its release is swallowed to match. Keys pressed before the combo
/// was complete have already reached the system, so they pass through.
#[derive(Debug)]
pub struct ComboTracker {
    combo: Combo,
    pressed: Vec<Key>,
    swallowed: Vec<Key>,
    active: bool,
}

impl ComboTracker {
    pub fn new(combo: Combo) -> Self {
        Self {
            combo,
            pressed: Vec::new(),
            swallowed: Vec::new(),
            active: false,
        }
    }

    pub fn press(&mut self, key: Key) -> Outcome {
        if !self.combo.contains(key) {
            return Outcome::default();
        }
        // Auto repeat: keep doing whatever was done for the first press.
        if self.pressed.contains(&key) {
            return Outcome {
                swallow: self.swallowed.contains(&key),
                ..Outcome::default()
            };
        }
        self.pressed.push(key);

        if self.active {
            self.swallowed.push(key);
            return Outcome {
                swallow: true,
                ..Outcome::default()
            };
        }
        if !self.combo.satisfied_by(&self.pressed) {
            return Outcome::default();
        }

        self.active = true;
        self.swallowed.push(key);
        let send_mask = self
            .pressed
            .iter()
            .any(|k| !self.swallowed.contains(k) && is_menu_key(*k));
        Outcome {
            swallow: true,
            signal: Some(Signal::Down),
            send_mask,
        }
    }

    pub fn release(&mut self, key: Key) -> Outcome {
        if !self.combo.contains(key) {
            return Outcome::default();
        }
        self.pressed.retain(|k| *k != key);
        let was_swallowed = self.swallowed.contains(&key);
        self.swallowed.retain(|k| *k != key);

        let mut signal = None;
        if self.active && !self.combo.satisfied_by(&self.pressed) {
            self.active = false;
            signal = Some(Signal::Up);
        }
        Outcome {
            swallow: was_swallowed,
            signal,
            send_mask: false,
        }
    }
}

fn is_menu_key(key: Key) -> bool {
    matches!(key, VK_LWIN | VK_RWIN | VK_LMENU | VK_RMENU)
}

/// Changes the running hook from other threads: new hotkeys, pause, and
/// recording a hotkey from Settings. The hook proc only ever try_locks, so a
/// change can never make it wait.
#[derive(Default)]
pub struct HotkeyControl {
    version: AtomicU64,
    pending: Mutex<Option<(Combo, Option<Combo>)>>,
    paused: AtomicBool,
    capturing: AtomicBool,
    capture: Mutex<Option<Sender<Vec<Key>>>>,
}

impl HotkeyControl {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The hook switches to these on the next key event.
    pub fn set_combos(&self, main: Combo, toggle: Option<Combo>) {
        if let Ok(mut p) = self.pending.lock() {
            *p = Some((main, toggle));
            self.version.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    pub fn paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// Swallows every key until a combination is pressed and released, and
    /// returns it. Escape on its own, or nothing within `timeout`, gives None.
    pub fn capture(&self, timeout: Duration) -> Option<Vec<Key>> {
        let (tx, rx) = mpsc::channel();
        *self.capture.lock().ok()? = Some(tx);
        self.capturing.store(true, Ordering::SeqCst);
        let keys = rx.recv_timeout(timeout).ok();
        self.capturing.store(false, Ordering::SeqCst);
        if let Ok(mut c) = self.capture.lock() {
            *c = None;
        }
        keys.filter(|k| !k.is_empty())
    }
}

/// Keys seen while recording a hotkey.
#[derive(Default)]
struct Capture {
    held: Vec<Key>,
    seen: Vec<Key>,
}

/// Everything the hook proc needs. Lives in a thread local on the hook
/// thread, because a hook proc is a plain function with no user data.
struct Hook {
    main: ComboTracker,
    toggle: Option<ComboTracker>,
    escape_held: bool,
    recording: Arc<AtomicBool>,
    tx: Sender<(HotkeyEvent, Instant)>,
    control: Arc<HotkeyControl>,
    version: u64,
    capture: Capture,
}

impl Hook {
    /// Picks up hotkeys set through HotkeyControl.
    fn reload(&mut self) {
        let version = self.control.version.load(Ordering::SeqCst);
        if version == self.version {
            return;
        }
        if let Ok(mut pending) = self.control.pending.try_lock() {
            if let Some((main, toggle)) = pending.take() {
                self.main = ComboTracker::new(main);
                self.toggle = toggle.map(ComboTracker::new);
            }
            self.version = version;
        }
    }

    /// Recording a hotkey: every key is swallowed so nothing types into the
    /// focused app. Returns true (swallow) for keys it handled.
    fn on_capture_key(&mut self, key: Key, down: bool) -> bool {
        let c = &mut self.capture;
        if down {
            if key == VK_ESCAPE && c.seen.is_empty() {
                self.finish_capture(Vec::new());
                return true;
            }
            if !c.held.contains(&key) {
                c.held.push(key);
            }
            if !c.seen.contains(&key) {
                c.seen.push(key);
            }
            return true;
        }
        if !c.held.contains(&key) {
            // Pressed before recording began: let its release through.
            return false;
        }
        c.held.retain(|k| *k != key);
        if c.held.is_empty() && !c.seen.is_empty() {
            let keys = std::mem::take(&mut c.seen);
            self.finish_capture(keys);
        }
        true
    }

    fn finish_capture(&mut self, keys: Vec<Key>) {
        self.capture = Capture::default();
        self.control.capturing.store(false, Ordering::SeqCst);
        if let Ok(c) = self.control.capture.try_lock() {
            if let Some(tx) = c.as_ref() {
                let _ = tx.send(keys);
            }
        }
    }

    /// Returns true if the key must be swallowed.
    fn on_key(&mut self, key: Key, down: bool) -> bool {
        // Stamped here, not when the controller gets to it, so a busy
        // controller cannot make a long hold look like a tap.
        let now = Instant::now();

        self.reload();
        if self.control.capturing.load(Ordering::SeqCst) {
            return self.on_capture_key(key, down);
        }
        if !self.capture.seen.is_empty() || !self.capture.held.is_empty() {
            // Recording timed out mid press.
            self.capture = Capture::default();
        }
        if self.control.paused() && !self.recording.load(Ordering::SeqCst) {
            return false;
        }

        if key == VK_ESCAPE {
            if down {
                if self.escape_held {
                    return true;
                }
                if self.recording.load(Ordering::SeqCst) {
                    self.escape_held = true;
                    let _ = self.tx.send((HotkeyEvent::Escape, now));
                    return true;
                }
                return false;
            }
            if self.escape_held {
                self.escape_held = false;
                return true;
            }
            return false;
        }

        let (main, toggle) = if down {
            (
                self.main.press(key),
                self.toggle
                    .as_mut()
                    .map(|t| t.press(key))
                    .unwrap_or_default(),
            )
        } else {
            (
                self.main.release(key),
                self.toggle
                    .as_mut()
                    .map(|t| t.release(key))
                    .unwrap_or_default(),
            )
        };

        match main.signal {
            Some(Signal::Down) => {
                let _ = self.tx.send((HotkeyEvent::MainDown, now));
            }
            Some(Signal::Up) => {
                let _ = self.tx.send((HotkeyEvent::MainUp, now));
            }
            None => {}
        }
        if toggle.signal == Some(Signal::Down) {
            let _ = self.tx.send((HotkeyEvent::ToggleDown, now));
        }
        if main.send_mask || toggle.send_mask {
            send_mask_key();
        }
        main.swallow || toggle.swallow
    }
}

thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to a
        // KBDLLHOOKSTRUCT that is valid for the duration of this call.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let message = wparam.0 as u32;
        let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let up = message == WM_KEYUP || message == WM_SYSKEYUP;

        if (down || up) && info.dwExtraInfo != MURMUR_INPUT_TAG {
            let key = VIRTUAL_KEY(info.vkCode as u16);
            let swallow = HOOK.with(|cell| {
                // try_borrow: never panic inside a hook proc.
                cell.try_borrow_mut()
                    .ok()
                    .and_then(|mut hook| hook.as_mut().map(|h| h.on_key(key, down)))
                    .unwrap_or(false)
            });
            if swallow {
                return LRESULT(1);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Starts the hook on its own thread with its own message loop. `recording`
/// tells the hook whether Escape should be captured (only while recording).
pub fn spawn(
    main: Combo,
    toggle: Option<Combo>,
    recording: Arc<AtomicBool>,
    tx: Sender<(HotkeyEvent, Instant)>,
    control: Arc<HotkeyControl>,
) {
    let spawned = std::thread::Builder::new()
        .name("murmur-hotkey".into())
        .spawn(move || {
            HOOK.with(|cell| {
                let version = control.version.load(Ordering::SeqCst);
                *cell.borrow_mut() = Some(Hook {
                    main: ComboTracker::new(main),
                    toggle: toggle.map(ComboTracker::new),
                    escape_held: false,
                    recording,
                    tx,
                    control,
                    version,
                    capture: Capture::default(),
                });
            });

            // SAFETY: plain Win32 calls; the hook is removed before the
            // thread (and its thread local state) goes away.
            unsafe {
                let module = match GetModuleHandleW(None) {
                    Ok(module) => module,
                    Err(err) => {
                        log::error!("keyboard hook failed: {err}");
                        return;
                    }
                };
                let hook = match SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(hook_proc),
                    Some(module.into()),
                    0,
                ) {
                    Ok(hook) => hook,
                    Err(err) => {
                        log::error!("keyboard hook failed: {err}");
                        return;
                    }
                };
                log::info!("keyboard hook installed");

                // The hook proc runs inside this loop. Nothing posts messages
                // to this thread, so GetMessageW only returns on WM_QUIT or error.
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).0 > 0 {}

                let _ = UnhookWindowsHookEx(hook);
            }
        });

    if let Err(err) = spawned {
        log::error!("could not start hotkey thread: {err}");
    }
}

/// Injected from another thread: the hook must return quickly and must not
/// inject input from inside its own proc.
fn send_mask_key() {
    std::thread::spawn(|| {
        let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: MASK_KEY,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: MURMUR_INPUT_TAG,
                },
            },
        };
        let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
        // SAFETY: inputs is a valid slice of fully initialised INPUTs.
        unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_A, VK_C, VK_D};

    fn ctrl_win() -> ComboTracker {
        ComboTracker::new(Combo::ctrl_win())
    }

    fn passed() -> Outcome {
        Outcome::default()
    }

    fn swallowed() -> Outcome {
        Outcome {
            swallow: true,
            ..Outcome::default()
        }
    }

    #[test]
    fn unrelated_keys_pass_through() {
        let mut t = ctrl_win();
        assert_eq!(t.press(VK_A), passed());
        assert_eq!(t.release(VK_A), passed());
    }

    #[test]
    fn ctrl_then_win_swallows_win_only() {
        let mut t = ctrl_win();
        assert_eq!(t.press(VK_LCONTROL), passed());
        assert_eq!(
            t.press(VK_LWIN),
            Outcome {
                swallow: true,
                signal: Some(Signal::Down),
                send_mask: false
            }
        );
        assert_eq!(
            t.release(VK_LWIN),
            Outcome {
                swallow: true,
                signal: Some(Signal::Up),
                send_mask: false
            }
        );
        assert_eq!(t.release(VK_LCONTROL), passed());
    }

    #[test]
    fn win_then_ctrl_sends_mask_so_start_menu_stays_shut() {
        let mut t = ctrl_win();
        assert_eq!(t.press(VK_LWIN), passed());
        assert_eq!(
            t.press(VK_LCONTROL),
            Outcome {
                swallow: true,
                signal: Some(Signal::Down),
                send_mask: true
            }
        );
        // Win went down to the system, so its release must go too.
        assert_eq!(
            t.release(VK_LWIN),
            Outcome {
                swallow: false,
                signal: Some(Signal::Up),
                send_mask: false
            }
        );
        assert_eq!(t.release(VK_LCONTROL), swallowed());
    }

    #[test]
    fn right_side_modifiers_work() {
        let mut t = ctrl_win();
        t.press(VK_RCONTROL);
        assert_eq!(t.press(VK_RWIN).signal, Some(Signal::Down));
        assert_eq!(t.release(VK_RCONTROL).signal, Some(Signal::Up));
    }

    #[test]
    fn auto_repeat_matches_first_press_and_signals_once() {
        let mut t = ctrl_win();
        assert_eq!(t.press(VK_LCONTROL), passed());
        assert_eq!(t.press(VK_LCONTROL), passed());
        assert_eq!(t.press(VK_LWIN).signal, Some(Signal::Down));
        assert_eq!(t.press(VK_LWIN), swallowed());
        assert_eq!(t.press(VK_LCONTROL), passed());
    }

    #[test]
    fn extra_slot_key_while_active_is_swallowed_and_does_not_end_combo() {
        let mut t = ctrl_win();
        t.press(VK_LCONTROL);
        t.press(VK_LWIN);
        assert_eq!(t.press(VK_RCONTROL), swallowed());
        // Right Ctrl still satisfies the Ctrl slot, so the combo holds.
        assert_eq!(t.release(VK_LCONTROL), passed());
        assert_eq!(
            t.release(VK_RCONTROL),
            Outcome {
                swallow: true,
                signal: Some(Signal::Up),
                send_mask: false
            }
        );
        assert_eq!(t.release(VK_LWIN), swallowed());
    }

    #[test]
    fn combo_can_fire_again_after_release() {
        let mut t = ctrl_win();
        for _ in 0..2 {
            t.press(VK_LCONTROL);
            assert_eq!(t.press(VK_LWIN).signal, Some(Signal::Down));
            assert_eq!(t.release(VK_LWIN).signal, Some(Signal::Up));
            t.release(VK_LCONTROL);
        }
    }

    #[test]
    fn half_combo_never_signals() {
        let mut t = ctrl_win();
        assert_eq!(t.press(VK_LCONTROL), passed());
        assert_eq!(t.press(VK_C), passed());
        assert_eq!(t.release(VK_C), passed());
        assert_eq!(t.release(VK_LCONTROL), passed());
    }

    #[test]
    fn three_key_combo_fires_on_last_key() {
        let mut t = ComboTracker::new(Combo::ctrl_shift_space());
        assert_eq!(t.press(VK_LCONTROL), passed());
        assert_eq!(t.press(VK_LSHIFT), passed());
        assert_eq!(t.press(VK_SPACE).signal, Some(Signal::Down));
        assert_eq!(t.release(VK_SPACE).signal, Some(Signal::Up));
    }

    #[test]
    fn alt_counts_as_menu_key() {
        let mut t = ComboTracker::new(Combo::new(vec![vec![VK_LMENU], vec![VK_D]]));
        t.press(VK_LMENU);
        assert!(t.press(VK_D).send_mask);
    }

    type TestHook = (
        Hook,
        Arc<AtomicBool>,
        std::sync::mpsc::Receiver<(HotkeyEvent, Instant)>,
    );

    fn hook() -> TestHook {
        let (tx, rx) = std::sync::mpsc::channel();
        let recording = Arc::new(AtomicBool::new(false));
        let hook = Hook {
            main: ComboTracker::new(Combo::ctrl_win()),
            toggle: None,
            escape_held: false,
            recording: recording.clone(),
            tx,
            control: HotkeyControl::new(),
            version: 0,
            capture: Capture::default(),
        };
        (hook, recording, rx)
    }

    #[test]
    fn paused_hook_lets_the_hotkey_through() {
        let (mut hook, _, rx) = hook();
        hook.control.set_paused(true);
        assert!(!hook.on_key(VK_LCONTROL, true));
        assert!(!hook.on_key(VK_LWIN, true));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn new_combos_apply_on_the_next_key() {
        let (mut hook, _, rx) = hook();
        hook.control.set_combos(Combo::ctrl_shift_space(), None);
        assert!(!hook.on_key(VK_LCONTROL, true));
        assert!(!hook.on_key(VK_LWIN, true));
        hook.on_key(VK_LWIN, false);
        assert!(!hook.on_key(VK_LSHIFT, true));
        assert!(hook.on_key(VK_SPACE, true));
        assert_eq!(rx.try_recv().unwrap().0, HotkeyEvent::MainDown);
    }

    #[test]
    fn capture_swallows_keys_and_returns_the_combination() {
        let (mut hook, _, _rx) = hook();
        let control = hook.control.clone();
        let waiter = std::thread::spawn(move || control.capture(Duration::from_secs(5)));
        while !hook.control.capturing.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
        // Released before recording began: passes through.
        assert!(!hook.on_key(VK_RSHIFT, false));
        assert!(hook.on_key(VK_LMENU, true));
        assert!(hook.on_key(VIRTUAL_KEY(0x78), true)); // F9
        assert!(hook.on_key(VIRTUAL_KEY(0x78), false));
        assert!(hook.on_key(VK_LMENU, false));
        assert_eq!(waiter.join().unwrap(), Some(vec![VK_LMENU, VIRTUAL_KEY(0x78)]));
        // Back to normal: Ctrl + Win fires again.
        assert!(!hook.on_key(VK_LCONTROL, true));
        assert!(hook.on_key(VK_LWIN, true));
    }

    #[test]
    fn escape_cancels_a_capture() {
        let (mut hook, _, _rx) = hook();
        let control = hook.control.clone();
        let waiter = std::thread::spawn(move || control.capture(Duration::from_secs(5)));
        while !hook.control.capturing.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
        assert!(hook.on_key(VK_ESCAPE, true));
        assert_eq!(waiter.join().unwrap(), None);
    }

    #[test]
    fn escape_passes_through_when_not_recording() {
        let (mut h, _, rx) = hook();
        assert!(!h.on_key(VK_ESCAPE, true));
        assert!(!h.on_key(VK_ESCAPE, false));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn escape_is_swallowed_and_sent_once_while_recording() {
        let (mut h, recording, rx) = hook();
        recording.store(true, Ordering::SeqCst);
        assert!(h.on_key(VK_ESCAPE, true));
        assert!(h.on_key(VK_ESCAPE, true)); // auto repeat
                                            // Recording ended in between; the release still matches the press.
        recording.store(false, Ordering::SeqCst);
        assert!(h.on_key(VK_ESCAPE, false));
        let events: Vec<_> = rx.try_iter().map(|(e, _)| e).collect();
        assert_eq!(events, vec![HotkeyEvent::Escape]);
    }

    #[test]
    fn hook_sends_main_down_and_up() {
        let (mut h, _, rx) = hook();
        assert!(!h.on_key(VK_LCONTROL, true));
        assert!(h.on_key(VK_LWIN, true));
        assert!(h.on_key(VK_LWIN, false));
        assert!(!h.on_key(VK_LCONTROL, false));
        let events: Vec<_> = rx.try_iter().map(|(e, _)| e).collect();
        assert_eq!(events, vec![HotkeyEvent::MainDown, HotkeyEvent::MainUp]);
    }
}
