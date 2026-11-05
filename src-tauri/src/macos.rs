//! macOS system calls shared by the hotkey tap, pasting, focus detection and
//! field reading: the Accessibility API (AXUIElement) and Quartz events.
//!
//! The event tap, posting keys and reading other apps' windows all need the
//! Accessibility permission (System Settings > Privacy & Security >
//! Accessibility). Without it these calls fail quietly and return nothing.

use std::ffi::c_void;

use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use core_foundation_sys::base::{CFIndex, CFRange};
use core_foundation_sys::dictionary::CFDictionaryRef;

pub type AXUIElementRef = CFTypeRef;
pub type AXValueRef = CFTypeRef;
pub type AXError = i32;
pub type CGEventRef = *mut c_void;
pub type CGEventTapProxy = *mut c_void;
pub type CGEventTapCallBack =
    unsafe extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;
pub type CFMachPortRef = *mut c_void;

const AX_SUCCESS: AXError = 0;
const AX_VALUE_CF_RANGE_TYPE: u32 = 4;

// CGEventTapLocation, CGEventTapPlacement, CGEventTapOptions.
pub const SESSION_EVENT_TAP: u32 = 1;
pub const HEAD_INSERT_EVENT_TAP: u32 = 0;
pub const TAP_OPTION_DEFAULT: u32 = 0;
// CGEventType.
pub const EVENT_KEY_DOWN: u32 = 10;
pub const EVENT_KEY_UP: u32 = 11;
pub const EVENT_FLAGS_CHANGED: u32 = 12;
pub const EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
pub const EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
// CGEventField.
pub const FIELD_KEYCODE: u32 = 9;
pub const FIELD_EVENT_SOURCE_USER_DATA: u32 = 42;
// CGEventFlags.
pub const FLAG_SHIFT: u64 = 0x0002_0000;
pub const FLAG_CONTROL: u64 = 0x0004_0000;
pub const FLAG_ALTERNATE: u64 = 0x0008_0000;
pub const FLAG_COMMAND: u64 = 0x0010_0000;
/// kCGEventSourceStateCombinedSessionState.
const COMBINED_SESSION_STATE: i32 = 0;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCreateApplication(pid: libc::pid_t) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementCopyParameterizedAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        parameter: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut libc::pid_t) -> AXError;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, seconds: f32) -> AXError;
    fn AXValueCreate(value_type: u32, value: *const c_void) -> AXValueRef;
    fn AXValueGetValue(value: AXValueRef, value_type: u32, out: *mut c_void) -> bool;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    pub fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    pub fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGEventSourceFlagsState(state: i32) -> u64;
}

/// Whether Murmur has the Accessibility permission. With `prompt`, macOS
/// shows its own dialog pointing the user at System Settings.
pub fn accessibility_trusted(prompt: bool) -> bool {
    // SAFETY: kAXTrustedCheckOptionPrompt is a constant CFString owned by
    // the framework; the dictionary lives until the call returns.
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let value = if prompt {
            CFBoolean::true_value()
        } else {
            CFBoolean::false_value()
        };
        let options = CFDictionary::from_CFType_pairs(&[(key, value)]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
    }
}

/// Modifier keys the user is holding right now (Command, Option, Control,
/// Shift), as event flags.
pub fn held_modifiers() -> u64 {
    // SAFETY: reads global keyboard state only.
    let flags = unsafe { CGEventSourceFlagsState(COMBINED_SESSION_STATE) };
    flags & (FLAG_SHIFT | FLAG_CONTROL | FLAG_ALTERNATE | FLAG_COMMAND)
}

/// An accessibility element: a window, a text field, an app.
#[derive(Clone, PartialEq)]
pub struct Element(CFType);

// AXUIElement is a CFType that may be used from any thread.
unsafe impl Send for Element {}

impl Element {
    /// # Safety
    /// `raw` must be an AXUIElementRef returned under the create rule.
    unsafe fn from_create(raw: AXUIElementRef) -> Option<Self> {
        (!raw.is_null()).then(|| Element(unsafe { CFType::wrap_under_create_rule(raw) }))
    }

