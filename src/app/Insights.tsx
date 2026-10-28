import { useEffect, type ReactNode } from "react";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { appLabel, formatDelta, formatMinutes, plural, shortDate } from "../lib/format";
import { useAppIcon } from "../lib/icons";
import { subscribeInsights, useInsights } from "../store/insights";

const BAR = "#e4e4e7"; // zinc-200
const GRID = "rgba(255,255,255,0.06)";
const AXIS = { fill: "#71717a", fontSize: 11 }; // zinc-500
const TOOLTIP = {
  contentStyle: {
    background: "#18181b",
    border: "1px solid rgba(255,255,255,0.1)",
    borderRadius: 8,
    fontSize: 12,
  },
  labelStyle: { color: "#a1a1aa" },
  itemStyle: { color: "#f4f4f5" },
  cursor: { fill: "rgba(255,255,255,0.04)" },
};

function Card({ label, value, detail }: { label: string; value: string; detail?: string }) {
  return (
    <div className="rounded-xl border border-white/8 bg-zinc-900/60 px-4 py-3">
      <p className="text-xs text-zinc-500">{label}</p>
      <p className="mt-1 truncate text-2xl font-semibold text-zinc-100 tabular-nums">{value}</p>
      {detail && <p className="mt-0.5 text-xs text-zinc-500">{detail}</p>}
    </div>
  );
}

function Panel({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="rounded-xl border border-white/8 bg-zinc-900/60 px-4 pb-3 pt-4">
      <h2 className="text-sm font-medium text-zinc-300">{title}</h2>
      <div className="mt-3">{children}</div>
    </section>
  );
}

/** Y axis label for the app chart: icon plus app name. */
function AppTick({ x, y, payload }: { x?: number; y?: number; payload?: { value: string } }) {
  const app = payload?.value ?? "";
  const icon = useAppIcon(app === "" ? null : app);
  return (
    <g transform={`translate(${(x ?? 0) - 120},${(y ?? 0) - 8})`}>
      {icon ? (
        <image href={icon} width={16} height={16} />
      ) : (
        <rect width={16} height={16} rx={3} fill="#3f3f46" />
      )}
      <text x={22} y={12} fill="#a1a1aa" fontSize={12}>
        {appLabel(app === "" ? null : app)}
      </text>
    </g>
  );
}

function HourStrip({ counts }: { counts: number[] }) {
  const max = Math.max(1, ...counts);
  return (
    <div>
      <div className="grid grid-cols-24 gap-1" style={{ gridTemplateColumns: "repeat(24, minmax(0, 1fr))" }}>
        {counts.map((c, h) => (
          <div
            key={h}
            title={`${h}:00, ${plural(c, "dictation", "dictations")}`}
            className="h-8 rounded-sm bg-zinc-100"
            style={{ opacity: c === 0 ? 0.05 : 0.15 + 0.85 * (c / max) }}
          />
        ))}
      </div>
      <div className="mt-1.5 flex justify-between text-[11px] text-zinc-500 tabular-nums">
        <span>0:00</span>
        <span>6:00</span>
        <span>12:00</span>
        <span>18:00</span>
        <span>23:00</span>
      </div>
    </div>
  );
}

