// What the dictation thread is doing, from its `dictation-state` events, so
// the window can show that Murmur is listening.

import { create } from "zustand";
import { events } from "../lib/api";

export type Activity = "idle" | "recording" | "processing";

export function activityOf(state: string): Activity {
  if (state === "recording") return "recording";
  if (state === "processing") return "processing";
  return "idle";
}

export const useDictation = create<{ activity: Activity }>(() => ({ activity: "idle" }));

let subscribed = false;

export function subscribeDictation() {
  if (subscribed) return;
  subscribed = true;
  void events.dictationState((state) => useDictation.setState({ activity: activityOf(state) }));
}
