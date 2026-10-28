// An editable list of two field entries (spoken and written, trigger and
// text, spelling and variants), shared by the Dictionary and Snippets pages.
// The whole list is saved on every change; Rust validates it.

import { useMemo, useState } from "react";
import { buttonClass, inputClass, primaryClass } from "./controls";

export type Pair = [string, string];

export interface PairField {
  label: string;
  placeholder: string;
  multiline?: boolean;
}

interface Props {
  pairs: Pair[];
  fields: [PairField, PairField];
  /** Saves the whole list; rejects with a one line message. */
  save: (pairs: Pair[]) => Promise<void>;
  addLabel: string;
  empty: string;
  /** Shows a search box once the list is this long. */
  searchFrom?: number;
}

function Editor({
  initial,
  fields,
  onSave,
  onCancel,
}: {
  initial: Pair;
  fields: [PairField, PairField];
  onSave: (p: Pair) => Promise<boolean>;
  onCancel: () => void;
}) {
  const [a, setA] = useState(initial[0]);
  const [b, setB] = useState(initial[1]);
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    setBusy(true);
    const ok = await onSave([a, b]);
    setBusy(false);
    if (ok) onCancel();
  };
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") onCancel();
    if (e.key === "Enter" && !(e.target instanceof HTMLTextAreaElement)) void submit();
  };
  return (
    <div className="flex animate-pop flex-col gap-2 px-4 py-3" onKeyDown={onKey}>
      <input
        autoFocus
        aria-label={fields[0].label}
        placeholder={fields[0].placeholder}
        value={a}
        onChange={(e) => setA(e.target.value)}
        className={inputClass}
      />
      {fields[1].multiline ? (
        <textarea
          aria-label={fields[1].label}
          placeholder={fields[1].placeholder}
          value={b}
          rows={3}
          onChange={(e) => setB(e.target.value)}
          className={`${inputClass} resize-y`}
        />
      ) : (
        <input
          aria-label={fields[1].label}
          placeholder={fields[1].placeholder}
          value={b}
          onChange={(e) => setB(e.target.value)}
          className={inputClass}
        />
      )}
      <div className="flex justify-end gap-2">
        <button className={buttonClass} onClick={onCancel}>
          Cancel
        </button>
        <button className={primaryClass} disabled={busy || !a.trim() || !b.trim()} onClick={() => void submit()}>
          Save
        </button>
      </div>
    </div>
  );
}

export function PairList({ pairs, fields, save, addLabel, empty, searchFrom = 12 }: Props) {
  /** Index being edited, -1 for a new entry, null when not editing. */
  const [editing, setEditing] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return pairs
      .map((p, i) => ({ p, i }))
      .filter(({ p }) => q === "" || p[0].toLowerCase().includes(q) || p[1].toLowerCase().includes(q));
  }, [pairs, query]);

  const commit = async (next: Pair[]) => {
    setError(null);
    try {
      await save(next);
      return true;
    } catch (err) {
      setError(typeof err === "string" ? err : "Could not save. Try again.");
      return false;
    }
  };

  const put = (index: number, p: Pair) =>
    commit(index === -1 ? [...pairs, p] : pairs.map((old, i) => (i === index ? p : old)));

  return (
    <div>
      <div className="flex gap-3">
        {pairs.length >= searchFrom && (
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search"
            className={`${inputClass} flex-1`}
          />
        )}
        <button
          className={`${primaryClass} ml-auto`}
          disabled={editing !== null}
          onClick={() => {
            setError(null);
            setEditing(-1);
          }}
        >
          {addLabel}
        </button>
      </div>

      {error && (
        <p key={error} className="mt-3 animate-shake text-sm text-red-300">
          {error}
        </p>
      )}

      <div className="mt-3 divide-y divide-white/6 surface rounded-xl">
        {editing === -1 && (
          <Editor initial={["", ""]} fields={fields} onSave={(p) => put(-1, p)} onCancel={() => setEditing(null)} />
        )}
        {pairs.length === 0 && editing !== -1 && (
          <p className="animate-pop px-4 py-10 text-center text-sm text-zinc-500">{empty}</p>
        )}
        {pairs.length > 0 && shown.length === 0 && (
          <p className="px-4 py-10 text-center text-sm text-zinc-500">Clear the search to see every entry.</p>
        )}
        {shown.map(({ p, i }) =>
          editing === i ? (
            <Editor key={i} initial={p} fields={fields} onSave={(np) => put(i, np)} onCancel={() => setEditing(null)} />
          ) : (
            <div
              // Keyed by content so deleting one row does not replay the rest.
              key={`${p[0]}\u0000${p[1]}`}
              className="group flex animate-rise items-start gap-4 px-4 py-2.5 transition-colors duration-200 hover:bg-white/[0.025]"
              style={{ animationDelay: `${Math.min(i, 10) * 25}ms` }}
            >
              <span className="w-2/5 shrink-0 truncate text-sm text-zinc-100" title={p[0]}>
                {p[0]}
              </span>
              <span className="line-clamp-2 min-w-0 flex-1 text-sm whitespace-pre-wrap text-zinc-400">{p[1]}</span>
              <span className="flex shrink-0 gap-1 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                <button
                  className="rounded px-2 py-0.5 text-xs text-zinc-300 hover:bg-white/8"
                  onClick={() => {
                    setError(null);
                    setEditing(i);
                  }}
                >
                  Edit
                </button>
                <button
                  className="rounded px-2 py-0.5 text-xs text-zinc-300 hover:bg-white/8"
                  onClick={() => void commit(pairs.filter((_, j) => j !== i))}
                >
                  Delete
                </button>
              </span>
            </div>
          ),
        )}
      </div>
    </div>
  );
}
