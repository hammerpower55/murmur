// The page's bones: the full stop, the panel at the top of every page, and the
// hanging section that lines everything up on the one vertical axis.

import type { ReactNode } from "react";
import { useDictation } from "../store/dictation";

/** Murmur's full stop. It breathes while Murmur listens or transcribes. */
export function Stop() {
  const activity = useDictation((d) => d.activity);
  return (
    <span
      aria-hidden
      className={`ml-[0.05em] inline-block h-[0.17em] w-[0.17em] rounded-full bg-accent ${
        activity === "idle" ? "" : "animate-breathe"
      }`}
    />
  );
}

export type Tone = "plain" | "invert" | "amber";

const TONE_CLASS: Record<Tone, string> = {
  plain: "",
  invert: "tone-invert",
  amber: "tone-amber",
};

interface TitleProps {
  children: ReactNode;
  /** Controls or a date, on the right of the title. */
  aside?: ReactNode;
  tone?: Tone;
  /** A chat needs the height more than a big title block. */
  compact?: boolean;
  /** Home and first run: the panel is the page. */
  hero?: boolean;
  /** Under the title, inside the panel. */
  below?: ReactNode;
  /** Runs edge to edge along the foot of the panel. Give it its height from the
   * first frame, before its data arrives, so the panel never changes size. */
  foot?: ReactNode;
}

/** The masthead: a big rounded panel with the title on it, rising from behind
 * its own baseline. */
export function Title({ children, aside, tone = "plain", compact, hero, below, foot }: TitleProps) {
  return (
    <header
      className={`masthead ${TONE_CLASS[tone]} ${compact ? "masthead-compact" : ""} ${hero ? "masthead-hero" : ""} ${
        foot !== undefined ? "pb-0!" : ""
      }`}
    >
      <div className="masthead-head">
        <h1 className="title">
          <span className="title-clip">
            <span className="title-line">
              {children}
              {/* A word joiner, so the full stop never wraps away from its word. */}
              {"⁠"}
              <Stop />
            </span>
          </span>
        </h1>
        {aside && <div className="masthead-aside">{aside}</div>}
      </div>
      {below}
      {foot !== undefined && <div className="-mx-5 mt-6">{foot}</div>}
    </header>
  );
}

/** A section on the axis: a heading in the margin, its content to the right. */
export function Hang({
  label,
  note,
  className = "",
  children,
}: {
  label?: ReactNode;
  /** Sits under the heading, such as a link to more. */
  note?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section className={`hang ${className}`}>
      <div className="hang-label">
        {label && <h2>{label}</h2>}
        {note && <div className="mt-1 font-normal">{note}</div>}
      </div>
      <div className="hang-body">{children}</div>
    </section>
  );
}
