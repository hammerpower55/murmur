import { useEffect, useState } from "react";
import { CountUp } from "../components/CountUp";
import { Empty } from "../components/Empty";
import { TranscriptItem } from "../components/TranscriptItem";
import { api, events, type HomeSummary } from "../lib/api";
import { plural } from "../lib/format";
import { useHotkeyText } from "../store/settings";

function Stat({ label, value, format }: { label: string; value: number; format: (n: number) => string }) {
  return (
    <div className="surface rounded-xl px-5 py-4">
      <p className="stat-label">{label}</p>
      <p className="stat-value mt-2">
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
    <section className="stagger page">
      <h1 className="page-title">Home</h1>

      <div className="stagger mt-8 grid grid-cols-2 gap-4">
        <Stat label="Words this week" value={summary.wordsThisWeek} format={(n) => n.toLocaleString("en-US")} />
        <Stat label="Streak" value={summary.streakDays} format={(n) => plural(n, "day", "days")} />
      </div>

      <div className="mt-12 flex items-baseline justify-between">
        <h2 className="section-title">Recent</h2>
        {summary.recent.length > 0 && (
          <button onClick={onSeeHistory} className="text-xs text-zinc-400 hover:text-zinc-200">
            See all
          </button>
        )}
      </div>
      <div className="stagger mt-4 flex flex-col gap-3">
        {summary.recent.length === 0 ? (
          <Empty>Hold {hotkey}, speak, then let go.</Empty>
        ) : (
          summary.recent.map((r) => <TranscriptItem key={r.id} row={r} />)
        )}
      </div>
    </section>
  );
}
