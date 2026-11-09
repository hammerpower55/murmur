// The orb: a ball of LED dots on a square grid, drawn on a 2D canvas. Each dot's
// size and brightness come from a soft pattern moving over a sphere, so the ball
// still reads as round, with a colour gradient sweeping across it. The maths is
// free of the DOM so it can be unit tested; `Orb` at the end only draws.

import type { Palette } from "./wave";

/** Distance between dots, CSS pixels. Rounded to whole device pixels when drawn,
 * so every dot lands the same way and the grid stays crisp. */
export const PITCH = 5;
/** The canvas, CSS pixels, matching `overlay.css`. */
export const CANVAS = 84;
/** The ball's radius at rest, CSS pixels: 56 across. */
export const REST_RADIUS = 28;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const mix = (a: number, b: number, t: number) => a + (b - a) * t;
const smoothstep = (lo: number, hi: number, v: number) => {
  const t = clamp((v - lo) / (hi - lo), 0, 1);
  return t * t * (3 - 2 * t);
};

export interface Dot {
  x: number;
  y: number;
}

/** The grid, centred on a dot, out to `radius`. */
export function gridDots(pitch: number, radius: number): Dot[] {
  const dots: Dot[] = [];
  const n = Math.ceil(radius / pitch);
  for (let row = -n; row <= n; row++) {
    for (let col = -n; col <= n; col++) {
      const x = col * pitch;
      const y = row * pitch;
      if (Math.hypot(x, y) <= radius) dots.push({ x, y });
    }
  }
  return dots;
}

/**
 * How lit a dot is, 0 to 1. `nx` and `ny` are its place in the ball, -1 to 1.
 * `flow` is how far the pattern has moved and `level` how loud the voice is.
 */
export function dotLight(nx: number, ny: number, flow: number, level: number): number {
  const r = Math.min(1, Math.hypot(nx, ny));
  const z = Math.sqrt(1 - r * r);

  // A soft pattern on the surface of a slowly turning sphere.
  const turn = flow * 0.35;
  const sx = nx * Math.cos(turn) + z * Math.sin(turn);
  const sz = z * Math.cos(turn) - nx * Math.sin(turn);
  const a = Math.sin(3.1 * sx + flow * 1.1 + 1.7 * Math.sin(2.3 * ny + flow * 0.7));
  const b = Math.sin(2.7 * ny - flow * 0.9 + 1.5 * Math.sin(2.9 * sz - flow * 0.6));
  const pattern = 0.5 + 0.25 * a + 0.25 * b;

  // Rings of light running out from the middle, as loud as the voice.
  const ripple = 0.5 + 0.5 * Math.sin(r * 9 - flow * 5);
  const lit = mix(pattern, ripple, 0.2 + 0.5 * level);

  // Biggest in the middle and smaller towards the edge: that is the roundness.
  const sphere = 0.42 + 0.58 * z;
  return clamp(sphere * (0.32 + 0.68 * lit) + 0.14 * level, 0, 1);
}

