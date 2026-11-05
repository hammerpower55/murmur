import { describe, expect, it } from "vitest";
import { hotkeyLabel, hotkeySpoken, keyLabel, keyLabelOn, nameOfKey, orderKeys } from "./keys";

describe("nameOfKey", () => {
  it("names keys the way settings.json stores them (hotkey/keys.rs)", () => {
    expect(nameOfKey("ControlRight", 0xa3)).toBe("ctrl");
    expect(nameOfKey("MetaLeft", 0x5b)).toBe("win");
    expect(nameOfKey("AltLeft", 0xa4)).toBe("alt");
    expect(nameOfKey("KeyD", 0x44)).toBe("d");
    expect(nameOfKey("Digit7", 0x37)).toBe("7");
    expect(nameOfKey("F9", 0x78)).toBe("f9");
    expect(nameOfKey("F24", 0x87)).toBe("f24");
    expect(nameOfKey("Numpad3", 0x63)).toBe("num3");
    expect(nameOfKey("Space", 0x20)).toBe("space");
    expect(nameOfKey("PageUp", 0x21)).toBe("pageup");
    expect(nameOfKey("Semicolon", 0xba)).toBe("vk:ba");
  });

  it("falls back to the virtual key when the event has no code", () => {
    expect(nameOfKey("", 0x12)).toBe("alt");
    expect(nameOfKey("", 0x11)).toBe("ctrl");
    expect(nameOfKey("", 0x5b)).toBe("win");
    expect(nameOfKey("", 0x78)).toBe("f9");
    expect(nameOfKey("", 0x44)).toBe("d");
    expect(nameOfKey("", 0x63)).toBe("num3");
    expect(nameOfKey("", 0xba)).toBe("vk:ba");
  });

  it("orders modifiers first and drops repeats", () => {
    expect(orderKeys(["space", "shift", "ctrl"])).toEqual(["ctrl", "shift", "space"]);
    expect(orderKeys(["win", "ctrl", "win"])).toEqual(["ctrl", "win"]);
    expect(orderKeys(["f9", "alt"])).toEqual(["alt", "f9"]);
  });
});
import { languageName, languagesFor, WHISPER_LANGUAGES } from "./languages";

describe("keyLabel", () => {
  it("names modifiers and special keys", () => {
    expect(hotkeyLabel(["ctrl", "win"])).toBe("Ctrl + Win");
    expect(hotkeyLabel(["ctrl", "shift", "space"])).toBe("Ctrl + Shift + Space");
    expect(keyLabel("pageup")).toBe("Page Up");
  });

  it("reads naturally in a sentence", () => {
    expect(hotkeySpoken(["ctrl", "win"])).toBe("Ctrl and Win");
    expect(hotkeySpoken(["ctrl", "shift", "space"])).toBe("Ctrl, Shift and Space");
    expect(hotkeySpoken(["f9"])).toBe("F9");
  });

  it("upper cases letters, digits and function keys", () => {
    expect(keyLabel("a")).toBe("A");
    expect(keyLabel("7")).toBe("7");
    expect(keyLabel("f12")).toBe("F12");
    expect(keyLabel("num3")).toBe("Num 3");
    expect(keyLabel("vk:ba")).toBe("Key BA");
  });

  it("uses Mac names for the modifiers on a Mac", () => {
    expect(keyLabelOn("win", true)).toBe("Cmd");
    expect(keyLabelOn("alt", true)).toBe("Option");
    expect(keyLabelOn("ctrl", true)).toBe("Control");
    expect(keyLabelOn("shift", true)).toBe("Shift");
    expect(keyLabelOn("win", false)).toBe("Win");
  });
});

describe("languages", () => {
  it("lists every Whisper language once", () => {
    const codes = WHISPER_LANGUAGES.map(([c]) => c);
    expect(new Set(codes).size).toBe(codes.length);
    expect(codes.length).toBe(100);
    expect(codes[0]).toBe("en");
  });

  it("gives a model's languages by name", () => {
    expect(languagesFor(["*"])).toBe(WHISPER_LANGUAGES);
    expect(languagesFor(["en"])).toEqual([["en", "English"]]);
    expect(languageName("yue")).toBe("Cantonese");
    expect(languageName("xx")).toBe("XX");
  });
});
