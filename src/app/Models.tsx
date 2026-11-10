import { useEffect, useMemo, useState } from "react";
import { inputClass, linkClass, Row, selectClass, Toggle } from "../components/controls";
import { FAMILY_NAMES, ModelCard } from "../components/ModelCard";
import { Hang, Title } from "../components/Page";
import { type ModelInfo } from "../lib/api";
import { LANGUAGE_NAMES, supportsLanguage } from "../lib/format";
import { prefersReducedMotion } from "../lib/motion";
import { useModels } from "../store/models";
import { useSettings } from "../store/settings";

const SCALE = [1, 2, 3, 4, 5];

/** Every model on one picture: further right is faster, higher is more accurate,
 * a bigger disc needs more memory. Picking one brings its row into view. */
function ModelMap({ models, active }: { models: ModelInfo[]; active: string | null }) {
  const pick = (id: string) => {
    const row = document.getElementById(`model-${id}`);
    if (!row) return;
    row.scrollIntoView({ behavior: prefersReducedMotion() ? "auto" : "smooth", block: "center" });
    row.classList.add("fresh");
    window.setTimeout(() => row.classList.remove("fresh"), 1800);
  };
  let order = 0;
  return (
    <div>
      <div className="grid grid-cols-5 overflow-clip rounded-[var(--radius-panel)] bg-surface">
        {[...SCALE].reverse().map((accuracy) =>
          SCALE.map((speed) => (
            <div
              key={`${accuracy}-${speed}`}
              className="flex min-h-14 flex-wrap items-center justify-center gap-1.5 border-r border-b border-rule p-1.5 nth-[5n]:border-r-0 nth-last-[-n+5]:border-b-0"
            >
              {models
                .filter((m) => m.accuracy === accuracy && m.speed === speed)
                .map((m) => {
                  const size = 12 + Math.min(1, m.ramMb / 2500) * 22;
                  const delay = 200 + order++ * 28;
                  return (
                    <button
                      key={m.id}
                      title={m.name}
                      aria-label={m.name}
                      onClick={() => pick(m.id)}
                      className={`rounded-full transition-transform duration-200 hover:scale-125 ${
                        m.id === active ? "bg-accent" : m.installed ? "bg-ink" : "border border-ink-2 hover:border-ink"
                      }`}
                      style={{
                        width: size,
                        height: size,
                        animation: `pop-in 480ms var(--ease-out-quint) ${delay}ms both`,
                      }}
                    />
                  );
                })}
            </div>
          )),
        )}
      </div>
      <div className="mt-1.5 flex justify-between px-5 text-caption text-ink-3">
        <span>Slower</span>
        <span>Faster</span>
      </div>
    </div>
  );
}

/** Models under one heading per family, in catalogue order. */
function byFamily(models: ModelInfo[]): [ModelInfo["family"], ModelInfo[]][] {
  const groups = new Map<ModelInfo["family"], ModelInfo[]>();
  for (const m of models) groups.set(m.family, [...(groups.get(m.family) ?? []), m]);
  return [...groups];
}

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

  if (!view) {
    return (
      <section className="page">
        <Title>Models</Title>
      </section>
    );
  }
  const active = view.models.find((m) => m.id === view.active);
  const fixers = view.models.filter((m) => m.family === "fixup");
  const card = (m: ModelInfo) => (
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
      device={view.device}
      on={settings?.fixMisheardWords === true && view.fixer === m.id}
    />
  );
  const romanUrduHint = view.romanUrdu && active !== undefined && active.family !== "whisper";

  return (
    <section className="stagger page">
      <Title
        aside={
          <div className="flex flex-col items-end gap-1.5">
            <div className="flex items-end gap-4">
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search models"
                aria-label="Search models"
                className={`${inputClass} w-36`}
              />
              <select
                value={language}
                aria-label="Language"
                onChange={(e) => setLanguage(e.target.value)}
                className={selectClass}
              >
                <option value="">All languages</option>
                {Object.entries(LANGUAGE_NAMES).map(([code, name]) => (
                  <option key={code} value={code}>
                    {name}
                  </option>
                ))}
              </select>
            </div>
            <p className="text-caption text-ink-3">Speeds are for your {view.device}</p>
          </div>
        }
      >
        Models
      </Title>

      {romanUrduHint && (
        <p className="mb-8 w-fit animate-pop rounded-full bg-accent px-4 py-1.5 font-medium text-accent-ink">
          Roman Urdu works best with a Whisper model
        </p>
      )}

      <div className="flex flex-col gap-8">
        {shown.length === 0 && (
          <Hang>
            <p className="py-3 text-ink-2">Clear the search to see every model.</p>
          </Hang>
        )}
        {shown.length > 0 && (
          <Hang label="Speed and accuracy" note={<span className="text-caption text-ink-3">Higher is more accurate</span>}>
            <ModelMap models={shown.filter((m) => m.accuracy > 0)} active={view.active} />
          </Hang>
        )}
        {byFamily(shown).map(([family, models]) => (
          <Hang key={family} label={FAMILY_NAMES[family]}>
            <div className="rows">{models.map(card)}</div>
          </Hang>
        ))}

        {fixers.length > 0 && (
          <Hang label="Fix misheard words">
            <div className="rows">
              {fixers.map(card)}
              {settings && fixers.some((m) => m.installed) && (
                <>
                  <Row label="Fix misheard words">
                    <Toggle
                      label="Fix misheard words"
                      checked={settings.fixMisheardWords}
                      onChange={(v) => void update({ fixMisheardWords: v })}
                    />
                  </Row>
                  <Row label="Check every word, not only your own words">
                    <Toggle
                      label="Check every word"
                      checked={settings.fixAnyWord}
                      disabled={!settings.fixMisheardWords}
                      onChange={(v) => void update({ fixAnyWord: v })}
                    />
                  </Row>
                  {view.npu && (
                    <Row label="Use the NPU">
                      <Toggle
                        label="Use the NPU"
                        checked={settings.useNpu}
                        onChange={(v) => void update({ useNpu: v })}
                      />
                    </Row>
                  )}
                  <Row label="Talk to the model to see what it can do">
                    <button className={linkClass} onClick={onOpenChat}>
                      Open chat
                    </button>
                  </Row>
                </>
              )}
            </div>
          </Hang>
        )}
      </div>
    </section>
  );
}