/** Transcribing: a comet of light running around the ball. 1 at its head. */
export function sweepLight(nx: number, ny: number, flow: number): number {
  const angle = Math.atan2(ny, nx);
  // How far behind the head this dot is, 0 to one full turn.
  const behind = (((flow * 3.2 - angle) % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI);
  return Math.exp(-behind * 0.9) * smoothstep(0.15, 0.7, Math.hypot(nx, ny));
}

type Point = [number, number];

/** The tick and cross, as lines counted in dots from the middle of the ball, y
 * down. Every line is an exact diagonal, so it lands on dots and reads cleanly. */
export const TICK: Point[][] = [[[-3, -1], [-1, 1], [3, -3]]];
export const CROSS: Point[][] = [
  [[-2, -2], [2, 2]],
  [[2, -2], [-2, 2]],
];

function distanceToSegment(x: number, y: number, a: Point, b: Point): number {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const t = clamp(((x - a[0]) * dx + (y - a[1]) * dy) / (dx * dx + dy * dy), 0, 1);
  return Math.hypot(x - a[0] - t * dx, y - a[1] - t * dy);
}

/** Whether a dot belongs to a glyph drawn in the grid's own dots. */
export function onGlyph(x: number, y: number, glyph: Point[][], pitch: number): boolean {
  for (const line of glyph) {
    for (let i = 0; i + 1 < line.length; i++) {
      if (distanceToSegment(x / pitch, y / pitch, line[i], line[i + 1]) < 0.3) return true;
    }
  }
  return false;
}

type Rgb = [number, number, number];

/** The gradient across the ball: gold, the chosen colour, coral, rose. */
export function gradientAt(palette: Palette, u: number): Rgb {
  const stops = [palette.gold, palette.base, palette.coral, palette.rose];
  const at = clamp(u, 0, 1) * (stops.length - 1);
  const i = Math.min(stops.length - 2, Math.floor(at));
  const t = at - i;
  return [
    mix(stops[i][0], stops[i + 1][0], t),
    mix(stops[i][1], stops[i + 1][1], t),
    mix(stops[i][2], stops[i + 1][2], t),
  ];
}

const css = ([r, g, b]: Rgb) => `rgb(${Math.round(r * 255)} ${Math.round(g * 255)} ${Math.round(b * 255)})`;

const STEPS = 48;
const MAX_PIXEL_RATIO = 3;
/** The dark ball the dots are set in, so they read over any window. */
const BODY = "#131110";

export interface OrbFrame {
  /** How far the pattern has moved. */
  flow: number;
  /** The voice, 0 to 1. */
  level: number;
  /** CSS pixels. */
  radius: number;
  /** 1 turns the dots grey. */
  mute: number;
  /** 1 runs the transcribing comet. */
  thinking: number;
  /** 1 shows the tick. */
  done: number;
  /** 1 shows the cross. */
  cancel: number;
}

export class Orb {
  private ctx: CanvasRenderingContext2D;
  private pitch = PITCH;
  private dots: (Dot & { tick: boolean; cross: boolean })[] = [];
  private colours: string[] = [];
  private pale = "#fff";
  private palette: Palette | null = null;

  constructor(private canvas: HTMLCanvasElement) {
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("no 2d canvas");
    this.ctx = ctx;
    this.resize();
  }

  resize() {
    const ratio = Math.min(MAX_PIXEL_RATIO, window.devicePixelRatio || 1);
    const size = Math.round(CANVAS * ratio);
    if (this.canvas.width !== size) {
      this.canvas.width = size;
      this.canvas.height = size;
    }
    this.ctx.setTransform(size / CANVAS, 0, 0, size / CANVAS, size / 2, size / 2);
    // A whole number of device pixels between dots keeps the grid even.
    this.pitch = Math.max(2, Math.round(PITCH * ratio)) / ratio;
    this.dots = gridDots(this.pitch, CANVAS / 2).map((d) => ({
      ...d,
      tick: onGlyph(d.x, d.y, TICK, this.pitch),
      cross: onGlyph(d.x, d.y, CROSS, this.pitch),
    }));
  }

  setPalette(palette: Palette) {
    this.palette = palette;
    this.colours = Array.from({ length: STEPS + 1 }, (_, i) => css(gradientAt(palette, i / STEPS)));
    this.pale = css(palette.pale);
  }

  draw(f: OrbFrame) {
    const { ctx, pitch } = this;
    ctx.clearRect(-CANVAS / 2, -CANVAS / 2, CANVAS, CANVAS);
    if (!this.palette) return;

    ctx.fillStyle = BODY;
    ctx.beginPath();
    ctx.arc(0, 0, f.radius, 0, Math.PI * 2);
    ctx.fill();

    const edge = f.radius - pitch * 0.5;
    const angle = f.flow * 0.5;
    const ax = Math.cos(angle);
    const ay = Math.sin(angle);
    const glyph = Math.max(f.done, f.cancel);

    for (const dot of this.dots) {
      const d = Math.hypot(dot.x, dot.y);
      if (d >= edge) continue;
      const nx = dot.x / f.radius;
      const ny = dot.y / f.radius;

      let light = dotLight(nx, ny, f.flow, f.level);
      if (f.thinking > 0) light = mix(light, 0.16 + 0.84 * sweepLight(nx, ny, f.flow), f.thinking * 0.85);
      const inGlyph = (f.done > 0 && dot.tick) || (f.cancel > 0 && dot.cross);
      light = inGlyph ? mix(light, 1, glyph) : light * (1 - 0.82 * glyph);

      // Dots shrink away at the rim, so a growing ball gains them smoothly.
      const rim = smoothstep(edge, edge - pitch * 1.4, d);
      const size = pitch * 0.5 * (0.24 + 0.72 * light) * rim;
      if (size < 0.12) continue;

      if (inGlyph && glyph > 0.5) {
        ctx.fillStyle = this.pale;
      } else if (f.mute > 0.01) {
        const grey = 0.62 * f.mute;
        const [r, g, b] = gradientAt(this.palette, 0.5 + 0.5 * (nx * ax + ny * ay));
        ctx.fillStyle = css([mix(r, grey, f.mute), mix(g, grey, f.mute), mix(b, grey, f.mute)]);
      } else {
        const u = 0.5 + 0.5 * (nx * ax + ny * ay);
        ctx.fillStyle = this.colours[Math.round(clamp(u, 0, 1) * STEPS)];
      }
      ctx.globalAlpha = 0.42 + 0.58 * light;
      ctx.beginPath();
      ctx.arc(dot.x, dot.y, size, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }

  clear() {
    this.ctx.clearRect(-CANVAS / 2, -CANVAS / 2, CANVAS, CANVAS);
  }
}
