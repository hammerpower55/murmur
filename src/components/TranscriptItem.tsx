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

interface Props {
  row: TranscriptRow;
  onDelete?: () => void;
}

export function TranscriptItem({ row, onDelete }: Props) {
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    await navigator.clipboard.writeText(row.finalText);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  };

  return (
    <article className="group rounded-xl border border-white/8 bg-zinc-900/60 px-4 py-3">
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
            {copied ? "Copied" : "Copy"}
          </button>
          {onDelete && (
            <button onClick={onDelete} className="rounded px-2 py-0.5 text-zinc-300 hover:bg-white/8">
              Delete
            </button>
          )}
        </span>
      </div>
      <p className="mt-2 whitespace-pre-wrap text-sm leading-relaxed text-zinc-100">{row.finalText}</p>
    </article>
  );
}
