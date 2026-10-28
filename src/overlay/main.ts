// The floating pill. Rust decides what to show and when (overlay-phase) and
// streams the mic level (audio-level); this page only draws.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { BAR_COUNT, MAX_HEIGHT, Wave } from "./wave";
import "./overlay.css";

type Phase = "hidden" | "recording" | "processing" | "done" | "cancelled" | "error";

/** Below this the bars rest and breathe; above IDLE_EXIT they follow the voice. */
const IDLE_ENTER = 0.03;
const IDLE_EXIT = 0.06;
const LEAVE_MS = 140;

const pill = document.getElementById("pill") as HTMLDivElement;
const barsEl = document.getElementById("bars") as HTMLDivElement;
const bars: HTMLDivElement[] = Array.from({ length: BAR_COUNT }, () => {
  const bar = document.createElement("div");
  bar.className = "bar";
  barsEl.appendChild(bar);
  return bar;
});

const wave = new Wave();
let frame = 0;
let idle = true;
let leaveTimer = 0;

function draw(now: number) {
  const v = wave.sample(now);
  if (idle && v > IDLE_EXIT) idle = false;
  else if (!idle && v < IDLE_ENTER) idle = true;
  barsEl.classList.toggle("idle", idle);

  const heights = wave.heights(now);
  for (let i = 0; i < BAR_COUNT; i++) {
    bars[i].style.transform = `scaleY(${heights[i] / MAX_HEIGHT})`;
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
  barColour: string;
}

function applyStyle(style: Style) {
  document.documentElement.style.setProperty("--bar-colour", style.barColour);
}

listen<Style>("overlay-style", (e) => applyStyle(e.payload));
invoke<Style>("overlay_style").then(applyStyle, () => {});
listen<Phase>("overlay-phase", (e) => setPhase(e.payload));
listen<{ rms: number; peak: number }>("audio-level", (e) => {
  wave.push(e.payload.rms, performance.now());
});
