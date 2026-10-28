import type { ReactNode } from "react";
import { useDictation } from "../store/dictation";
import { Wave } from "./Wave";

/** An empty state that listens: its wave comes alive the moment you dictate. */
export function Empty({ children }: { children: ReactNode }) {
  const activity = useDictation((d) => d.activity);
  return (
    <div className="flex animate-rise flex-col items-center gap-5 py-14 text-center">
      <span className={activity === "idle" ? "animate-breathe" : ""}>
        <Wave activity={activity} size="lg" />
      </span>
      <p className="text-sm text-zinc-400">{children}</p>
    </div>
  );
}
