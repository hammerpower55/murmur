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
