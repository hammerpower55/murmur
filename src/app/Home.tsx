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

/** The last summary seen, so coming back to Home is complete from its first frame. */
let lastSummary: HomeSummary | null = null;

/** The streak as a second big number over a run of filled pills. */
function Streak({ days }: { days: number }) {
  return (
    <div className="flex flex-col items-end @max-[44rem]/page:items-start">
      <p className="stat-large">
        <CountUp value={days} format={(n) => plural(n, "day", "days")} />
      </p>
      <span className="mt-3 flex gap-1" aria-hidden>
        {Array.from({ length: STREAK_TICKS }, (_, i) => (
          <span
            key={i}
            className={`h-2.5 w-2.5 origin-bottom rounded-full ${i < days ? "bg-ink" : "bg-surface-2"}`}
            style={{ animation: `pop-in 420ms var(--ease-out-quint) ${320 + i * 24}ms both` }}
          />
        ))}
      </span>
      <span className="mt-2 font-bold">Streak</span>
    </div>
  );
}

export function Home({ onSeeHistory }: { onSeeHistory: () => void }) {
  const [summary, setSummary] = useState<HomeSummary | null>(lastSummary);
  const hotkey = useHotkeyText();
  const perDay = useInsights((s) => s.data?.wordsPerDay);
  useEffect(subscribeInsights, []);

  useEffect(() => {
    const load = () =>
      void api.homeSummary().then((s) => {
        lastSummary = s;
        setSummary(s);
      });
    load();
    const unlisten = events.transcriptAdded(load);
    return () => void unlisten.then((f) => f());
  }, []);

  return (
    <section className="stagger page">
      <Title
        hero
        tone="amber"
        aside={<p className="font-medium">{dateline()}</p>}
        below={
          <div className="mt-8 flex animate-rise flex-wrap items-end justify-between gap-x-8 gap-y-4">
            <div>
              <p className="figure">
                <CountUp value={summary?.wordsThisWeek ?? 0} format={posterNumber} />
              </p>
              <p className="mt-3 font-bold">Words this week</p>
            </div>
            <Streak days={summary?.streakDays ?? 0} />
          </div>
        }
        foot={
          <Bars
            className="h-14 px-5"
            values={(perDay ?? []).map((d) => d.words)}
            hot={(perDay?.length ?? 0) - 1}
            titles={perDay?.map((d) => `${shortDate(d.date)}, ${plural(d.words, "word", "words")}`)}
          />
        }
      >
        Home
      </Title>

      <Hang
        label="Recent"
        note={
          summary && summary.recent.length > 0 && (
            <button onClick={onSeeHistory} className={`${linkClass} text-ink-2`}>
              See all
            </button>
          )
        }
      >
        {summary &&
          (summary.recent.length === 0 ? (
            <Empty>{`Hold ${hotkey}, speak, then let go.`}</Empty>
          ) : (
            <div className="stagger rows">
              {summary.recent.map((r, i) => (
                <TranscriptItem key={r.id} row={r} lead={i === 0} />
              ))}
            </div>
          ))}
      </Hang>
    </section>
  );
}
