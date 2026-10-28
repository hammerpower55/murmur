import { useMemo, useState } from "react";
import { ModelCard } from "../components/ModelCard";
import { LANGUAGE_NAMES, supportsLanguage } from "../lib/format";
import { useModels } from "../store/models";

export function Models() {
  const { view, errors, download, cancel, use, remove } = useModels();
  const [query, setQuery] = useState("");
  const [language, setLanguage] = useState("");

  const shown = useMemo(() => {
    if (!view) return [];
    const q = query.trim().toLowerCase();
    return view.models.filter(
      (m) =>
        supportsLanguage(m.languages, language) &&
        (q === "" ||
          m.name.toLowerCase().includes(q) ||
          m.family.includes(q) ||
          m.languagesLabel.toLowerCase().includes(q)),
    );
  }, [view, query, language]);

  if (!view) return null;
  const active = view.models.find((m) => m.id === view.active);
  const romanUrduHint = view.romanUrdu && active !== undefined && active.family !== "whisper";

  return (
    <section className="mx-auto max-w-3xl px-6 py-8">
      <h1 className="text-xl font-semibold text-zinc-100">Models</h1>
      <p className="mt-1 text-sm text-zinc-500">Ratings compare models on this device class.</p>

      {romanUrduHint && (
        <p className="mt-4 rounded-lg border border-amber-300/20 bg-amber-300/5 px-3 py-2 text-sm text-amber-200">
          Roman Urdu works best with a Whisper model
        </p>
      )}

      <div className="mt-6 flex gap-3">
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search models"
          className="min-w-0 flex-1 rounded-lg border border-white/10 bg-zinc-900 px-3 py-2 text-sm text-zinc-100 placeholder:text-zinc-500 focus:border-zinc-400 focus:outline-none"
        />
        <select
          value={language}
          onChange={(e) => setLanguage(e.target.value)}
          className="rounded-lg border border-white/10 bg-zinc-900 px-3 py-2 text-sm text-zinc-100 focus:border-zinc-400 focus:outline-none"
        >
          <option value="">All languages</option>
          {Object.entries(LANGUAGE_NAMES).map(([code, name]) => (
            <option key={code} value={code}>
              {name}
            </option>
          ))}
        </select>
      </div>

      <div className="mt-4 flex flex-col gap-3">
        {shown.length === 0 && (
          <p className="py-10 text-center text-sm text-zinc-500">No models match. Clear the search to see all.</p>
        )}
        {shown.map((m) => (
          <ModelCard
            key={m.id}
            model={m}
            active={view.active === m.id}
            loading={view.loading === m.id}
            progress={view.downloads[m.id]}
            error={errors[m.id]}
            onDownload={() => void download(m.id)}
            onCancel={() => void cancel(m.id)}
            onUse={() => void use(m.id)}
            onDelete={() => void remove(m.id)}
          />
        ))}
      </div>
    </section>
  );
}
