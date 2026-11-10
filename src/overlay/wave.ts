// How the voice and the phase move the orb, and the colours it is made of. Kept
// free of the DOM so it can be unit tested.

/** Exponential smoothing per level event: quick to rise, slower to fall. */
export const ATTACK = 0.55;
export const RELEASE = 0.3;
/** Level events arrive this often. */
export const EVENT_MS = 50;

/** Levels at or below this (room noise) leave the orb at rest. */
export const GATE = 0.06;
/** Below 1 lifts quiet speech so it still moves the orb. */
export const CURVE = 0.55;

/** How much a full voice grows the ball. */
export const SWELL = 0.36;
/** How far it breathes at rest, and how long a breath takes. */
export const BREATH = 0.03;
export const BREATH_MS = 2600;
/** How fast the dots' pattern moves, per second: slow at rest, stirred by a voice. */
export const FLOW_REST = 0.42;
export const FLOW_VOICE = 2.1;
export const FLOW_THINKING = 1.1;
/** Transcribing: smaller, and pulsing this fast. */
export const THINK_RADIUS = 0.78;
export const THINK_PULSE = 0.1;
export const THINK_MS = 1200;
export const CANCEL_RADIUS = 0.74;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** Maps a mic level (0 to 1, from -50 to -10 dBFS) onto how far the orb moves. */
export function shape(level: number): number {
  const v = clamp((level - GATE) / (1 - GATE), 0, 1);
  return Math.pow(v, CURVE);
}

export type OrbPhase = "recording" | "processing" | "done" | "cancelled" | "error";

/** How big the ball wants to be right now, as a multiple of its resting size.
 * `still` (reduced motion) drops breathing and pulsing but keeps the voice,
 * because that is the feedback the orb exists to give. */
export function targetScale(phase: OrbPhase, value: number, now: number, still = false): number {
  switch (phase) {
    case "recording": {
      const breath = still ? 0 : BREATH * Math.sin((now / BREATH_MS) * 2 * Math.PI);
      // Breathing fades out as the voice takes over.
      return 1 + breath * (1 - value) + SWELL * value;
    }
    case "processing":
      return still ? THINK_RADIUS : THINK_RADIUS + THINK_PULSE * Math.sin((now / THINK_MS) * 2 * Math.PI);
    case "cancelled":
      return CANCEL_RADIUS;
    default:
      return 1;
  }
}

export function flowRate(phase: OrbPhase, value: number): number {
  if (phase === "recording") return FLOW_REST + (FLOW_VOICE - FLOW_REST) * value;
  if (phase === "processing") return FLOW_THINKING;
  return FLOW_REST;
}

/** Glides `current` towards `target`, the same whatever the frame rate. */
export function approach(current: number, target: number, dtMs: number, timeMs: number): number {
  return target + (current - target) * Math.exp(-dtMs / timeMs);
}

type Rgb = [number, number, number];

export interface Palette {
  gold: Rgb;
  /** The chosen colour. */
  base: Rgb;
  coral: Rgb;
  rose: Rgb;
  /** The brightest dots: the tick and cross. */
  pale: Rgb;
}

const toLinear = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const toGamma = (c: number) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);

/** A colour in OKLCH: lightness, chroma, hue in degrees. */
function hexToOklch(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  const r = toLinear(((n >> 16) & 255) / 255);
  const g = toLinear(((n >> 8) & 255) / 255);
  const b = toLinear((n & 255) / 255);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  return [L, Math.hypot(A, B), (Math.atan2(B, A) * 180) / Math.PI];
}

function oklchToRgb(L: number, C: number, H: number): Rgb {
  const h = (H * Math.PI) / 180;
  const A = C * Math.cos(h);
  const B = C * Math.sin(h);
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3;
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3;
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ].map((c) => clamp(toGamma(clamp(c, 0, 1)), 0, 1)) as Rgb;
}

/** The dots' gradient, drawn from the one colour the user picks, so any colour
 * works. Amber runs through coral to rose; the hue never turns green. */
export function palette(hex: string): Palette {
  const [l, c, h] = hexToOklch(hex);
  return {
    gold: oklchToRgb(Math.min(0.95, l + 0.07), c, h + 8),
    base: oklchToRgb(l, c, h),
    coral: oklchToRgb(Math.max(0.3, l - 0.08), c * 1.25, h - 44),
    rose: oklchToRgb(Math.max(0.3, l - 0.14), c * 1.3, h - 82),
    pale: oklchToRgb(Math.min(0.99, l + 0.16), c * 0.3, h + 12),
  };
}

/**
 * A smoothed level that can be read at any moment. Each event moves the
 * smoothed target; frames in between interpolate towards it so the orb glides
 * instead of stepping.
 */
export class Wave {
  private from = 0;
  private to = 0;
  private at = 0;

  reset(now: number) {
    this.from = 0;
    this.to = 0;
    this.at = now;
  }

  /** A level event, 0 to 1. */
  push(level: number, now: number) {
    const target = shape(level);
    const alpha = target > this.to ? ATTACK : RELEASE;
    this.from = this.valueAt(now);
    this.to = alpha * target + (1 - alpha) * this.to;
    this.at = now;
  }

  valueAt(now: number): number {
    const k = clamp((now - this.at) / EVENT_MS, 0, 1);
    return this.from + (this.to - this.from) * k;
  }
}
