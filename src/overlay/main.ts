// The floating pill. Rust decides what to show and when (overlay-phase) and
// streams the mic level (audio-level); this page only draws.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { BAR_COUNT, MAX_HEIGHT, Wave } from "./wave";
import "./overlay.css";

type Phase = "hidden" | "recording" | "processing" | "done" | "cancelled" | "error";

/** Below this the wave rests and breathes; above IDLE_EXIT it follows the voice. */
const IDLE_ENTER = 0.03;
const IDLE_EXIT = 0.06;
const LEAVE_MS = 140;
/** Hue drift across the wave, degrees: deeper amber on the left to gold on
 * the right, kept on the warm side so the right end never turns green. */
const HUE_FROM = -20;
const HUE_TO = 2;
/** Each dot of the Transcribing row starts this much after its neighbour. */
const THINK_STEP_MS = 55;

const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

const pill = document.getElementById("pill") as HTMLDivElement;

function makeBars(parent: HTMLElement, each?: (bar: HTMLDivElement, i: number) => void) {
  return Array.from({ length: BAR_COUNT }, (_, i) => {
    const bar = document.createElement("div");
    bar.className = "bar";
    bar.style.setProperty("--shift", String(HUE_FROM + ((HUE_TO - HUE_FROM) * i) / (BAR_COUNT - 1)));
    each?.(bar, i);
    parent.appendChild(bar);
    return bar;
  });
}

const bars = makeBars(document.getElementById("bars") as HTMLDivElement);
makeBars(document.getElementById("thinking") as HTMLDivElement, (bar, i) => {
  bar.style.animationDelay = `${i * THINK_STEP_MS}ms`;
});

const wave = new Wave();
let frame = 0;
let idle = true;
let leaveTimer = 0;

function draw(now: number) {
  const still = reducedMotion.matches;
  const v = wave.sample(now);
  if (idle && v > IDLE_EXIT) idle = false;
  else if (!idle && v < IDLE_ENTER) idle = true;

  // Resting dots breathe on a 1.2 s cycle; a voice lights the wave up.
  const rest = still ? 0.55 : 0.5 + 0.15 * Math.sin((now / 1200) * 2 * Math.PI);
  const levels = wave.levels(now, still);
  const heights = wave.heights(now, still);
  for (let i = 0; i < BAR_COUNT; i++) {
    bars[i].style.transform = `scaleY(${heights[i] / MAX_HEIGHT})`;
    bars[i].style.opacity = idle ? String(rest) : String(Math.min(1, 0.55 + levels[i] * 1.2));
  }
  frame = requestAnimationFrame(draw);
}

function startDrawing() {
  cancelAnimationFrame(frame);
  wave.reset(performance.now());
  idle = true;
  frame = requestAnimationFrame(draw);
}

function stopDrawing() {
  cancelAnimationFrame(frame);
  frame = 0;
}

function setPhase(phase: Phase) {
  window.clearTimeout(leaveTimer);
  if (phase === "hidden") {
    stopDrawing();
    pill.classList.remove("shown");
    pill.classList.add("leaving");
    leaveTimer = window.setTimeout(() => {
      pill.classList.remove("leaving");
      pill.dataset.phase = "hidden";
    }, LEAVE_MS);
    return;
  }

  pill.dataset.phase = phase;
  if (!pill.classList.contains("shown")) {
    pill.classList.remove("leaving");
    // Restart from the resting state so the appear animation always runs.
    void pill.offsetWidth;
    pill.classList.add("shown");
  }
  if (phase === "recording") startDrawing();
  else stopDrawing();
}

interface Style {
  waveColour: string;
}

function applyStyle(style: Style) {
  document.documentElement.style.setProperty("--wave", style.waveColour);
}

listen<Style>("overlay-style", (e) => applyStyle(e.payload));
invoke<Style>("overlay_style").then(applyStyle, () => {});
listen<Phase>("overlay-phase", (e) => setPhase(e.payload));
listen<{ rms: number; peak: number }>("audio-level", (e) => {
  wave.push(e.payload.rms, performance.now());
});
