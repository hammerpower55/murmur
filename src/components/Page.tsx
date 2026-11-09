// The page's bones: the full stop, the poster at the top of every page, and the
// hanging section that lines everything up on the one vertical axis.

import type { ReactNode } from "react";
import { useDictation } from "../store/dictation";
import { Rings } from "./Rings";

/** Murmur's full stop. It breathes while Murmur listens or transcribes, and
 * with `land` it drops into place after the words it ends. */
export function Stop({ land }: { land?: boolean }) {
  const activity = useDictation((d) => d.activity);
  return (
    <span
      aria-hidden
      className={`ml-[0.05em] inline-block h-[0.17em] w-[0.17em] rounded-full bg-accent ${
        activity === "idle" ? (land ? "stop-land" : "") : "animate-breathe"
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
  /** Which picture the rings draw. */
  seed?: number;
  /** A chat needs the height more than a poster. */
  compact?: boolean;
  /** Home and first run: the poster is the page. */
  hero?: boolean;
  /** Under the title, inside the field. */
  below?: ReactNode;
  /** Runs edge to edge along the foot of the field. */
  foot?: ReactNode;
}

/** The masthead: a colour field across the top of the page with the title on
 * it, rising from behind its own baseline, and the rings behind. */
export function Title({ children, aside, tone = "plain", seed = 1, compact, hero, below, foot }: TitleProps) {
  return (
    <header
      className={`masthead ${TONE_CLASS[tone]} ${compact ? "masthead-compact" : ""} ${hero ? "masthead-hero" : ""} ${
        foot ? "pb-0!" : ""
      }`}
    >
      <Rings
        seed={seed}
        className={
          hero
            ? "top-1/2 -right-40 w-[34rem] -translate-y-1/2"
            : compact
              ? "top-1/2 -right-16 w-[20rem] -translate-y-1/2"
              : "top-1/2 -right-28 w-[28rem] -translate-y-1/2"
        }
      />
      <div className="masthead-head">
        <h1 className="title">
          <span className="title-clip">
            <span className="title-line">
              {children}
              {/* A word joiner, so the full stop never wraps away from its word. */}
              {"\u2060"}
              <Stop land />
            </span>
          </span>
        </h1>
        {aside && <div className="masthead-aside">{aside}</div>}
      </div>
      {below}
      {foot && <div className="-mx-8 mt-6">{foot}</div>}
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
