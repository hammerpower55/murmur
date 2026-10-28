import type { ModelInfo, Progress } from "../lib/api";
import { formatBytes, formatRam, percent } from "../lib/format";

function Dots({ label, value }: { label: string; value: number }) {
  return (
    <div className="flex items-center gap-2" aria-label={`${label} ${value} of 5`}>
      <span className="w-16 text-xs text-zinc-500">{label}</span>
      <span className="flex gap-1">
        {[1, 2, 3, 4, 5].map((i) => (
          <span
            key={i}
            className={`h-1.5 w-1.5 rounded-full ${i <= value ? "bg-zinc-100" : "bg-zinc-700"}`}
          />
        ))}
      </span>
    </div>
  );
}

const button =
  "rounded-md px-3 py-1.5 text-sm font-medium transition-colors disabled:cursor-default disabled:opacity-40";
const primary = `${button} bg-zinc-100 text-zinc-900 hover:bg-white`;
const secondary = `${button} border border-white/10 text-zinc-200 hover:bg-white/5`;

interface Props {
  model: ModelInfo;
  active: boolean;
  loading: boolean;
  progress?: Progress;
  error?: string;
  onDownload: () => void;
  onCancel: () => void;
  onUse: () => void;
  onDelete: () => void;
  /** Onboarding shows a lone card without the delete action. */
  compact?: boolean;
}

export function ModelCard(p: Props) {
  const { model } = p;
  const downloading = p.progress !== undefined;

  return (
    <article
      className={`rounded-xl border bg-zinc-900/60 p-4 ${p.active ? "border-zinc-400/40" : "border-white/8"}`}
    >
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="truncate text-sm font-semibold text-zinc-100">{model.name}</h3>
            {p.active && (
              <span className="rounded-full bg-zinc-100 px-2 py-0.5 text-[11px] font-medium text-zinc-900">
                Active
              </span>
            )}
            {p.loading && <span className="text-xs text-zinc-400">Loading</span>}
          </div>
          <p className="mt-1 text-xs text-zinc-500">
            {model.family === "whisper" ? "Whisper" : "Parakeet"} · {formatBytes(model.sizeBytes)}
            {model.ramMb > 0 && ` · ${formatRam(model.ramMb)}`} · {model.languagesLabel}
          </p>
          {model.note && <p className="mt-1 text-xs text-amber-300/80">{model.note}</p>}
        </div>

        <div className="flex shrink-0 items-center gap-2">
          {downloading ? (
            <button className={secondary} onClick={p.onCancel}>
              Cancel
            </button>
          ) : !model.installed ? (
            <button className={primary} onClick={p.onDownload}>
              Download
            </button>
          ) : (
            <>
              {!p.active && (
                <button className={primary} onClick={p.onUse} disabled={p.loading}>
                  Use
                </button>
              )}
              {!p.compact && (
                <button
                  className={secondary}
                  onClick={p.onDelete}
                  disabled={p.active || p.loading}
                  title={p.active ? "Switch to another model before deleting this one" : undefined}
                >
                  Delete
                </button>
              )}
            </>
          )}
        </div>
      </div>

      {model.accuracy > 0 && (
        <div className="mt-3 flex flex-wrap gap-x-6 gap-y-1">
          <Dots label="Accuracy" value={model.accuracy} />
          <Dots label="Speed" value={model.speed} />
        </div>
      )}

      {downloading && p.progress && (
        <div className="mt-3">
          <div className="h-1 overflow-hidden rounded-full bg-zinc-800">
            <div
              className="h-full rounded-full bg-zinc-100 transition-[width] duration-150"
              style={{ width: `${percent(p.progress.done, p.progress.total)}%` }}
            />
          </div>
          <p className="mt-1.5 text-xs text-zinc-500">
            {formatBytes(p.progress.done)} of {formatBytes(p.progress.total)}
          </p>
        </div>
      )}

      {p.error && <p className="mt-3 text-xs text-red-300">{p.error}</p>}
    </article>
  );
}
