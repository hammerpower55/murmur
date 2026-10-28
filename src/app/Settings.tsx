import { useState } from "react";
import { api, errorText } from "../lib/api";

/** Phase 6 needs only Privacy > Clear history; the full page arrives in Phase 8. */
export function Settings() {
  const [confirming, setConfirming] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  const clear = async () => {
    setConfirming(false);
    try {
      await api.clearHistory();
      setStatus("History cleared");
    } catch (err) {
      setStatus(errorText(err));
    }
  };

  return (
    <section className="mx-auto max-w-3xl px-6 py-8">
      <h1 className="text-xl font-semibold text-zinc-100">Settings</h1>

      <h2 className="mt-8 text-sm font-medium text-zinc-300">Privacy</h2>
      <div className="mt-3 flex items-center justify-between rounded-xl border border-white/8 bg-zinc-900/60 px-4 py-3">
        <span className="text-sm text-zinc-100">Clear history</span>
        {confirming ? (
          <span className="flex gap-2">
            <button
              onClick={() => setConfirming(false)}
              className="rounded-md border border-white/10 px-3 py-1.5 text-sm text-zinc-200 hover:bg-white/5"
            >
              Cancel
            </button>
            <button
              onClick={() => void clear()}
              className="rounded-md bg-red-400 px-3 py-1.5 text-sm font-medium text-zinc-950 hover:bg-red-300"
            >
              Delete every transcript
            </button>
          </span>
        ) : (
          <button
            onClick={() => {
              setStatus(null);
              setConfirming(true);
            }}
            className="rounded-md border border-white/10 px-3 py-1.5 text-sm text-zinc-200 hover:bg-white/5"
          >
            Clear
          </button>
        )}
      </div>
      {status && <p className="mt-2 text-sm text-zinc-400">{status}</p>}
    </section>
  );
}
