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

/** "Code.exe" to "Code", "Slack.app" to "Slack". */
export function appLabel(appName: string | null): string {
  if (!appName) return "Unknown app";
  return appName.replace(/\.(exe|app)$/i, "");
}

export function plural(n: number, one: string, many: string): string {
  return `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;
}

/** "21 min", "1 h 5 min", "3 h". */
export function formatMinutes(minutes: number): string {
  const total = Math.max(0, Math.round(minutes));
  const h = Math.floor(total / 60);
  const m = total % 60;
  if (h === 0) return `${m} min`;
  return m === 0 ? `${h} h` : `${h} h ${m} min`;
}

/** "+12 vs last week", "-3 vs last week", "Same as last week". */
export function formatDelta(now: number, before: number): string {
  const d = now - before;
  if (d === 0) return "Same as last week";
  return `${d > 0 ? "+" : "-"}${Math.abs(d).toLocaleString("en-US")} vs last week`;
}

/** "2026-10-28" to "28 Oct". */
export function shortDate(ymd: string): string {
  const [y, m, d] = ymd.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString("en-GB", { day: "numeric", month: "short" });
}
