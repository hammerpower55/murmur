// Typed wrappers around the Rust commands and events.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Family = "whisper" | "parakeet";

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
};

export const events = {
  transcriptAdded: (cb: () => void): Promise<UnlistenFn> => listen("transcript-added", () => cb()),
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
