import { describe, expect, it } from "vitest";
import { hotkeyLabel, hotkeySpoken, keyLabel } from "./keys";
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
