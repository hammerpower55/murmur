import type { ModelInfo, Progress } from "../lib/api";
import { formatBytes, formatRam, percent } from "../lib/format";
import { linkClass, primaryClass } from "./controls";

export const FAMILY_NAMES: Record<ModelInfo["family"], string> = {
  whisper: "Whisper",
  parakeet: "Parakeet",
  moonshine: "Moonshine",
  sensevoice: "SenseVoice",
  fixup: "Word fixer",
};

/** A scale of five soft ticks. */
function Scale({ label, value, title }: { label: string; value: number; title: string }) {
  return (
    <div className="flex items-center gap-2" title={title} aria-label={`${label} ${value} of 5. ${title}`}>
      <span className="text-caption text-ink-3">{label}</span>
      <span className="flex gap-0.5" aria-hidden>
        {[1, 2, 3, 4, 5].map((i) => (
          <span
            key={i}
            className={`h-1.5 w-4 origin-left rounded-full ${i <= value ? "bg-ink" : "bg-surface-2"}`}
            style={{ animation: `grow-across 420ms var(--ease-out-expo) ${i * 45}ms both` }}
          />
        ))}
      </span>
    </div>
  );
}

/** "About 0.2 s for a 5 s clip on Apple M2, estimated". */
export function speedTitle(model: ModelInfo, device = "this PC"): string {
  if (model.clipMs <= 0) return `Speed on ${device} is not known`;
  const s = model.clipMs / 1000;
  const time = s < 1 ? s.toFixed(1) : s < 10 ? s.toFixed(1).replace(/\.0$/, "") : Math.round(s).toString();
  return `About ${time} s for a 5 s clip on ${device}${model.measured ? "" : ", estimated"}`;
}

function Tag({ children }: { children: string }) {
  return <span className="animate-pop rounded-full bg-accent px-2 text-caption font-semibold text-accent-ink">{children}</span>;
}

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
  /** Onboarding shows a lone model without the delete action. */
  compact?: boolean;
  /** The chip speeds are for, such as "Apple M2 Pro". */
  device?: string;
  /** The word fixer: whether it is switched on and this is the one in use. */
  on?: boolean;
}

export function ModelCard(p: Props) {
  const { model } = p;
  const downloading = p.progress !== undefined;

  return (
    <article id={`model-${model.id}`} className="py-4">
      <div className="flex items-start justify-between gap-6">
        <div className="min-w-0">
          <div className="flex min-h-6 flex-wrap items-center gap-x-2">
            <h3 className="text-[1.125rem] leading-snug font-bold tracking-tight">{model.name}</h3>
            {model.family === "fixup" && model.installed && p.on && <Tag>On</Tag>}
            {p.active && <Tag>Active</Tag>}
            {p.loading && <span className="animate-breathe text-caption text-accent-text">Loading</span>}
          </div>
          <p className="mt-0.5 flex flex-wrap gap-x-3 text-caption text-ink-2 tabular-nums">
            {p.compact && <span>{FAMILY_NAMES[model.family]}</span>}
            <span>{formatBytes(model.sizeBytes)}</span>
            {model.ramMb > 0 && <span>{formatRam(model.ramMb)}</span>}
            <span>{model.languagesLabel}</span>
          </p>
        </div>

        <div className="flex shrink-0 items-center gap-4">
          {downloading ? (
            <button className={linkClass} onClick={p.onCancel}>
              Cancel
            </button>
          ) : !model.installed ? (
            <button className={primaryClass} onClick={p.onDownload}>
              Download
            </button>
          ) : (
            <>
              {!p.compact && (
                <button
                  className={linkClass}
                  onClick={p.onDelete}
                  disabled={p.active || p.loading}
                  title={p.active ? "Switch to another model before deleting this one" : undefined}
                >
                  Delete
                </button>
              )}
              {!p.active && model.family !== "fixup" && (
                <button className={primaryClass} onClick={p.onUse} disabled={p.loading}>
                  Use
                </button>
              )}
            </>
          )}
        </div>
      </div>

      {model.accuracy > 0 && (
        <div className="mt-2 flex flex-wrap gap-x-6 gap-y-1">
          <Scale label="Accuracy" value={model.accuracy} title="Accuracy on English" />
          <Scale label="Speed" value={model.speed} title={speedTitle(model, p.device)} />
        </div>
      )}

      {model.note && <p className="mt-2 max-w-[68ch] text-caption text-accent-text">{model.note}</p>}

      {downloading && p.progress && (
        <div className="mt-3 animate-pop">
          <div className="h-1.5 overflow-hidden rounded-full bg-surface-2">
            <div
              className="h-full origin-left rounded-full bg-accent transition-transform duration-300 ease-out"
              style={{ transform: `scaleX(${percent(p.progress.done, p.progress.total) / 100})` }}
            />
          </div>
          <p className="mt-1.5 text-caption text-ink-2 tabular-nums">
            {formatBytes(p.progress.done)} of {formatBytes(p.progress.total)}
          </p>
        </div>
      )}

      {p.error && (
        <p key={p.error} className="mt-2 animate-shake text-caption text-danger">
          {p.error}
        </p>
      )}
    </article>
  );
}
