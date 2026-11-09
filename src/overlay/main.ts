// The floating orb. Rust decides what to show and when (overlay-phase) and
// streams the mic level (audio-level); this page only draws. Everything moves
// through the grid of dots, never a transform on the canvas, so it stays crisp.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Orb, REST_RADIUS } from "./orb";
import { Wave, approach, flowRate, palette, targetScale, type OrbPhase } from "./wave";
import "./overlay.css";

type Phase = "hidden" | OrbPhase;

const LEAVE_MS = 180;
/** The size the ball grows from when it appears and shrinks to when it leaves. */
const SMALL_SCALE = 0.45;
/** How quickly it follows, milliseconds: the voice closely, phases softly. */
const FOLLOW_VOICE_MS = 70;
const FOLLOW_PHASE_MS = 150;
const FOLLOW_LEAVE_MS = 70;
const FOLLOW_STATE_MS = 110;

const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

const pill = document.getElementById("pill") as HTMLDivElement;
const canvas = document.getElementById("orb") as HTMLCanvasElement;
const orb = new Orb(canvas);

const wave = new Wave();
let phase: Phase = "hidden";
let frame = 0;
let leaveTimer = 0;
let last = 0;
let scale = SMALL_SCALE;
let level = 0;
let mute = 0;
let thinking = 0;
let done = 0;
let cancel = 0;
/** How far the pattern has moved. Keeps going between dictations. */
let flow = Math.random() * 100;

function draw(now: number) {
  const dt = last === 0 ? 16 : Math.min(100, now - last);
  last = now;
  const still = reducedMotion.matches;

  const voice = phase === "recording" ? wave.valueAt(now) : 0;
  level = approach(level, voice, dt, FOLLOW_VOICE_MS);

  if (phase === "hidden") {
    scale = approach(scale, SMALL_SCALE, dt, FOLLOW_LEAVE_MS);
  } else {
    const follow = phase === "recording" ? FOLLOW_VOICE_MS : FOLLOW_PHASE_MS;
    scale = approach(scale, targetScale(phase, level, now, still), dt, follow);
    if (!still) flow += (dt / 1000) * flowRate(phase, level);
  }
  mute = approach(mute, phase === "cancelled" ? 1 : 0, dt, FOLLOW_STATE_MS);
  thinking = approach(thinking, phase === "processing" ? 1 : 0, dt, FOLLOW_STATE_MS);
  done = approach(done, phase === "done" ? 1 : 0, dt, FOLLOW_STATE_MS);
  cancel = approach(cancel, phase === "cancelled" ? 1 : 0, dt, FOLLOW_STATE_MS);

  orb.draw({ flow, level, radius: REST_RADIUS * scale, mute, thinking, done, cancel });
  frame = requestAnimationFrame(draw);
}

function startDrawing() {
  if (frame !== 0) return;
  last = 0;
  frame = requestAnimationFrame(draw);
}

/** Nothing runs once the orb has gone. */
function stopDrawing() {
  cancelAnimationFrame(frame);
  frame = 0;
  orb.clear();
}

function setPhase(next: Phase) {
  window.clearTimeout(leaveTimer);
  const wasHidden = phase === "hidden";
  phase = next;

  if (next === "hidden") {
    pill.classList.remove("shown");
    leaveTimer = window.setTimeout(() => {
      stopDrawing();
      pill.dataset.phase = "hidden";
    }, LEAVE_MS);
    return;
  }

  pill.dataset.phase = next;
  pill.classList.add("shown");
  if (wasHidden) {
    orb.resize();
    scale = SMALL_SCALE;
    level = 0;
    mute = 0;
    thinking = 0;
    done = 0;
    cancel = 0;
  }
  if (next === "recording") wave.reset(performance.now());
  startDrawing();
}

interface Style {
  waveColour: string;
}

function applyStyle(style: Style) {
  orb.setPalette(palette(style.waveColour));
}

applyStyle({ waveColour: "#f6b83d" });
listen<Style>("overlay-style", (e) => applyStyle(e.payload));
invoke<Style>("overlay_style").then(applyStyle, () => {});
listen<Phase>("overlay-phase", (e) => setPhase(e.payload));
listen<{ rms: number; peak: number }>("audio-level", (e) => {
  wave.push(e.payload.rms, performance.now());
});
