//! Key codes on macOS. The hook logic and the stored key names
//! (`hotkey/keys.rs`) work in Windows virtual key codes, so a Mac key code
//! from the event tap is translated into the matching virtual key first.
//! Command is "win", Option is "alt".

#![allow(non_camel_case_types)]

/// A Windows virtual key code, the same shape as the `windows` crate's type.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VIRTUAL_KEY(pub u16);

pub const VK_ESCAPE: VIRTUAL_KEY = VIRTUAL_KEY(0x1B);
pub const VK_SPACE: VIRTUAL_KEY = VIRTUAL_KEY(0x20);
pub const VK_LSHIFT: VIRTUAL_KEY = VIRTUAL_KEY(0xA0);
pub const VK_RSHIFT: VIRTUAL_KEY = VIRTUAL_KEY(0xA1);
pub const VK_LCONTROL: VIRTUAL_KEY = VIRTUAL_KEY(0xA2);
pub const VK_RCONTROL: VIRTUAL_KEY = VIRTUAL_KEY(0xA3);
pub const VK_LMENU: VIRTUAL_KEY = VIRTUAL_KEY(0xA4);
pub const VK_RMENU: VIRTUAL_KEY = VIRTUAL_KEY(0xA5);
pub const VK_LWIN: VIRTUAL_KEY = VIRTUAL_KEY(0x5B);
pub const VK_RWIN: VIRTUAL_KEY = VIRTUAL_KEY(0x5C);
#[cfg(test)]
pub const VK_A: VIRTUAL_KEY = VIRTUAL_KEY(0x41);
#[cfg(test)]
pub const VK_C: VIRTUAL_KEY = VIRTUAL_KEY(0x43);
#[cfg(test)]
pub const VK_D: VIRTUAL_KEY = VIRTUAL_KEY(0x44);

/// Mac key code (kVK_* in Carbon's Events.h) to virtual key.
const TABLE: &[(u16, u16)] = &[
    // Letters
    (0x00, 0x41), // A
    (0x0B, 0x42), // B
    (0x08, 0x43), // C
    (0x02, 0x44), // D
    (0x0E, 0x45), // E
    (0x03, 0x46), // F
    (0x05, 0x47), // G
    (0x04, 0x48), // H
    (0x22, 0x49), // I
    (0x26, 0x4A), // J
    (0x28, 0x4B), // K
    (0x25, 0x4C), // L
    (0x2E, 0x4D), // M
    (0x2D, 0x4E), // N
    (0x1F, 0x4F), // O
    (0x23, 0x50), // P
    (0x0C, 0x51), // Q
    (0x0F, 0x52), // R
    (0x01, 0x53), // S
    (0x11, 0x54), // T
    (0x20, 0x55), // U
    (0x09, 0x56), // V
    (0x0D, 0x57), // W
    (0x07, 0x58), // X
    (0x10, 0x59), // Y
    (0x06, 0x5A), // Z
    // Digits
    (0x1D, 0x30),
    (0x12, 0x31),
    (0x13, 0x32),
    (0x14, 0x33),
    (0x15, 0x34),
    (0x17, 0x35),
    (0x16, 0x36),
    (0x1A, 0x37),
    (0x1C, 0x38),
    (0x19, 0x39),
    // Punctuation, as the OEM keys in the same place on a US keyboard
    (0x29, 0xBA), // ;
    (0x18, 0xBB), // =
    (0x2B, 0xBC), // ,
    (0x1B, 0xBD), // -
    (0x2F, 0xBE), // .
    (0x2C, 0xBF), // /
    (0x32, 0xC0), // `
    (0x21, 0xDB), // [
    (0x2A, 0xDC), // \
    (0x1E, 0xDD), // ]
    (0x27, 0xDE), // '
    // Editing and navigation
    (0x24, 0x0D), // Return
    (0x4C, 0x0D), // keypad Enter
    (0x30, 0x09), // Tab
    (0x31, 0x20), // Space
    (0x33, 0x08), // Delete (backspace)
    (0x75, 0x2E), // Forward delete
    (0x35, 0x1B), // Escape
    (0x72, 0x2D), // Help, where PC keyboards have Insert
    (0x73, 0x24), // Home
    (0x77, 0x23), // End
    (0x74, 0x21), // Page up
    (0x79, 0x22), // Page down
    (0x7B, 0x25), // Left
    (0x7E, 0x26), // Up
    (0x7C, 0x27), // Right
    (0x7D, 0x28), // Down
    (0x6E, 0x5D), // Context menu on PC keyboards
    (0x47, 0x90), // keypad Clear, where PC keyboards have Num Lock
    // Modifiers
    (0x37, 0x5B), // Command
    (0x36, 0x5C), // right Command
    (0x38, 0xA0), // Shift
    (0x3C, 0xA1), // right Shift
    (0x3B, 0xA2), // Control
    (0x3E, 0xA3), // right Control
    (0x3A, 0xA4), // Option
    (0x3D, 0xA5), // right Option
    // Keypad digits
    (0x52, 0x60),
    (0x53, 0x61),
    (0x54, 0x62),
    (0x55, 0x63),
    (0x56, 0x64),
    (0x57, 0x65),
    (0x58, 0x66),
    (0x59, 0x67),
    (0x5B, 0x68),
    (0x5C, 0x69),
    // Function keys
    (0x7A, 0x70), // F1
    (0x78, 0x71),
    (0x63, 0x72),
    (0x76, 0x73),
    (0x60, 0x74),
    (0x61, 0x75),
    (0x62, 0x76),
    (0x64, 0x77),
    (0x65, 0x78),
    (0x6D, 0x79),
    (0x67, 0x7A),
    (0x6F, 0x7B), // F12
    (0x69, 0x7C), // F13
    (0x6B, 0x7D),
    (0x71, 0x7E),
    (0x6A, 0x7F),
    (0x40, 0x80),
    (0x4F, 0x81),
    (0x50, 0x82),
    (0x5A, 0x83), // F20
];

