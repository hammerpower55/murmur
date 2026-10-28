import { useEffect, useState } from "react";
import { CountUp } from "../components/CountUp";
import { Empty } from "../components/Empty";
import { TranscriptItem } from "../components/TranscriptItem";
import { api, events, type HomeSummary } from "../lib/api";
import { plural } from "../lib/format";
import { useHotkeyText } from "../store/settings";

function Stat({ label, value, format }: { label: string; value: number; format: (n: number) => string }) {
  return (
    <div className="surface rounded-xl px-4 py-3">
      <p className="text-xs text-zinc-500">{label}</p>
      <p className="mt-1 text-2xl font-semibold text-zinc-100 tabular-nums">
        <CountUp value={value} format={format} />
      </p>
    </div>
  );
}

export function Home({ onSeeHistory }: { onSeeHistory: () => void }) {
  const [summary, setSummary] = useState<HomeSummary | null>(null);
  const hotkey = useHotkeyText();

  useEffect(() => {
    const load = () => void api.homeSummary().then(setSummary);
    load();
    const unlisten = events.transcriptAdded(load);
    return () => void unlisten.then((f) => f());
  }, []);

  if (!summary) return null;

  return (
    <section className="stagger mx-auto max-w-3xl px-6 py-8">
      <h1 className="text-xl font-semibold text-zinc-100">Home</h1>

      <div className="stagger mt-6 grid grid-cols-2 gap-3">
        <Stat label="Words this week" value={summary.wordsThisWeek} format={(n) => n.toLocaleString("en-US")} />
        <Stat label="Streak" value={summary.streakDays} format={(n) => plural(n, "day", "days")} />
      </div>

      <div className="mt-8 flex items-baseline justify-between">
        <h2 className="text-sm font-medium text-zinc-300">Recent</h2>
        {summary.recent.length > 0 && (
          <button onClick={onSeeHistory} className="text-xs text-zinc-400 hover:text-zinc-200">
            See all
          </button>
        )}
      </div>
      <div className="stagger mt-3 flex flex-col gap-3">
        {summary.recent.length === 0 ? (
          <Empty>Hold {hotkey}, speak, then let go.</Empty>
        ) : (
          summary.recent.map((r) => <TranscriptItem key={r.id} row={r} />)
        )}
      </div>
    </section>
  );
}