export function Insights() {
  const data = useInsights((s) => s.data);
  useEffect(subscribeInsights, []);

  if (!data) return null;

  if (data.totalDictations === 0) {
    return (
      <section className="mx-auto max-w-4xl px-6 py-8">
        <h1 className="text-xl font-semibold text-zinc-100">Insights</h1>
        <p className="py-16 text-center text-sm text-zinc-500">Dictate a few times to see your insights.</p>
      </section>
    );
  }

  const perDay = data.wordsPerDay.map((d) => ({ ...d, label: shortDate(d.date) }));
  const apps = data.wordsByApp.map((a) => ({ app: a.app ?? "", words: a.words }));

  return (
    <section className="mx-auto max-w-4xl px-6 py-8">
      <h1 className="text-xl font-semibold text-zinc-100">Insights</h1>

      <div className="mt-6 grid grid-cols-2 gap-3 lg:grid-cols-3">
        <Card
          label="Words this week"
          value={data.wordsThisWeek.toLocaleString("en-US")}
          detail={formatDelta(data.wordsThisWeek, data.wordsLastWeek)}
        />
        <Card label="Total words" value={data.totalWords.toLocaleString("en-US")} />
        <Card label="Average speed" value={`${Math.round(data.averageWpm)} wpm`} detail="Last 30 days" />
        <Card label="Time saved" value={formatMinutes(data.minutesSaved)} detail="Compared with typing" />
        <Card label="Streak" value={plural(data.streakDays, "day", "days")} />
        <Card
          label="Most used word"
          value={data.mostUsedWord?.word ?? "None yet"}
          detail={data.mostUsedWord ? plural(data.mostUsedWord.count, "time", "times") : undefined}
        />
      </div>

      <div className="mt-6 flex flex-col gap-4">
        <Panel title="Words per day">
          <ResponsiveContainer width="100%" height={200}>
            <BarChart data={perDay} margin={{ top: 4, right: 4, bottom: 0, left: -12 }}>
              <CartesianGrid vertical={false} stroke={GRID} />
              <XAxis dataKey="label" tick={AXIS} tickLine={false} axisLine={false} interval={4} />
              <YAxis tick={AXIS} tickLine={false} axisLine={false} allowDecimals={false} width={44} />
              <Tooltip {...TOOLTIP} formatter={(v) => [Number(v).toLocaleString("en-US"), "Words"]} />
              <Bar dataKey="words" fill={BAR} radius={[3, 3, 0, 0]} isAnimationActive={false} />
            </BarChart>
          </ResponsiveContainer>
        </Panel>

        <div className="grid gap-4 lg:grid-cols-2">
          <Panel title="Top words">
            <ResponsiveContainer width="100%" height={Math.max(120, data.topWords.length * 26)}>
              <BarChart data={data.topWords} layout="vertical" margin={{ top: 0, right: 12, bottom: 0, left: 0 }}>
                <XAxis type="number" hide allowDecimals={false} />
                <YAxis
                  type="category"
                  dataKey="word"
                  tick={{ ...AXIS, fill: "#a1a1aa", fontSize: 12 }}
                  tickLine={false}
                  axisLine={false}
                  width={96}
                />
                <Tooltip {...TOOLTIP} formatter={(v) => [Number(v).toLocaleString("en-US"), "Times"]} />
                <Bar dataKey="count" fill={BAR} radius={[0, 3, 3, 0]} barSize={14} isAnimationActive={false} />
              </BarChart>
            </ResponsiveContainer>
          </Panel>

          <Panel title="Words by app, last 30 days">
            {apps.length === 0 ? (
              <p className="py-10 text-center text-sm text-zinc-500">Dictate into an app to see it here.</p>
            ) : (
              <ResponsiveContainer width="100%" height={Math.max(120, apps.length * 30)}>
                <BarChart data={apps} layout="vertical" margin={{ top: 0, right: 12, bottom: 0, left: 0 }}>
                  <XAxis type="number" hide allowDecimals={false} />
                  <YAxis
                    type="category"
                    dataKey="app"
                    tick={<AppTick />}
                    tickLine={false}
                    axisLine={false}
                    width={124}
                  />
                  <Tooltip
                    {...TOOLTIP}
                    labelFormatter={(a) => appLabel(String(a) || null)}
                    formatter={(v) => [Number(v).toLocaleString("en-US"), "Words"]}
                  />
                  <Bar dataKey="words" fill={BAR} radius={[0, 3, 3, 0]} barSize={14} isAnimationActive={false} />
                </BarChart>
              </ResponsiveContainer>
            )}
          </Panel>
        </div>

        <Panel title="Dictations by hour">
          <HourStrip counts={data.dictationsByHour} />
        </Panel>
      </div>
    </section>
  );
}
