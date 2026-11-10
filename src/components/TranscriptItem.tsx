import { useState } from "react";
import { type TranscriptRow } from "../lib/api";
import { appLabel, formatDuration, formatWhen, plural } from "../lib/format";
import { languageName } from "../lib/languages";
import { AppIcon } from "./AppIcon";
import { linkClass } from "./controls";

const FRESH_MS = 8000;
/** The latest dictation runs large, unless it would fill the page. */
const LEAD_MAX_CHARS = 280;

interface Props {
  row: TranscriptRow;
  onDelete?: () => void;
  /** Replaces the date and time, where a heading already gives the day. */
  time?: string;
  /** Set as the page's lead paragraph. */
  lead?: boolean;
}

export function TranscriptItem({ row, onDelete, time, lead }: Props) {
  const [copied, setCopied] = useState(false);
  // Dictated in the last few seconds: greet it with a wash of amber, once.
  const [fresh] = useState(() => Date.now() - new Date(row.createdAt).getTime() < FRESH_MS);

  const copy = async () => {
    await navigator.clipboard.writeText(row.finalText);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  };

  return (
    <article className={`lean group py-4 ${fresh ? "fresh" : ""}`}>
      <p
        className={`max-w-[68ch] whitespace-pre-wrap ${
          lead && row.finalText.length <= LEAD_MAX_CHARS ? "statement" : "text-read"
        }`}
      >
        {row.finalText}
      </p>
      {row.fixedWords.length > 0 && (
        <p className="mt-1.5 flex flex-wrap gap-x-3 text-caption text-ink-2">
          <span className="font-semibold">Fixed</span>
          {row.fixedWords.map((f, i) => (
            <span key={i}>
              <span className="text-ink-3 line-through">{f.heard}</span> to {f.meant}
            </span>
          ))}
        </p>
      )}
      <div className="mt-2 flex min-h-5 flex-wrap items-center gap-x-3 text-caption text-ink-3 tabular-nums">
        <span className="flex min-w-0 items-center gap-1.5 text-ink-2" title={row.appTitle ?? undefined}>
          <AppIcon app={row.appName} />
          <span className="truncate">{appLabel(row.appName)}</span>
        </span>
        <span>{time ?? formatWhen(row.createdAt)}</span>
        <span>{plural(row.wordCount, "word", "words")}</span>
        <span>{formatDuration(row.audioMs)}</span>
        {row.language && <span>{languageName(row.language)}</span>}
        <span className="ml-auto flex gap-3 text-ink-2 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
          <button onClick={() => void copy()} className={linkClass}>
            <span key={copied ? "copied" : "copy"} className={copied ? "animate-pop font-semibold text-ink" : ""}>
              {copied ? "Copied" : "Copy"}
            </span>
          </button>
          {onDelete && (
            <button onClick={onDelete} className={linkClass}>
              Delete
            </button>
          )}
        </span>
      </div>
    </article>
  );
}
