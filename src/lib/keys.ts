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

/** For sentences: "Ctrl and Win", "Ctrl, Shift and Space". */
export function hotkeySpoken(names: string[]): string {
  const labels = names.map(keyLabel);
  if (labels.length <= 1) return labels.join("");
  return `${labels.slice(0, -1).join(", ")} and ${labels[labels.length - 1]}`;
}
