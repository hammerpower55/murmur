import { useCallback, useEffect, useRef, useState } from "react";
import { TranscriptItem } from "../components/TranscriptItem";
import { api, events, type TranscriptRow } from "../lib/api";
import { useHotkeyText } from "../store/settings";

const PAGE = 50;

export function History() {
  const [query, setQuery] = useState("");
  const [rows, setRows] = useState<TranscriptRow[] | null>(null);
  const [more, setMore] = useState(false);
  const latest = useRef(0);
  const hotkey = useHotkeyText();

  /** Reloads from the top, keeping as many rows as are already shown. */
  const load = useCallback(async (q: string, count: number) => {
    const ticket = ++latest.current;
    const page = await api.listTranscripts(q, count + 1, 0);
    if (ticket !== latest.current) return;
    setMore(page.length > count);
    setRows(page.slice(0, count));
  }, []);

  useEffect(() => {
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

  return (
    <section className="mx-auto max-w-3xl px-6 py-8">
      <h1 className="text-xl font-semibold text-zinc-100">History</h1>

      <input
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder="Search transcripts"
        className="mt-6 w-full rounded-lg border border-white/10 bg-zinc-900 px-3 py-2 text-sm text-zinc-100 placeholder:text-zinc-500 focus:border-zinc-400 focus:outline-none"
      />

      <div className="mt-4 flex flex-col gap-3">
        {rows !== null && rows.length === 0 && (
          <p className="py-10 text-center text-sm text-zinc-500">
            {query.trim() === ""
              ? `Hold ${hotkey} to dictate your first transcript.`
              : "Clear the search to see every transcript."}
          </p>
        )}
        {rows?.map((r) => (
          <TranscriptItem key={r.id} row={r} onDelete={() => void remove(r.id)} />
        ))}
        {more && rows && (
          <button
            onClick={() => void load(query, rows.length + PAGE)}
            className="self-center rounded-md border border-white/10 px-3 py-1.5 text-sm text-zinc-300 hover:bg-white/5"
          >
            Show more
          </button>
        )}
      </div>
    </section>
  );
}
