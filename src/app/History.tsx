import { useCallback, useEffect, useRef, useState } from "react";
import { Bars } from "../components/Bars";
import { buttonClass, inputClass } from "../components/controls";
import { Empty } from "../components/Empty";
import { Hang, Title } from "../components/Page";
import { TranscriptItem } from "../components/TranscriptItem";
import { api, events, type TranscriptRow } from "../lib/api";
import { dayOfMonth, formatTime, groupByDay } from "../lib/format";
import { useHotkeyText } from "../store/settings";

const PAGE = 50;
/** How many dictations the bars across the top show, newest on the right. */
const PRINT = 60;

/** The last unfiltered list, so coming back to History is complete from its first
 * frame. */
let lastRows: TranscriptRow[] | null = null;

/** The numeral of the day, large, with what is left of its name under it. */
function DayHeading({ label, iso }: { label: string; iso: string }) {
  return (
    <>
      <span className="numeral block">{dayOfMonth(iso)}</span>
      <span className="mt-1 block font-medium text-ink-2">{label.replace(/^\d+\s*/, "")}</span>
    </>
  );
}

export function History() {
  const [query, setQuery] = useState("");
  const [rows, setRows] = useState<TranscriptRow[] | null>(lastRows);
  const [more, setMore] = useState(false);
  const latest = useRef(0);
  const hotkey = useHotkeyText();

  /** Reloads from the top, keeping as many rows as are already shown. */
  const load = useCallback(async (q: string, count: number) => {
    const ticket = ++latest.current;
    const page = await api.listTranscripts(q, count + 1, 0);
    if (ticket !== latest.current) return;
    const shown = page.slice(0, count);
    if (q === "") lastRows = shown;
    setMore(page.length > count);
    setRows(shown);
  }, []);

  // Straight away when the page opens; only typing waits for a pause.
  const opened = useRef(false);
  useEffect(() => {
    if (!opened.current) {
      opened.current = true;
      void load(query, PAGE);
      return;
    }
    const t = window.setTimeout(() => void load(query, PAGE), 150);
    return () => window.clearTimeout(t);
  }, [query, load]);

  useEffect(() => {
    const unlisten = events.transcriptAdded(() => void load(query, Math.max(PAGE, rows?.length ?? 0)));
    return () => void unlisten.then((f) => f());
  }, [query, rows?.length, load]);

  const remove = async (id: number) => {
    await api.deleteTranscript(id);
    setRows((r) => r?.filter((x) => x.id !== id) ?? null);
  };

  const print = (rows ?? []).slice(0, PRINT).reverse();

  return (
    <section className="stagger page">
      <Title
        foot={<Bars className="h-16 px-5" values={print.map((r) => r.wordCount)} hot={print.length - 1} />}
        aside={
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search transcripts"
            aria-label="Search transcripts"
            className={`${inputClass} w-56`}
          />
        }
      >
        History
      </Title>

      <div className="flex flex-col gap-8">
        {rows !== null && rows.length === 0 && (
          <Hang>
            {query.trim() === "" ? (
              <Empty>{`Hold ${hotkey} to dictate your first transcript.`}</Empty>
            ) : (
              <p className="animate-pop py-3 text-ink-2">Clear the search to see every transcript.</p>
            )}
          </Hang>
        )}
        {rows &&
          groupByDay(rows).map((day) => (
            <Hang key={day.label} label={<DayHeading label={day.label} iso={day.rows[0].createdAt} />}>
              <div className="rows">
                {day.rows.map((r) => (
                  <TranscriptItem
                    key={r.id}
                    row={r}
                    time={formatTime(r.createdAt)}
                    onDelete={() => void remove(r.id)}
                  />
                ))}
              </div>
            </Hang>
          ))}
        {more && rows && (
          <Hang>
            <div className="py-3">
              <button onClick={() => void load(query, rows.length + PAGE)} className={buttonClass}>
                Show more
              </button>
            </div>
          </Hang>
        )}
      </div>
    </section>
  );
}
