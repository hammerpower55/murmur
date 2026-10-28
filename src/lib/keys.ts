// Labels for hotkey key names as stored in settings.json (see
// src-tauri/src/hotkey/keys.rs).

const LABELS: Record<string, string> = {
  ctrl: "Ctrl",
  alt: "Alt",
  shift: "Shift",
  win: "Win",
  space: "Space",
  enter: "Enter",
  tab: "Tab",
  backspace: "Backspace",
  escape: "Esc",
  pause: "Pause",
  capslock: "Caps Lock",
  scrolllock: "Scroll Lock",
  insert: "Insert",
  delete: "Delete",
  home: "Home",
  end: "End",
  pageup: "Page Up",
  pagedown: "Page Down",
  left: "Left",
  up: "Up",
  right: "Right",
  down: "Down",
  menu: "Menu",
  printscreen: "Print Screen",
  numlock: "Num Lock",
};

export function keyLabel(name: string): string {
  if (LABELS[name]) return LABELS[name];
  if (/^[a-z0-9]$/.test(name)) return name.toUpperCase();
  if (/^f\d{1,2}$/.test(name)) return name.toUpperCase();
  if (/^num\d$/.test(name)) return `Num ${name.slice(3)}`;
  if (name.startsWith("vk:")) return `Key ${name.slice(3).toUpperCase()}`;
  return name;
}

/** "Ctrl + Win". */
export function hotkeyLabel(names: string[]): string {
  return names.map(keyLabel).join(" + ");
}

const CODE_NAMES: Record<string, string> = {
  ControlLeft: "ctrl",
  ControlRight: "ctrl",
  AltLeft: "alt",
  AltRight: "alt",
  ShiftLeft: "shift",
  ShiftRight: "shift",
  MetaLeft: "win",
  MetaRight: "win",
  OSLeft: "win",
  OSRight: "win",
  Space: "space",
  Enter: "enter",
  NumpadEnter: "enter",
  Tab: "tab",
  Backspace: "backspace",
  Escape: "escape",
  Pause: "pause",
  CapsLock: "capslock",
  ScrollLock: "scrolllock",
  Insert: "insert",
  Delete: "delete",
  Home: "home",
  End: "end",
  PageUp: "pageup",
  PageDown: "pagedown",
  ArrowLeft: "left",
  ArrowUp: "up",
  ArrowRight: "right",
  ArrowDown: "down",
  ContextMenu: "menu",
  PrintScreen: "printscreen",
  NumLock: "numlock",
};

/**
 * The stored name for a key from a browser KeyboardEvent, matching
 * src-tauri/src/hotkey/keys.rs. Keys without a name use their Windows
 * virtual key code, which `keyCode` carries on Windows.
 */
export function nameOfKey(code: string, keyCode: number): string {
  if (code === "") return nameOfVk(keyCode);
  if (CODE_NAMES[code]) return CODE_NAMES[code];
  let m = /^Key([A-Z])$/.exec(code);
  if (m) return m[1].toLowerCase();
  m = /^Digit(\d)$/.exec(code);
  if (m) return m[1];
  m = /^F(\d{1,2})$/.exec(code);
  if (m && Number(m[1]) >= 1 && Number(m[1]) <= 24) return `f${m[1]}`;
  m = /^Numpad(\d)$/.exec(code);
  if (m) return `num${m[1]}`;
  return `vk:${keyCode.toString(16).padStart(2, "0")}`;
}

const VK_NAMES: Record<number, string> = {
  0x10: "shift",
  0x11: "ctrl",
  0x12: "alt",
  0x5b: "win",
  0x5c: "win",
  0xa0: "shift",
  0xa1: "shift",
  0xa2: "ctrl",
  0xa3: "ctrl",
  0xa4: "alt",
  0xa5: "alt",
  0x20: "space",
  0x0d: "enter",
  0x09: "tab",
  0x08: "backspace",
  0x1b: "escape",
  0x13: "pause",
  0x14: "capslock",
  0x91: "scrolllock",
  0x2d: "insert",
  0x2e: "delete",
  0x24: "home",
  0x23: "end",
  0x21: "pageup",
  0x22: "pagedown",
  0x25: "left",
  0x26: "up",
  0x27: "right",
  0x28: "down",
  0x5d: "menu",
  0x2c: "printscreen",
  0x90: "numlock",
};

/** Name from a Windows virtual key code, for events that carry no `code`. */
export function nameOfVk(vk: number): string {
  if (VK_NAMES[vk]) return VK_NAMES[vk];
  if ((vk >= 0x30 && vk <= 0x39) || (vk >= 0x41 && vk <= 0x5a)) return String.fromCharCode(vk).toLowerCase();
  if (vk >= 0x60 && vk <= 0x69) return `num${vk - 0x60}`;
  if (vk >= 0x70 && vk <= 0x87) return `f${vk - 0x70 + 1}`;
  return `vk:${vk.toString(16).padStart(2, "0")}`;
}

const MODIFIER_ORDER = ["ctrl", "alt", "shift", "win"];

/** Modifiers first (Ctrl, Alt, Shift, Win), then the rest as pressed, no repeats. */
export function orderKeys(pressed: string[]): string[] {
  const mods = MODIFIER_ORDER.filter((m) => pressed.includes(m));
  const rest = pressed.filter((k, i) => !MODIFIER_ORDER.includes(k) && pressed.indexOf(k) === i);
  return [...mods, ...rest];
}

/**
 * Records one key combination typed into this window: resolves with its key
 * names once every key is released, or null on Escape alone, when the
 * window loses focus, or after `timeoutMs`. Keys are kept from the page
 * while recording. Done here rather than in the global hook, because keys
 * typed into Murmur's own window never reach a low level hook.
 */
export function recordHotkey(timeoutMs = 10_000): Promise<string[] | null> {
  return new Promise((resolve) => {
    const held = new Set<string>();
    const seen: string[] = [];
    const finish = (keys: string[] | null) => {
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
      window.removeEventListener("blur", cancel);
      window.clearTimeout(timer);
      resolve(keys && keys.length > 0 ? orderKeys(keys) : null);
    };
    const cancel = () => finish(null);
    // Physical key identity: `code` when the event has one, else the key code.
    const id = (e: KeyboardEvent) => e.code || `vk${e.keyCode}`;
    const down = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const name = nameOfKey(e.code, e.keyCode);
      if (name === "escape" && seen.length === 0) return finish(null);
      held.add(id(e));
      if (!seen.includes(name)) seen.push(name);
    };
    const up = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (!held.delete(id(e))) return;
      if (held.size === 0 && seen.length > 0) finish(seen);
    };
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    window.addEventListener("blur", cancel);
    const timer = window.setTimeout(cancel, timeoutMs);
  });
}

/** For sentences: "Ctrl and Win", "Ctrl, Shift and Space". */
export function hotkeySpoken(names: string[]): string {
  const labels = names.map(keyLabel);
  if (labels.length <= 1) return labels.join("");
  return `${labels.slice(0, -1).join(", ")} and ${labels[labels.length - 1]}`;
}
