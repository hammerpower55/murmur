// Insights cache (spec 6.7): computed in SQL on the Rust side, kept here,
// refreshed whenever a transcript is added or the typing speed changes.

import { create } from "zustand";
import { api, events, type Insights } from "../lib/api";

interface InsightsState {
  data: Insights | null;
  refresh: () => Promise<void>;
}

export const useInsights = create<InsightsState>((set) => ({
  data: null,
  refresh: async () => set({ data: await api.insights() }),
}));

let subscribed = false;

export function subscribeInsights() {
  if (subscribed) return;
  subscribed = true;
  const { refresh } = useInsights.getState();
  void refresh();
  void events.transcriptAdded(() => void refresh());
  void events.insightsChanged(() => void refresh());
}