    pub fn system_wide() -> Option<Self> {
        // SAFETY: returns a new reference we own.
        unsafe { Self::from_create(AXUIElementCreateSystemWide()) }
    }

    pub fn application(pid: libc::pid_t) -> Option<Self> {
        // SAFETY: returns a new reference we own.
        unsafe { Self::from_create(AXUIElementCreateApplication(pid)) }
    }

    /// How long calls through this element wait for a busy app. Set on the
    /// system wide element, it applies to every element.
    pub fn set_timeout(&self, seconds: f32) {
        // SAFETY: a live element.
        unsafe {
            AXUIElementSetMessagingTimeout(self.0.as_CFTypeRef(), seconds);
        }
    }

    fn attribute(&self, name: &str) -> Option<CFType> {
        let name = CFString::new(name);
        let mut out: CFTypeRef = std::ptr::null();
        // SAFETY: valid element and attribute name; on success `out` holds
        // a reference we own.
        let err = unsafe {
            AXUIElementCopyAttributeValue(
                self.0.as_CFTypeRef(),
                name.as_concrete_TypeRef(),
                &mut out,
            )
        };
        (err == AX_SUCCESS && !out.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(out) })
    }

    pub fn element(&self, name: &str) -> Option<Element> {
        // An attribute holding an element; the reference is ours already.
        let value = self.attribute(name)?;
        Some(Element(value))
    }

    pub fn string(&self, name: &str) -> Option<String> {
        self.attribute(name)?
            .downcast::<CFString>()
            .map(|s| s.to_string())
    }

    pub fn number(&self, name: &str) -> Option<i64> {
        self.attribute(name)?
            .downcast::<core_foundation::number::CFNumber>()
            .and_then(|n| n.to_i64())
    }

    /// A range attribute such as the selection, in UTF-16 units.
    pub fn range(&self, name: &str) -> Option<(usize, usize)> {
        let value = self.attribute(name)?;
        let mut range = CFRange {
            location: 0,
            length: 0,
        };
        // SAFETY: `range` is a CFRange, matching the requested type.
        let ok = unsafe {
            AXValueGetValue(
                value.as_CFTypeRef(),
                AX_VALUE_CF_RANGE_TYPE,
                &mut range as *mut CFRange as *mut c_void,
            )
        };
        (ok && range.location >= 0 && range.length >= 0)
            .then_some((range.location as usize, range.length as usize))
    }

    /// The text in a range of UTF-16 units.
    pub fn string_for_range(&self, start: usize, len: usize) -> Option<String> {
        let range = CFRange {
            location: start as CFIndex,
            length: len as CFIndex,
        };
        // SAFETY: AXValueCreate copies the range; the result is ours.
        let param = unsafe {
            AXValueCreate(AX_VALUE_CF_RANGE_TYPE, &range as *const CFRange as *const c_void)
        };
        if param.is_null() {
            return None;
        }
        // SAFETY: created above under the create rule.
        let param = unsafe { CFType::wrap_under_create_rule(param) };
        let name = CFString::new("AXStringForRange");
        let mut out: CFTypeRef = std::ptr::null();
        // SAFETY: valid element, attribute and parameter.
        let err = unsafe {
            AXUIElementCopyParameterizedAttributeValue(
                self.0.as_CFTypeRef(),
                name.as_concrete_TypeRef(),
                param.as_CFTypeRef(),
                &mut out,
            )
        };
        if err != AX_SUCCESS || out.is_null() {
            return None;
        }
        // SAFETY: copied under the create rule.
        unsafe { CFType::wrap_under_create_rule(out) }
            .downcast::<CFString>()
            .map(|s| s.to_string())
    }

    pub fn pid(&self) -> Option<libc::pid_t> {
        let mut pid: libc::pid_t = 0;
        // SAFETY: valid element and out pointer.
        let err = unsafe { AXUIElementGetPid(self.0.as_CFTypeRef(), &mut pid) };
        (err == AX_SUCCESS).then_some(pid)
    }
}
