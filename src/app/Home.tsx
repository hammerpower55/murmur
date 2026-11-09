import { useEffect, useState } from "react";
import { Bars } from "../components/Bars";
import { CountUp } from "../components/CountUp";
import { linkClass } from "../components/controls";
import { Empty } from "../components/Empty";
import { Hang, Title } from "../components/Page";
import { TranscriptItem } from "../components/TranscriptItem";
import { api, events, type HomeSummary } from "../lib/api";
import { dateline, plural, posterNumber, shortDate } from "../lib/format";
import { subscribeInsights, useInsights } from "../store/insights";
import { useHotkeyText } from "../store/settings";

const STREAK_TICKS = 14;

/** The streak as a run of filled squares. */
function Streak({ days }: { days: number }) {
  return (
    <span className="flex items-center gap-3">
      <span className="flex gap-0.5" aria-hidden>
        {Array.from({ length: STREAK_TICKS }, (_, i) => (
          <span
            key={i}
            className={`h-2.5 w-2.5 origin-bottom ${i < days ? "bg-ink" : "bg-rule"}`}
            style={{ animation: `grow-up 420ms var(--ease-out-expo) ${320 + i * 24}ms both` }}
          />
        ))}
      </span>
      <span className="font-bold">
        <CountUp value={days} format={(n) => plural(n, "day", "days")} />
      </span>
    </span>
  );
}

export function Home({ onSeeHistory }: { onSeeHistory: () => void }) {
  const [summary, setSummary] = useState<HomeSummary | null>(null);
  const hotkey = useHotkeyText();
  const perDay = useInsights((s) => s.data?.wordsPerDay);
  useEffect(subscribeInsights, []);

  useEffect(() => {
    const load = () => void api.homeSummary().then(setSummary);
    load();
    const unlisten = events.transcriptAdded(load);
    return () => void unlisten.then((f) => f());
  }, []);

  if (!summary) return null;

  return (
    <section className="stagger page">
      <Title
        hero
        tone="amber"
        seed={3}
        aside={<p className="font-medium">{dateline()}</p>}
        below={
          <div className="mt-8 animate-rise">
            <p className="figure">
              <CountUp value={summary.wordsThisWeek} format={posterNumber} />
            </p>
            <div className="mt-3 flex flex-wrap items-center gap-x-8 gap-y-1">
              <span className="font-bold">Words this week</span>
              <Streak days={summary.streakDays} />
            </div>
          </div>
        }
        foot={
          perDay && (
            <Bars
              className="h-14"
              values={perDay.map((d) => d.words)}
              hot={perDay.length - 1}
              titles={perDay.map((d) => `${shortDate(d.date)}, ${plural(d.words, "word", "words")}`)}
            />
          )
        }
      >
        Home
      </Title>

      <Hang
        label="Recent"
        note={
          summary.recent.length > 0 && (
            <button onClick={onSeeHistory} className={`${linkClass} text-ink-2`}>
              See all
            </button>
          )
        }
      >
        {summary.recent.length === 0 ? (
          <Empty>{`Hold ${hotkey}, speak, then let go.`}</Empty>
        ) : (
          <div className="stagger rows">
            {summary.recent.map((r, i) => (
              <TranscriptItem key={r.id} row={r} lead={i === 0} />
            ))}
          </div>
        )}
      </Hang>
    </section>
  );
}
