//! Hotkeys as stored in settings.json: a list of key names such as
//! ["ctrl", "win"]. Modifier names match either side of the keyboard. Keys
//! without a friendly name are stored as "vk:" plus their hex code.

use anyhow::{bail, Result};
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;
#[cfg(target_os = "macos")]
use super::vk_mac::VIRTUAL_KEY;

use super::listener::{Combo, Key};

const MODIFIERS: [(&str, [u16; 2]); 4] = [
    ("ctrl", [0xA2, 0xA3]),
    ("alt", [0xA4, 0xA5]),
    ("shift", [0xA0, 0xA1]),
    ("win", [0x5B, 0x5C]),
];

const NAMED: [(&str, u16); 21] = [
    ("space", 0x20),
    ("enter", 0x0D),
    ("tab", 0x09),
    ("backspace", 0x08),
    ("escape", 0x1B),
    ("pause", 0x13),
    ("capslock", 0x14),
    ("scrolllock", 0x91),
    ("insert", 0x2D),
    ("delete", 0x2E),
    ("home", 0x24),
    ("end", 0x23),
    ("pageup", 0x21),
    ("pagedown", 0x22),
    ("left", 0x25),
    ("up", 0x26),
    ("right", 0x27),
    ("down", 0x28),
    ("menu", 0x5D),
    ("printscreen", 0x2C),
    ("numlock", 0x90),
];

#[cfg(windows)]
const NEEDS_MODIFIER: &str = "Add Ctrl, Alt, Shift or Win, or use a function key.";
#[cfg(target_os = "macos")]
const NEEDS_MODIFIER: &str = "Add Control, Option, Shift or Command, or use a function key.";

fn is_modifier(name: &str) -> bool {
    MODIFIERS.iter().any(|(n, _)| *n == name)
}

/// The keys one name stands for, or None for an unknown name.
fn slot(name: &str) -> Option<Vec<Key>> {
    if let Some((_, vks)) = MODIFIERS.iter().find(|(n, _)| *n == name) {
        return Some(vks.iter().map(|v| VIRTUAL_KEY(*v)).collect());
    }
    if let Some((_, vk)) = NAMED.iter().find(|(n, _)| *n == name) {
        return Some(vec![VIRTUAL_KEY(*vk)]);
    }
    let one = |vk: u16| Some(vec![VIRTUAL_KEY(vk)]);
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            return one(c.to_ascii_uppercase() as u16);
        }
    }
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u16>().ok()) {
        if (1..=24).contains(&n) {
            return one(0x70 + n - 1);
        }
    }
    if let Some(n) = name.strip_prefix("num").and_then(|n| n.parse::<u16>().ok()) {
        if n <= 9 {
            return one(0x60 + n);
        }
    }
    if let Some(hex) = name.strip_prefix("vk:") {
        if let Ok(vk) = u16::from_str_radix(hex, 16) {
            if (1..=0xFE).contains(&vk) {
                return one(vk);
            }
        }
    }
    None
}

/// The stored name for a virtual key. Settings records hotkeys in the page
/// (src/lib/keys.ts names keys the same way); this keeps the two in step.
#[cfg(test)]
pub fn name_of(key: Key) -> String {
    let vk = key.0;
    if let Some((name, _)) = MODIFIERS.iter().find(|(_, vks)| vks.contains(&vk)) {
        return name.to_string();
    }
    if let Some((name, _)) = NAMED.iter().find(|(_, v)| *v == vk) {
        return name.to_string();
    }
    match vk {
        0x30..=0x39 | 0x41..=0x5A => ((vk as u8) as char).to_ascii_lowercase().to_string(),
        0x60..=0x69 => format!("num{}", vk - 0x60),
        0x70..=0x87 => format!("f{}", vk - 0x70 + 1),
        _ => format!("vk:{vk:02x}"),
    }
}

