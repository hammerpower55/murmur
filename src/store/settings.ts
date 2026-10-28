// Live settings. Every change is saved at once; if Rust refuses it, the old
// value comes back and the reason is shown.

import { useEffect } from "react";
import { create } from "zustand";
import { api, errorText, type Settings } from "../lib/api";
import { hotkeySpoken } from "../lib/keys";

interface SettingsState {
  settings: Settings | null;
  defaults: Settings | null;
  error: string | null;
  load: () => Promise<void>;
  update: (patch: Partial<Settings>) => Promise<boolean>;
  clearError: () => void;
}

export const useSettings = create<SettingsState>((set, get) => ({
  settings: null,
  defaults: null,
  error: null,
  load: async () => {
    const { settings, defaults } = await api.getSettings();
    set({ settings, defaults });
  },
  update: async (patch) => {
    const before = get().settings;
    if (!before) return false;
    const next = { ...before, ...patch };
    set({ settings: next, error: null });
    try {
      const saved = await api.setSettings(next);
      // Another change may have landed while this one was saving.
      if (get().settings === next) set({ settings: saved });
      return true;
    } catch (err) {
      if (get().settings === next) set({ settings: before });
      set({ error: errorText(err) });
      return false;
    }
  },
  clearError: () => set({ error: null }),
}));

/** The push to talk hotkey for sentences, e.g. "Ctrl and Win". */
export function useHotkeyText(): string {
  const settings = useSettings((s) => s.settings);
  const load = useSettings((s) => s.load);
  useEffect(() => {
    if (!settings) void load();
  }, [settings, load]);
  return hotkeySpoken(settings?.mainHotkey ?? ["ctrl", "win"]);
}
