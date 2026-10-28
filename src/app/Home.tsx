import { useEffect, useState } from "react";
import { TranscriptItem } from "../components/TranscriptItem";
import { api, events, type HomeSummary } from "../lib/api";
import { plural } from "../lib/format";

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-white/8 bg-zinc-900/60 px-4 py-3">
      <p className="text-xs text-zinc-500">{label}</p>
      <p className="mt-1 text-2xl font-semibold text-zinc-100 tabular-nums">{value}</p>
    </div>
  );
}

export function Home({ onSeeHistory }: { onSeeHistory: () => void }) {
  const [summary, setSummary] = useState<HomeSummary | null>(null);

  useEffect(() => {
    const load = () => void api.homeSummary().then(setSummary);
    load();
    const unlisten = events.transcriptAdded(load);
    return () => void unlisten.then((f) => f());
  }, []);

  if (!summary) return null;

  return (
    <section className="mx-auto max-w-3xl px-6 py-8">
      <h1 className="text-xl font-semibold text-zinc-100">Home</h1>

      <div className="mt-6 grid grid-cols-2 gap-3">
        <Stat label="Words this week" value={summary.wordsThisWeek.toLocaleString("en-US")} />
        <Stat label="Streak" value={plural(summary.streakDays, "day", "days")} />
      </div>

      <div className="mt-8 flex items-baseline justify-between">
        <h2 className="text-sm font-medium text-zinc-300">Recent</h2>
        {summary.recent.length > 0 && (
          <button onClick={onSeeHistory} className="text-xs text-zinc-400 hover:text-zinc-200">
            See all
          </button>
        )}
      </div>
      <div className="mt-3 flex flex-col gap-3">
        {summary.recent.length === 0 ? (
          <p className="py-10 text-center text-sm text-zinc-500">Hold Ctrl and Win, speak, then let go.</p>
        ) : (
          summary.recent.map((r) => <TranscriptItem key={r.id} row={r} />)
        )}
      </div>
    </section>
  );
}