/// Key names in a stable order: modifiers first (Ctrl, Alt, Shift, Win),
/// then the rest in the order pressed, without repeats.
#[cfg(test)]
pub fn names_of(pressed: &[Key]) -> Vec<String> {
    let names: Vec<String> = pressed.iter().map(|k| name_of(*k)).collect();
    let mut out: Vec<String> = MODIFIERS
        .iter()
        .map(|(n, _)| n.to_string())
        .filter(|n| names.contains(n))
        .collect();
    for n in names {
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

pub fn combo(names: &[String]) -> Result<Combo> {
    let slots = names
        .iter()
        .map(|n| slot(n).ok_or_else(|| anyhow::anyhow!("unknown key {n}")))
        .collect::<Result<Vec<_>>>()?;
    Ok(Combo::new(slots))
}

/// A hotkey the hook can use without getting in the way of typing.
pub fn validate_hotkey(names: &[String]) -> Result<()> {
    if names.is_empty() || names.len() > 4 {
        bail!("Use one to four keys.");
    }
    if names.iter().any(|n| slot(n).is_none()) {
        bail!("That key cannot be used in a hotkey.");
    }
    if (1..names.len()).any(|i| names[..i].contains(&names[i])) {
        bail!("Use each key once.");
    }
    if names.iter().any(|n| n == "escape") {
        bail!("Escape cancels a dictation, so it cannot be part of a hotkey.");
    }
    let modifiers = names.iter().filter(|n| is_modifier(n)).count();
    let function_keys = |n: &String| {
        n.starts_with('f') && n.len() > 1 && n[1..].chars().all(|c| c.is_ascii_digit())
            || n == "pause"
            || n == "scrolllock"
    };
    if modifiers == 0 && !names.iter().all(function_keys) {
        bail!(NEEDS_MODIFIER);
    }
    if modifiers == 1 && names.len() == 1 {
        bail!("Add a second key so the modifier still works on its own.");
    }
    Ok(())
}

pub fn same_combo(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|k| b.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn names_round_trip_through_virtual_keys() {
        for name in ["ctrl", "win", "space", "a", "7", "f9", "f24", "num3", "pause", "vk:ba"] {
            let keys = slot(name).unwrap();
            assert_eq!(name_of(keys[0]), name, "{name}");
        }
        assert_eq!(slot("ctrl").unwrap().len(), 2);
        assert!(slot("f25").is_none());
        assert!(slot("hyper").is_none());
    }

    #[test]
    fn captured_keys_become_ordered_names() {
        let keys = [VIRTUAL_KEY(0x20), VIRTUAL_KEY(0xA1), VIRTUAL_KEY(0xA3), VIRTUAL_KEY(0xA2)];
        assert_eq!(names_of(&keys), v(&["ctrl", "shift", "space"]));
    }

    #[test]
    fn default_hotkeys_match_the_built_in_combos() {
        assert_eq!(combo(&v(&["ctrl", "win"])).unwrap(), Combo::ctrl_win());
        assert_eq!(
            combo(&v(&["ctrl", "shift", "space"])).unwrap(),
            Combo::ctrl_shift_space()
        );
    }

    #[test]
    fn validation() {
        assert!(validate_hotkey(&v(&["ctrl", "win"])).is_ok());
        assert!(validate_hotkey(&v(&["alt", "f2"])).is_ok());
        assert!(validate_hotkey(&v(&["f9"])).is_ok());
        assert!(validate_hotkey(&v(&["pause"])).is_ok());
        assert!(validate_hotkey(&v(&[])).is_err());
        assert!(validate_hotkey(&v(&["a"])).is_err());
        assert!(validate_hotkey(&v(&["space"])).is_err());
        assert!(validate_hotkey(&v(&["ctrl"])).is_err());
        assert!(validate_hotkey(&v(&["ctrl", "escape"])).is_err());
        assert!(validate_hotkey(&v(&["ctrl", "ctrl"])).is_err());
        assert!(validate_hotkey(&v(&["ctrl", "alt", "shift", "win", "a"])).is_err());
    }

    #[test]
    fn same_combo_ignores_order() {
        assert!(same_combo(&v(&["ctrl", "win"]), &v(&["win", "ctrl"])));
        assert!(!same_combo(&v(&["ctrl", "win"]), &v(&["ctrl", "alt"])));
    }
}
