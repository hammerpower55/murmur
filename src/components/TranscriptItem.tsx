import { useState } from "react";
import { type TranscriptRow } from "../lib/api";
import { useAppIcon } from "../lib/icons";
import { appLabel, formatDuration, formatWhen, plural } from "../lib/format";
import { languageName } from "../lib/languages";

function AppIcon({ app }: { app: string | null }) {
  const src = useAppIcon(app);
  return src ? (
    <img src={src} alt="" className="h-4 w-4 shrink-0" />
  ) : (
    <span className="h-4 w-4 shrink-0 rounded bg-zinc-700" />
  );
}

const FRESH_MS = 8000;

interface Props {
  row: TranscriptRow;
  onDelete?: () => void;
}

export function TranscriptItem({ row, onDelete }: Props) {
  const [copied, setCopied] = useState(false);
  // Dictated in the last few seconds: greet it with a soft ring, once.
  const [fresh] = useState(() => Date.now() - new Date(row.createdAt).getTime() < FRESH_MS);

  const copy = async () => {
    await navigator.clipboard.writeText(row.finalText);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  };

  return (
    <article className={`surface lift group rounded-xl px-5 py-4 ${fresh ? "fresh" : ""}`}>
      <div className="flex items-center gap-2 text-xs text-zinc-500">
        <AppIcon app={row.appName} />
        <span className="truncate text-zinc-400" title={row.appTitle ?? undefined}>
          {appLabel(row.appName)}
        </span>
        <span>·</span>
        <span>{formatWhen(row.createdAt)}</span>
        <span>·</span>
        <span>{plural(row.wordCount, "word", "words")}</span>
        <span>·</span>
        <span>{formatDuration(row.audioMs)}</span>
        {row.language && (
          <>
            <span>·</span>
            <span>{languageName(row.language)}</span>
          </>
        )}
        <span className="ml-auto flex gap-1 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
          <button
            onClick={() => void copy()}
            className="rounded px-2 py-0.5 text-zinc-300 hover:bg-white/8"
          >
            <span key={copied ? "copied" : "copy"} className={copied ? "inline-block animate-pop text-accent" : ""}>
              {copied ? "Copied" : "Copy"}
            </span>
          </button>
          {onDelete && (
            <button onClick={onDelete} className="rounded px-2 py-0.5 text-zinc-300 hover:bg-white/8">
              Delete
            </button>
          )}
        </span>
      </div>
      <p className="mt-2 whitespace-pre-wrap text-sm leading-relaxed text-zinc-100">{row.finalText}</p>
      {row.fixedWords.length > 0 && (
        <p className="mt-2 flex flex-wrap items-center gap-1.5 text-xs text-zinc-500">
          <span className="text-accent">Fixed</span>
          {row.fixedWords.map((f, i) => (
            <span key={i} className="rounded-full border border-white/8 px-2 py-0.5">
              <span className="text-zinc-500 line-through decoration-zinc-600">{f.heard}</span>
              <span className="px-1 text-zinc-600">to</span>
              <span className="text-zinc-200">{f.meant}</span>
            </span>
          ))}
        </p>
      )}
    </article>
  );
}
