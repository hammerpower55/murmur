// Display helpers. Sizes use 1024 based units, as Windows Explorer does.

export function formatBytes(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  if (mb < 1) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  if (mb < 1000) return `${Math.round(mb)} MB`;
  return `${(mb / 1024).toFixed(1)} GB`;
}

export function formatRam(mb: number): string {
  return mb >= 1000 ? `${(mb / 1024).toFixed(1)} GB RAM` : `${mb} MB RAM`;
}

export function percent(done: number, total: number): number {
  if (total <= 0) return 0;
  return Math.min(100, Math.floor((done / total) * 100));
}

/** Languages offered in the filter, by code. */
export const LANGUAGE_NAMES: Record<string, string> = {
  en: "English",
  ur: "Urdu",
  hi: "Hindi",
  ar: "Arabic",
  fr: "French",
  de: "German",
  es: "Spanish",
  it: "Italian",
  pt: "Portuguese",
  nl: "Dutch",
  pl: "Polish",
  ru: "Russian",
  uk: "Ukrainian",
};

export function supportsLanguage(languages: string[], code: string): boolean {
  return code === "" || languages.includes("*") || languages.includes(code);
}

/** "2:45 PM" today, "Yesterday, 2:45 PM", else "28 Oct, 2:45 PM". */
export function formatWhen(iso: string, now: Date = new Date()): string {
  const when = new Date(iso);
  const time = when.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  const day = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const diffDays = Math.round((day(now) - day(when)) / 86_400_000);
  if (diffDays === 0) return time;
  if (diffDays === 1) return `Yesterday, ${time}`;
  const date = when.toLocaleDateString("en-GB", { day: "numeric", month: "short" });
  return `${date}, ${time}`;
}

/** "4.2 s", or "1 min 5 s" from a minute up. */
export function formatDuration(ms: number): string {
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(1)} s`;
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  return s === 0 ? `${m} min` : `${m} min ${s} s`;
}

/** "Code.exe" to "Code". */
export function appLabel(appName: string | null): string {
  if (!appName) return "Unknown app";
  return appName.replace(/\.exe$/i, "");
}

export function plural(n: number, one: string, many: string): string {
  return `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;
}
