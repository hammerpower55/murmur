import { useEffect, useMemo, useState } from "react";
import { buttonClass, Toggle } from "../components/controls";
import { ModelCard } from "../components/ModelCard";
import { LANGUAGE_NAMES, supportsLanguage } from "../lib/format";
import { useModels } from "../store/models";
import { useSettings } from "../store/settings";

export function Models({ onOpenChat }: { onOpenChat: () => void }) {
  const { view, errors, download, cancel, use, remove } = useModels();
  const { settings, load: loadSettings, update } = useSettings();
  useEffect(() => {
    if (!settings) void loadSettings();
  }, [settings, loadSettings]);
  const [query, setQuery] = useState("");
  const [language, setLanguage] = useState("");

  const shown = useMemo(() => {
    if (!view) return [];
    const q = query.trim().toLowerCase();
    return view.models.filter(
      (m) =>
        m.family !== "fixup" &&
        supportsLanguage(m.languages, language) &&
        (q === "" ||
          m.name.toLowerCase().includes(q) ||
          m.family.includes(q) ||
          m.languagesLabel.toLowerCase().includes(q)),
    );
  }, [view, query, language]);

  if (!view) return null;
  const active = view.models.find((m) => m.id === view.active);
  const fixers = view.models.filter((m) => m.family === "fixup");
  const card = (m: (typeof view.models)[number]) => (
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
      on={settings?.fixMisheardWords === true && view.fixer === m.id}
    />
  );
  const romanUrduHint = view.romanUrdu && active !== undefined && active.family !== "whisper";

  return (
    <section className="stagger page">
      <h1 className="page-title">Models</h1>

      {romanUrduHint && (
        <p className="mt-4 animate-pop rounded-lg border border-accent/25 bg-accent/8 px-3 py-2 text-sm text-accent-strong">
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

      <div className="stagger mt-4 flex flex-col gap-3">
        {shown.length === 0 && (
          <p className="py-10 text-center text-sm text-zinc-500">Clear the search to see every model.</p>
        )}
        {shown.map(card)}
      </div>

      {fixers.length > 0 && (
        <>
          <h2 className="mt-10 text-sm font-semibold text-zinc-100">Fix misheard words</h2>
          <div className="stagger mt-3 flex flex-col gap-3">{fixers.map(card)}</div>
          {settings && fixers.some((m) => m.installed) && (
            <div className="surface mt-3 flex animate-pop flex-col divide-y divide-white/6 rounded-xl">
              <label className="flex items-center justify-between gap-4 px-4 py-3 text-sm text-zinc-200">
                Fix misheard words
                <Toggle
                  label="Fix misheard words"
                  checked={settings.fixMisheardWords}
                  onChange={(v) => void update({ fixMisheardWords: v })}
                />
              </label>
              <label className="flex items-center justify-between gap-4 px-4 py-3 text-sm text-zinc-200">
                Check every word, not only your own words
                <Toggle
                  label="Check every word"
                  checked={settings.fixAnyWord}
                  disabled={!settings.fixMisheardWords}
                  onChange={(v) => void update({ fixAnyWord: v })}
                />
              </label>
              <div className="flex items-center justify-between gap-4 px-4 py-3 text-sm text-zinc-200">
                Talk to the model to see what it can do
                <button className={buttonClass} onClick={onOpenChat}>
                  Open chat
                </button>
              </div>
            </div>
          )}
        </>
      )}
    </section>
  );
}
