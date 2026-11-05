//! macOS counterpart of the WH_KEYBOARD_LL hook: a Quartz event tap that
//! sees key down, key up and modifier changes for the whole session, hands
//! them to the same `Hook` the Windows hook proc uses, and drops the events
//! it swallows. Each event is timestamped inside the tap callback.
//!
//! An active tap needs the Accessibility permission. Until the user grants
//! it, the tap cannot be created; the thread asks once (macOS shows its own
//! dialog) and then tries again every couple of seconds.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};

use core_foundation_sys::mach_port::CFMachPortCreateRunLoopSource;
use core_foundation_sys::runloop::{
    kCFRunLoopCommonModes, CFRunLoopAddSource, CFRunLoopGetCurrent, CFRunLoopRun,
};

use super::super::vk_mac::{from_mac, modifier_flag};
use super::{Combo, ComboTracker, Hook, HotkeyControl, HotkeyEvent, HOOK, MURMUR_INPUT_TAG};
use crate::macos::{
    accessibility_trusted, CFMachPortRef, CGEventGetFlags, CGEventGetIntegerValueField,
    CGEventRef, CGEventTapCreate, CGEventTapEnable, CGEventTapProxy, EVENT_FLAGS_CHANGED,
    EVENT_KEY_DOWN, EVENT_KEY_UP, EVENT_TAP_DISABLED_BY_TIMEOUT,
    EVENT_TAP_DISABLED_BY_USER_INPUT, FIELD_EVENT_SOURCE_USER_DATA, FIELD_KEYCODE,
    FLAG_ALTERNATE, FLAG_COMMAND, FLAG_CONTROL, FLAG_SHIFT, HEAD_INSERT_EVENT_TAP,
    SESSION_EVENT_TAP, TAP_OPTION_DEFAULT,
};

/// How often to try again while the Accessibility permission is missing.
const RETRY_EVERY: Duration = Duration::from_secs(2);

/// The tap, so the callback can switch it back on when macOS disables it
/// (it does that when a callback runs too long).
static TAP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Every device dependent modifier bit (left and right of each modifier).
const DEVICE_BITS: u64 = 0x0000_207F;

/// Whether the modifier with this key code is down after a flags changed
/// event. Uses the left and right bits when the keyboard reports them, and
/// the modifier's shared bit when it does not (some virtual keyboards).
fn modifier_down(code: u16, flags: u64) -> Option<bool> {
    let bit = modifier_flag(code)?;
    if flags & DEVICE_BITS != 0 {
        return Some(flags & bit != 0);
    }
    let shared = match code {
        0x3B | 0x3E => FLAG_CONTROL,
        0x38 | 0x3C => FLAG_SHIFT,
        0x3A | 0x3D => FLAG_ALTERNATE,
        _ => FLAG_COMMAND,
    };
    Some(flags & shared != 0)
}

unsafe extern "C" fn tap_callback(
    _proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    _user_info: *mut c_void,
) -> CGEventRef {
    if event_type == EVENT_TAP_DISABLED_BY_TIMEOUT || event_type == EVENT_TAP_DISABLED_BY_USER_INPUT
    {
        let tap = TAP.load(Ordering::SeqCst);
        if !tap.is_null() {
            // SAFETY: the tap lives for the life of the process.
            unsafe { CGEventTapEnable(tap, true) };
        }
        return event;
    }
    if event.is_null() {
        return event;
    }
    // SAFETY: `event` is the live event this callback was given.
    let (tag, code, flags) = unsafe {
        (
            CGEventGetIntegerValueField(event, FIELD_EVENT_SOURCE_USER_DATA),
            CGEventGetIntegerValueField(event, FIELD_KEYCODE) as u16,
            CGEventGetFlags(event),
        )
    };
    // Keys Murmur posts itself (the paste) are tagged; leave them alone.
    if tag == MURMUR_INPUT_TAG as i64 {
        return event;
    }
    let down = match event_type {
        EVENT_KEY_DOWN => true,
        EVENT_KEY_UP => false,
        EVENT_FLAGS_CHANGED => match modifier_down(code, flags) {
            Some(down) => down,
            None => return event,
        },
        _ => return event,
    };
    let Some(key) = from_mac(code) else {
        return event;
    };
    let swallow = HOOK.with(|cell| {
        // try_borrow: never panic inside the tap.
        cell.try_borrow_mut()
            .ok()
            .and_then(|mut hook| hook.as_mut().map(|h| h.on_key(key, down)))
            .unwrap_or(false)
    });
    if swallow {
        std::ptr::null_mut()
    } else {
        event
    }
}

fn create_tap() -> CFMachPortRef {
    let mask = (1u64 << EVENT_KEY_DOWN) | (1u64 << EVENT_KEY_UP) | (1u64 << EVENT_FLAGS_CHANGED);
    // SAFETY: plain tap creation; the callback has the right signature and
    // takes no user data. Returns null without the Accessibility permission.
    unsafe {
        CGEventTapCreate(
            SESSION_EVENT_TAP,
            HEAD_INSERT_EVENT_TAP,
            TAP_OPTION_DEFAULT,
            mask,
            tap_callback,
            std::ptr::null_mut(),
        )
    }
}

/// Starts the tap on its own thread with its own run loop. `recording`
/// tells it whether Escape should be captured (only while recording).
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
                });
            });

            if !accessibility_trusted(true) {
                log::warn!("waiting for the Accessibility permission for the hotkey");
            }
            let tap = loop {
                let tap = create_tap();
                if !tap.is_null() {
                    break tap;
                }
                std::thread::sleep(RETRY_EVERY);
            };
            TAP.store(tap, Ordering::SeqCst);

            // SAFETY: the run loop source and tap live until the process
            // exits; this thread's run loop is the one the callback runs on.
            unsafe {
                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap as _, 0);
                if source.is_null() {
                    log::error!("keyboard tap failed: no run loop source");
                    return;
                }
                CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
                CGEventTapEnable(tap, true);
                log::info!("keyboard tap installed");
                CFRunLoopRun();
            }
        });

    if let Err(err) = spawned {
        log::error!("could not start hotkey thread: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_state_from_flags() {
        // Left Command down: device bit 0x08 plus the shared Command bit.
        assert_eq!(modifier_down(0x37, 0x0010_0108), Some(true));
        // Right Command still up while the left one is down.
        assert_eq!(modifier_down(0x36, 0x0010_0108), Some(false));
        // Released: only the non coalesced bit is left.
        assert_eq!(modifier_down(0x37, 0x0000_0100), Some(false));
        // No device bits reported: fall back to the shared bit.
        assert_eq!(modifier_down(0x3B, FLAG_CONTROL), Some(true));
        assert_eq!(modifier_down(0x31, FLAG_CONTROL), None);
    }
}
