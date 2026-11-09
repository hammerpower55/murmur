//! Global keyboard hook (WH_KEYBOARD_LL). Turns raw key events into hotkey
//! events and swallows the hotkey so it never reaches the focused app.
//!
//! The hook proc must stay fast and must never wait on another thread: if it
//! overruns LowLevelHooksTimeout, Windows skips the hook and the key reaches
//! the app unseen. So it only updates small trackers and sends on a channel.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[cfg(windows)]
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_RWIN, VK_SPACE,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION,
    KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

#[cfg(target_os = "macos")]
pub use super::vk_mac::{
    VIRTUAL_KEY, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL, VK_RMENU,
    VK_RSHIFT, VK_RWIN, VK_SPACE,
};

/// On macOS the event tap (listener_mac.rs) takes the place of the hook.
#[cfg(target_os = "macos")]
#[path = "listener_mac.rs"]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::spawn;

pub type Key = VIRTUAL_KEY;

/// Unassigned virtual key. Sent while Win or Alt is held so releasing them
/// does not open the Start menu or focus a menu bar.
#[cfg(windows)]
const MASK_KEY: Key = VIRTUAL_KEY(0xE8);
/// Tags input Murmur injects so the hook can ignore it.
pub(crate) const MURMUR_INPUT_TAG: usize = 0x4D55_524D; // "MURM"

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    MainDown,
    MainUp,
    ToggleDown,
    /// The screenplay hotkey: starts or stops a dictation that becomes a
    /// screenplay.
    ScriptDown,
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
/// standing aside while Settings records a hotkey. The hook proc only ever
/// try_locks, so a change can never make it wait.
/// Every hotkey the hook listens for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotkeys {
    pub main: Combo,
    /// Hands free: starts or stops, never push to talk.
    pub toggle: Option<Combo>,
    /// Screenplay mode, hands free like the toggle.
    pub script: Option<Combo>,
}

#[derive(Default)]
pub struct HotkeyControl {
    version: AtomicU64,
    pending: Mutex<Option<Hotkeys>>,
    /// "Pause hotkeys" in the tray.
    paused: AtomicBool,
    /// Settings is recording a hotkey; every key passes through to it.
    suspended: AtomicBool,
}

