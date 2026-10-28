// Typed wrappers around the Rust commands and events.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Family = "whisper" | "parakeet" | "moonshine" | "sensevoice";

export interface ModelInfo {
  id: string;
  name: string;
  family: Family;
  /** 1 to 5; 0 for custom models (unknown). */
  accuracy: number;
  speed: number;
  /** Language codes, or ["*"] for multilingual Whisper models. */
  languages: string[];
  languagesLabel: string;
  ramMb: number;
  sizeBytes: number;
  note: string | null;
  default: boolean;
  custom: boolean;
  installed: boolean;
}

export interface Progress {
  done: number;
  total: number;
}

export interface ModelsView {
  models: ModelInfo[];
  active: string | null;
  loading: string | null;
  downloads: Record<string, Progress>;
  romanUrdu: boolean;
}

export interface TranscriptRow {
  id: number;
  /** ISO 8601 local time, e.g. 2026-10-28T14:53:10. */
  createdAt: string;
  rawText: string;
  finalText: string;
  wordCount: number;
  audioMs: number;
  inferenceMs: number;
  modelId: string;
  language: string | null;
  appName: string | null;
  appTitle: string | null;
  wpm: number;
}

export interface HomeSummary {
  wordsThisWeek: number;
  streakDays: number;
  recent: TranscriptRow[];
}

export interface WordCount {
  word: string;
  count: number;
}

export interface Insights {
  totalDictations: number;
  wordsThisWeek: number;
  /** Last week over the same days (Monday through today's weekday). */
  wordsLastWeek: number;
  totalWords: number;
  averageWpm: number;
  minutesSaved: number;
  streakDays: number;
  mostUsedWord: WordCount | null;
  /** Last 30 days, oldest first. */
  wordsPerDay: { date: string; words: number }[];
  topWords: WordCount[];
  wordsByApp: { app: string | null; words: number }[];
  /** 24 entries, one per hour of the day. */
  dictationsByHour: number[];
}

export type OverlayPosition = "bottomCentre" | "topCentre" | "bottomRight";

/** Mirrors settings.rs. Hotkeys are key names such as ["ctrl", "win"]. */
export interface Settings {
  launchAtLogin: boolean;
  showInTray: boolean;
  closeToTray: boolean;
  mainHotkey: string[];
  toggleHotkeyEnabled: boolean;
  toggleHotkey: string[];
  microphone: string | null;
  inputGain: number;
  unloadAfterMinutes: number;
  language: string;
  romanUrdu: boolean;
  overlayEnabled: boolean;
  overlayPosition: OverlayPosition;
  overlayColour: string;
  soundsEnabled: boolean;
  soundVolume: number;
  removeFillers: boolean;
  addTrailingSpace: boolean;
  voicePunctuation: boolean;
  typingWpm: number;
  keepAudioClips: boolean;
}

export interface DictEntry {
  spoken: string;
  written: string;
}

export interface Snippet {
  trigger: string;
  text: string;
}

export interface UrduRow {
  canonical: string;
  variants: string[];
}

export interface TextData {
  dictionary: DictEntry[];
  snippets: Snippet[];
  romanUrdu: UrduRow[];
}

export type ListKind = "dictionary" | "snippets";

export const api = {
  listModels: () => invoke<ModelsView>("list_models"),
  downloadModel: (id: string) => invoke<void>("download_model", { id }),
  cancelDownload: (id: string) => invoke<void>("cancel_download", { id }),
  deleteModel: (id: string) => invoke<number>("delete_model", { id }),
  useModel: (id: string) => invoke<void>("use_model", { id }),
  onboardingNeeded: () => invoke<boolean>("onboarding_needed"),
  finishOnboarding: () => invoke<void>("finish_onboarding"),
  checkMicrophone: () => invoke<void>("check_microphone"),
  openMicrophoneSettings: () => invoke<void>("open_microphone_settings"),
  listTranscripts: (query: string, limit: number, offset: number) =>
    invoke<TranscriptRow[]>("list_transcripts", { query, limit, offset }),
  deleteTranscript: (id: number) => invoke<boolean>("delete_transcript", { id }),
  clearHistory: () => invoke<void>("clear_history"),
  homeSummary: () => invoke<HomeSummary>("home_summary"),
  appIcon: (appName: string) => invoke<string | null>("app_icon", { appName }),
  insights: () => invoke<Insights>("insights"),
  getSettings: () => invoke<{ settings: Settings; defaults: Settings }>("get_settings"),
  setSettings: (next: Settings) => invoke<Settings>("set_settings", { next }),
  listMicrophones: () => invoke<string[]>("list_microphones"),
  testMicrophone: (on: boolean) => invoke<void>("test_microphone", { on }),
  /** Resolves with the keys pressed, or null if cancelled. */
  captureHotkey: () => invoke<string[] | null>("capture_hotkey"),
  openDataFolder: () => invoke<void>("open_data_folder"),
  openLicences: () => invoke<void>("open_licences"),
  appInfo: () => invoke<{ name: string; version: string }>("app_info"),
  textData: () => invoke<TextData>("text_data"),
  setDictionary: (entries: DictEntry[]) => invoke<void>("set_dictionary", { entries }),
  setSnippets: (snippets: Snippet[]) => invoke<void>("set_snippets", { snippets }),
  setRomanUrdu: (rows: UrduRow[]) => invoke<void>("set_roman_urdu", { rows }),
  resetRomanUrdu: () => invoke<UrduRow[]>("reset_roman_urdu"),
  /** False if the user cancelled the save dialog. */
  exportCsv: (kind: ListKind) => invoke<boolean>("export_csv", { kind }),
  /** Rows read, or null if the user cancelled. */
  importCsv: (kind: ListKind) => invoke<number | null>("import_csv", { kind }),
};

export const events = {
  transcriptAdded: (cb: () => void): Promise<UnlistenFn> => listen("transcript-added", () => cb()),
  insightsChanged: (cb: () => void): Promise<UnlistenFn> => listen("insights-changed", () => cb()),
  /** idle, recording, processing, cancelled or error. */
  dictationState: (cb: (state: string) => void): Promise<UnlistenFn> =>
    listen<string>("dictation-state", (e) => cb(e.payload)),
  micLevel: (cb: (rms: number) => void): Promise<UnlistenFn> =>
    listen<{ rms: number; peak: number }>("mic-level", (e) => cb(e.payload.rms)),
  modelsChanged: (cb: () => void): Promise<UnlistenFn> => listen("models-changed", () => cb()),
  modelDownload: (cb: (p: Progress & { id: string }) => void): Promise<UnlistenFn> =>
    listen<Progress & { id: string }>("model-download", (e) => cb(e.payload)),
  modelError: (cb: (e: { id: string; message: string }) => void): Promise<UnlistenFn> =>
    listen<{ id: string; message: string }>("model-error", (e) => cb(e.payload)),
};

/** Commands reject with a one line message. */
export function errorText(err: unknown): string {
  return typeof err === "string" ? err : err instanceof Error ? err.message : "Something went wrong. Try again.";
}
