import { useEffect, type ReactNode } from "react";
import { Bar, BarChart, CartesianGrid, Cell, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { AppIcon } from "../components/AppIcon";
import { Bars } from "../components/Bars";
import { CountUp } from "../components/CountUp";
import { Empty } from "../components/Empty";
import { Hang, Title, type Tone } from "../components/Page";
import { appLabel, formatDelta, formatMinutes, plural, posterNumber, shortDate } from "../lib/format";
import { prefersReducedMotion } from "../lib/motion";
import { subscribeInsights, useInsights } from "../store/insights";

const n = (v: number) => v.toLocaleString("en-US");

/** A figure on its own rounded tile. */
function Tile({
  label,
  value,
  detail,
  tone = "plain",
  span,
  large,
}: {
  label: string;
  value: ReactNode;
  detail?: string;
  tone?: Tone;
  span: string;
  large?: boolean;
}) {
  const toneClass = tone === "amber" ? "tone-amber" : tone === "invert" ? "tone-invert" : "bg-surface";
  return (
    <div className={`flex min-h-36 min-w-0 flex-col justify-between rounded-[var(--radius-panel)] px-5 py-4 ${span} ${toneClass}`}>
      <p className="font-bold">{label}</p>
      <div className="mt-6 min-w-0">
        <p className={`truncate ${large ? "stat-large" : "stat text-[2rem]"}`}>{value}</p>
        <p className="mt-1 min-h-[1lh] text-caption text-ink-2 tabular-nums">{detail}</p>
      </div>
    </div>
  );
}

/** The words themselves are the chart: type is set to the size of the count. */
function WordScale({ words }: { words: { word: string; count: number }[] }) {
  const max = Math.max(1, ...words.map((w) => w.count));
  return (
    <ol className="stagger rows">
      {words.map((w, i) => (
        <li key={w.word} className="flex items-baseline gap-3 py-1.5">
          <span className="w-5 shrink-0 text-caption text-ink-3 tabular-nums">{i + 1}</span>
          <span
            className="min-w-0 flex-1 truncate leading-none font-extrabold tracking-tight"
            style={{ fontSize: `${16 + 48 * (w.count / max) ** 1.4}px` }}
          >
            {w.word}
          </span>
          <span className="shrink-0 tabular-nums">{n(w.count)}</span>
        </li>
      ))}
    </ol>
  );
}

/** A ranked table: position, name, a bar drawn to scale, and the count. */
function Ranked({ rows }: { rows: { key: string; label: ReactNode; title?: string; value: number }[] }) {
  const max = Math.max(1, ...rows.map((r) => r.value));
  const motion = !prefersReducedMotion();
  return (
    <ol className="rows">
      {rows.map((r, i) => (
        <li key={r.key} className="grid grid-cols-[1.25rem_minmax(0,9rem)_minmax(0,1fr)_3.5rem] items-center gap-x-3 py-2">
          <span className="text-caption text-ink-3 tabular-nums">{i + 1}</span>
          <span className="flex min-w-0 items-center gap-2 truncate font-medium" title={r.title}>
            {r.label}
          </span>
          <span className="h-2.5 overflow-hidden rounded-full bg-surface-2" aria-hidden>
            <span
              className={`block h-full origin-left rounded-full ${i === 0 ? "bg-accent" : "bg-ink"}`}
              style={{
                width: `${(r.value / max) * 100}%`,
                animation: motion ? `grow-across 800ms var(--ease-out-expo) ${240 + i * 50}ms both` : undefined,
              }}
            />
          </span>
          <span className="text-right tabular-nums">{n(r.value)}</span>
        </li>
      ))}
    </ol>
  );
}

function HourStrip({ counts }: { counts: number[] }) {
  const peak = counts.indexOf(Math.max(...counts));
  return (
    <div className="py-3">
      <Bars
        className="h-28 border-b border-rule"
        values={counts}
        hot={peak}
        titles={counts.map((c, h) => `${h}:00, ${plural(c, "dictation", "dictations")}`)}
      />
      <div className="mt-1 flex justify-between text-caption text-ink-3 tabular-nums">
        <span>0:00</span>
        <span>6:00</span>
        <span>12:00</span>
        <span>18:00</span>
        <span>23:00</span>
      </div>
    </div>
  );
}

const AXIS = { fill: "var(--ink-3)", fontSize: 12 };

export function Insights() {
  const data = useInsights((s) => s.data);
  useEffect(subscribeInsights, []);

  if (!data) {
    return (
      <section className="page">
        <Title>Insights</Title>
      </section>
    );
  }

  if (data.totalDictations === 0) {
    return (
      <section className="page">
        <Title>Insights</Title>
        <Hang>
          <Empty>Dictate a few times to see your insights.</Empty>
        </Hang>
      </section>
    );
  }

  const perDay = data.wordsPerDay.map((d) => ({ ...d, label: shortDate(d.date) }));
  const today = perDay.length - 1;
  const motion = !prefersReducedMotion();

  return (
    <section className="stagger page">
      <Title>Insights</Title>

      <div className="bleed stagger -mt-6 grid grid-cols-12 gap-2">
        <Tile
          tone="amber"
          large
          span="col-span-7"
          label="Words this week"
          value={<CountUp value={data.wordsThisWeek} format={posterNumber} />}
          detail={formatDelta(data.wordsThisWeek, data.wordsLastWeek)}
        />
        <Tile
          tone="invert"
          span="col-span-5 [&_.stat]:text-[2.5rem] @max-[44rem]/page:[&_.stat]:text-[1.75rem]"
          label="Time saved"
          value={<CountUp value={data.minutesSaved} format={formatMinutes} />}
          detail="Compared with typing"
        />
        <Tile
          span="col-span-3 @max-[44rem]/page:col-span-6"
          label="Total words"
          value={<CountUp value={data.totalWords} format={n} />}
        />
        <Tile
          span="col-span-3 @max-[44rem]/page:col-span-6"
          label="Average speed"
          value={<CountUp value={data.averageWpm} format={(v) => `${v} wpm`} />}
          detail="Last 30 days"
        />
        <Tile
          span="col-span-3 @max-[44rem]/page:col-span-6"
          label="Streak"
          value={<CountUp value={data.streakDays} format={(v) => plural(v, "day", "days")} />}
        />
        <Tile
          span="col-span-3 @max-[44rem]/page:col-span-6"
          label="Most used word"
          value={data.mostUsedWord?.word ?? "None yet"}
          detail={data.mostUsedWord ? plural(data.mostUsedWord.count, "time", "times") : undefined}
        />
      </div>

      <div className="mt-8 flex flex-col gap-8">
        <Hang label="Words per day">
          <div className="pt-3">
            <ResponsiveContainer width="100%" height={260}>
              <BarChart data={perDay} margin={{ top: 4, right: 0, bottom: 0, left: 0 }} barCategoryGap={2}>
                <CartesianGrid vertical={false} stroke="var(--rule)" />
                <XAxis
                  dataKey="label"
                  tick={AXIS}
                  tickLine={false}
                  axisLine={{ stroke: "var(--rule)" }}
                  // Weeks counted back from today, so today always has a label
                  // and the first one never hangs off the edge.
                  ticks={perDay.filter((_, i) => (today - i) % 7 === 0).map((d) => d.label)}
                  interval={0}
                />
                <YAxis
                  orientation="right"
                  tick={AXIS}
                  tickLine={false}
                  axisLine={false}
                  allowDecimals={false}
                  width={44}
                />
                <Tooltip
                  contentStyle={{
                    background: "var(--surface-2)",
                    border: "none",
                    borderRadius: 12,
                    boxShadow: "none",
                    fontSize: 12,
                    padding: "4px 8px",
                  }}
                  labelStyle={{ color: "var(--ink-2)" }}
                  itemStyle={{ color: "var(--ink)", padding: 0 }}
                  cursor={{ fill: "var(--rule)", radius: 6 }}
                  formatter={(v) => [n(Number(v)), "Words"]}
                />
                <Bar
                  dataKey="words"
                  fill="var(--ink)"
                  radius={[6, 6, 0, 0]}
                  isAnimationActive={motion}
                  animationBegin={200}
                  animationDuration={800}
                  animationEasing="ease-out"
                >
                  {perDay.map((_, i) => (
                    <Cell key={i} fill={i === today ? "var(--accent)" : "var(--ink)"} />
                  ))}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          </div>
        </Hang>

        <Hang label="Top words">
          <WordScale words={data.topWords} />
        </Hang>

        <Hang label="Words by app" note={<span className="text-ink-3">Last 30 days</span>}>
          {data.wordsByApp.length === 0 ? (
            <p className="py-3 text-ink-2">Dictate into an app to see it here.</p>
          ) : (
            <Ranked
              rows={data.wordsByApp.map((a) => ({
                key: a.app ?? "",
                title: appLabel(a.app),
                label: (
                  <>
                    <AppIcon app={a.app} />
                    <span className="truncate">{appLabel(a.app)}</span>
                  </>
                ),
                value: a.words,
              }))}
            />
          )}
        </Hang>

        <Hang label="Dictations by hour">
          <HourStrip counts={data.dictationsByHour} />
        </Hang>
      </div>
    </section>
  );
}