impl HotkeyControl {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The hook switches to these on the next key event.
    pub fn set_combos(&self, hotkeys: Hotkeys) {
        if let Ok(mut p) = self.pending.lock() {
            *p = Some(hotkeys);
            self.version.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    pub fn paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// While Settings records a hotkey the hook lets every key through, so
    /// pressing the current hotkey records it instead of starting a
    /// dictation. Recording itself happens in the page: keys typed into
    /// Murmur's own window go to WebView2 and never reach a low level hook.
    pub fn set_suspended(&self, suspended: bool) {
        self.suspended.store(suspended, Ordering::SeqCst);
    }
}

/// Everything the hook proc needs. Lives in a thread local on the hook
/// thread, because a hook proc is a plain function with no user data.
struct Hook {
    main: ComboTracker,
    toggle: Option<ComboTracker>,
    script: Option<ComboTracker>,
    escape_held: bool,
    recording: Arc<AtomicBool>,
    tx: Sender<(HotkeyEvent, Instant)>,
    control: Arc<HotkeyControl>,
    version: u64,
}

impl Hook {
    /// Picks up hotkeys set through HotkeyControl.
    fn reload(&mut self) {
        let version = self.control.version.load(Ordering::SeqCst);
        if version == self.version {
            return;
        }
        if let Ok(mut pending) = self.control.pending.try_lock() {
            if let Some(hotkeys) = pending.take() {
                self.main = ComboTracker::new(hotkeys.main);
                self.toggle = hotkeys.toggle.map(ComboTracker::new);
                self.script = hotkeys.script.map(ComboTracker::new);
            }
            self.version = version;
        }
    }

    /// Returns true if the key must be swallowed.
    fn on_key(&mut self, key: Key, down: bool) -> bool {
        // Stamped here, not when the controller gets to it, so a busy
        // controller cannot make a long hold look like a tap.
        let now = Instant::now();

        self.reload();
        let standing_aside = self.control.paused() || self.control.suspended.load(Ordering::SeqCst);
        if standing_aside && !self.recording.load(Ordering::SeqCst) {
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

        let step = |t: &mut ComboTracker| if down { t.press(key) } else { t.release(key) };
        let main = step(&mut self.main);
        let toggle = self.toggle.as_mut().map(step).unwrap_or_default();
        let script = self.script.as_mut().map(step).unwrap_or_default();

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
        if script.signal == Some(Signal::Down) {
            let _ = self.tx.send((HotkeyEvent::ScriptDown, now));
        }
        if main.send_mask || toggle.send_mask || script.send_mask {
            send_mask_key();
        }
        main.swallow || toggle.swallow || script.swallow
    }
}

// On macOS this is the event tap's state, on its own thread like the hook.
thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

#[cfg(windows)]
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
#[cfg(windows)]
pub fn spawn(
    hotkeys: Hotkeys,
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
                    main: ComboTracker::new(hotkeys.main),
                    toggle: hotkeys.toggle.map(ComboTracker::new),
                    script: hotkeys.script.map(ComboTracker::new),
                    escape_held: false,
                    recording,
                    tx,
                    control,
                    version,
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
#[cfg(windows)]
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

/// macOS has no Start menu, and Command or Option pressed alone does
/// nothing, so there is nothing to mask.
#[cfg(target_os = "macos")]
fn send_mask_key() {}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    use super::super::vk_mac::{VK_A, VK_C, VK_D};
    #[cfg(windows)]
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
            script: None,
            escape_held: false,
            recording: recording.clone(),
            tx,
            control: HotkeyControl::new(),
            version: 0,
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
    fn the_screenplay_hotkey_sends_its_own_event() {
        let (mut hook, _, rx) = hook();
        hook.control.set_combos(Hotkeys {
            main: Combo::ctrl_win(),
            toggle: None,
            script: Some(Combo::new(vec![vec![VK_LCONTROL], vec![VK_LMENU], vec![VK_SPACE]])),
        });
        hook.on_key(VK_LCONTROL, true);
        hook.on_key(VK_LMENU, true);
        assert!(hook.on_key(VK_SPACE, true));
        assert_eq!(rx.try_recv().unwrap().0, HotkeyEvent::ScriptDown);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_hotkey_containing_the_main_one_fires_after_it() {
        let shift_too = || Combo::new(vec![vec![VK_LCONTROL], vec![VK_LSHIFT], vec![VK_LWIN]]);
        // Win last: both complete on the same key, the main one first.
        let (mut hook, _, rx) = hook();
        hook.control.set_combos(Hotkeys { main: Combo::ctrl_win(), toggle: None, script: Some(shift_too()) });
        hook.on_key(VK_LCONTROL, true);
        hook.on_key(VK_LSHIFT, true);
        assert!(hook.on_key(VK_LWIN, true));
        let events: Vec<_> = rx.try_iter().map(|(e, _)| e).collect();
        assert_eq!(events, vec![HotkeyEvent::MainDown, HotkeyEvent::ScriptDown]);
    }

    #[test]
    fn new_combos_apply_on_the_next_key() {
        let (mut hook, _, rx) = hook();
        hook.control.set_combos(Hotkeys {
            main: Combo::ctrl_shift_space(),
            toggle: None,
            script: None,
        });
        assert!(!hook.on_key(VK_LCONTROL, true));
        assert!(!hook.on_key(VK_LWIN, true));
        hook.on_key(VK_LWIN, false);
        assert!(!hook.on_key(VK_LSHIFT, true));
        assert!(hook.on_key(VK_SPACE, true));
        assert_eq!(rx.try_recv().unwrap().0, HotkeyEvent::MainDown);
    }

    #[test]
    fn suspended_hook_lets_every_key_reach_settings() {
        let (mut hook, _, rx) = hook();
        hook.control.set_suspended(true);
        // Pressing the current hotkey records it instead of dictating.
        assert!(!hook.on_key(VK_LCONTROL, true));
        assert!(!hook.on_key(VK_LWIN, true));
        assert!(rx.try_recv().is_err());
        hook.on_key(VK_LWIN, false);
        hook.on_key(VK_LCONTROL, false);
        hook.control.set_suspended(false);
        assert!(!hook.on_key(VK_LCONTROL, true));
        assert!(hook.on_key(VK_LWIN, true));
        assert_eq!(rx.try_recv().unwrap().0, HotkeyEvent::MainDown);
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
