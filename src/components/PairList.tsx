// An editable list of two field entries (spoken and written, trigger and
// text, spelling and variants), shared by the Dictionary and Snippets pages.
// The whole list is saved on every change; Rust validates it. It is set as a
// table whose second column starts on the page's axis.

import { useId, useMemo, useState } from "react";
import { inputClass, linkClass, primaryClass } from "./controls";
import { Empty } from "./Empty";

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
  /** A small label shown after each pair, by index. */
  tags?: (string | undefined)[];
}

const ROW = "grid grid-cols-12 gap-x-4";

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
  const id = useId();
  const ids = [`${id}-a`, `${id}-b`];
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
    <div className={`${ROW} animate-pop gap-y-3 py-3`} onKeyDown={onKey}>
      <label htmlFor={ids[0]} className="col-span-3 py-1 font-medium">
        {fields[0].label}
      </label>
      <input
        id={ids[0]}
        autoFocus
        placeholder={fields[0].placeholder}
        value={a}
        onChange={(e) => setA(e.target.value)}
        className={`${inputClass} col-span-9`}
      />
      <label htmlFor={ids[1]} className="col-span-3 py-1 font-medium">
        {fields[1].label}
      </label>
      {fields[1].multiline ? (
        <textarea
          id={ids[1]}
          placeholder={fields[1].placeholder}
          value={b}
          rows={3}
          onChange={(e) => setB(e.target.value)}
          className={`${inputClass} col-span-9 resize-y`}
        />
      ) : (
        <input
          id={ids[1]}
          placeholder={fields[1].placeholder}
          value={b}
          onChange={(e) => setB(e.target.value)}
          className={`${inputClass} col-span-9`}
        />
      )}
      <div className="col-span-9 col-start-4 flex items-center gap-4">
        <button className={primaryClass} disabled={busy || !a.trim() || !b.trim()} onClick={() => void submit()}>
          Save
        </button>
        <button className={linkClass} onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}

export function PairList({ pairs, fields, save, addLabel, empty, searchFrom = 12, tags }: Props) {
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
      <div className={`${ROW} items-end border-t border-ink py-2`}>
        <span className="col-span-3 font-bold">{fields[0].label}</span>
        <span className="col-span-9 flex items-end justify-between gap-6">
          <span className="font-bold">{fields[1].label}</span>
          <span className="flex items-end gap-4">
            {pairs.length >= searchFrom && (
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search"
                aria-label="Search"
                className={`${inputClass} w-40`}
              />
            )}
            <button
              className={primaryClass}
              disabled={editing !== null}
              onClick={() => {
                setError(null);
                setEditing(-1);
              }}
            >
              {addLabel}
            </button>
          </span>
        </span>
      </div>

      {error && (
        <p key={error} className="animate-shake border-t border-rule py-2 text-danger">
          {error}
        </p>
      )}

      <div className="rows border-t border-rule">
        {editing === -1 && (
          <Editor initial={["", ""]} fields={fields} onSave={(p) => put(-1, p)} onCancel={() => setEditing(null)} />
        )}
        {pairs.length === 0 && editing !== -1 && (
          <div className={ROW}>
            <div className="col-span-9 col-start-4">
              <Empty>{empty}</Empty>
            </div>
          </div>
        )}
        {pairs.length > 0 && shown.length === 0 && (
          <div className={`${ROW} py-3`}>
            <p className="col-span-9 col-start-4 text-ink-2">Clear the search to see every entry.</p>
          </div>
        )}
        {shown.map(({ p, i }) =>
          editing === i ? (
            <Editor key={i} initial={p} fields={fields} onSave={(np) => put(i, np)} onCancel={() => setEditing(null)} />
          ) : (
            <div
              // Keyed by content so deleting one row does not replay the rest.
              key={`${p[0]}\u0000${p[1]}`}
              className={`${ROW} group animate-rise items-start py-2.5`}
              style={{ animationDelay: `${Math.min(i, 10) * 20}ms` }}
            >
              <span className="col-span-3 truncate font-medium" title={p[0]}>
                {p[0]}
              </span>
              <span className="col-span-9 flex items-start gap-4">
                <span className="line-clamp-2 min-w-0 flex-1 whitespace-pre-wrap text-ink-2">{p[1]}</span>
                {tags?.[i] && <span className="shrink-0 bg-accent px-1.5 text-caption font-semibold text-accent-ink">{tags[i]}</span>}
                <span className="flex shrink-0 gap-3 text-caption text-ink-2 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                  <button
                    className={linkClass}
                    onClick={() => {
                      setError(null);
                      setEditing(i);
                    }}
                  >
                    Edit
                  </button>
                  <button className={linkClass} onClick={() => void commit(pairs.filter((_, j) => j !== i))}>
                    Delete
                  </button>
                </span>
              </span>
            </div>
          ),
        )}
      </div>
    </div>
  );
}