/// The virtual key for a Mac key code, or None for keys with no
/// counterpart (Fn, volume keys).
pub fn from_mac(code: u16) -> Option<VIRTUAL_KEY> {
    TABLE
        .iter()
        .find(|(mac, _)| *mac == code)
        .map(|(_, vk)| VIRTUAL_KEY(*vk))
}

/// The modifier flag bit (device dependent, so left and right differ) that
/// is set while this modifier key is down, from IOKit's IOLLEvent.h.
pub fn modifier_flag(code: u16) -> Option<u64> {
    Some(match code {
        0x3B => 0x0000_0001, // NX_DEVICELCTLKEYMASK
        0x38 => 0x0000_0002, // NX_DEVICELSHIFTKEYMASK
        0x3C => 0x0000_0004, // NX_DEVICERSHIFTKEYMASK
        0x37 => 0x0000_0008, // NX_DEVICELCMDKEYMASK
        0x36 => 0x0000_0010, // NX_DEVICERCMDKEYMASK
        0x3A => 0x0000_0020, // NX_DEVICELALTKEYMASK
        0x3D => 0x0000_0040, // NX_DEVICERALTKEYMASK
        0x3E => 0x0000_2000, // NX_DEVICERCTLKEYMASK
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mac_key_maps_once() {
        for (i, (mac, _)) in TABLE.iter().enumerate() {
            assert!(
                TABLE[i + 1..].iter().all(|(m, _)| m != mac),
                "key code {mac:#x} listed twice"
            );
        }
    }

    #[test]
    fn modifiers_and_common_keys() {
        assert_eq!(from_mac(0x37), Some(VK_LWIN));
        assert_eq!(from_mac(0x3B), Some(VK_LCONTROL));
        assert_eq!(from_mac(0x3D), Some(VK_RMENU));
        assert_eq!(from_mac(0x31), Some(VK_SPACE));
        assert_eq!(from_mac(0x35), Some(VK_ESCAPE));
        assert_eq!(from_mac(0x00), Some(VK_A));
        assert_eq!(from_mac(0x65), Some(VIRTUAL_KEY(0x78))); // F9
        assert_eq!(from_mac(0x3F), None); // Fn
        for code in [0x3B, 0x3E, 0x38, 0x3C, 0x37, 0x36, 0x3A, 0x3D] {
            assert!(modifier_flag(code).is_some());
        }
        assert!(modifier_flag(0x31).is_none());
    }
}
