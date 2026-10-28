import { useEffect, useState } from "react";
import { ModelCard } from "../components/ModelCard";
import { api, errorText } from "../lib/api";
import { APP_NAME } from "../lib/app";
import { Wave } from "../components/Wave";
import { useDictation } from "../store/dictation";
import { useModels } from "../store/models";
import { useHotkeyText } from "../store/settings";

type Mic = { state: "checking" } | { state: "ok" } | { state: "blocked"; message: string };

interface Props {
  onDone: () => void;
  onSeeAllModels: () => void;
}

export function Onboarding({ onDone, onSeeAllModels }: Props) {
  const { view, errors, download, cancel, use } = useModels();
  const [mic, setMic] = useState<Mic>({ state: "checking" });
  const hotkey = useHotkeyText();
  const activity = useDictation((d) => d.activity);

  const checkMic = () => {
    setMic({ state: "checking" });
    api
      .checkMicrophone()
      .then(() => setMic({ state: "ok" }))
      .catch((err) => setMic({ state: "blocked", message: errorText(err) }));
  };
  useEffect(checkMic, []);

  if (!view) return null;
  const model = view.models.find((m) => m.default);
  const ready = model !== undefined && view.active === model.id;
  const anyActive = view.active !== null;

  return (
    <section className="stagger mx-auto flex max-w-xl flex-col gap-8 px-6 py-16">
      <div className="flex flex-col items-start gap-6">
        <Wave activity={activity === "idle" ? "recording" : activity} size="lg" />
        <h1 className="text-3xl font-semibold tracking-tight text-zinc-100">Welcome to {APP_NAME}</h1>
      </div>

      <div>
        <h2 className="text-sm font-medium text-zinc-300">Microphone</h2>
        {mic.state === "checking" && <p className="mt-2 animate-breathe text-sm text-zinc-500">Checking the microphone</p>}
        {mic.state === "ok" && <p className="mt-2 animate-pop text-sm text-zinc-300">Microphone ready</p>}
        {mic.state === "blocked" && (
          <div className="mt-2 flex flex-col items-start gap-3">
            <p className="text-sm text-amber-200">{mic.message}</p>
            <div className="flex gap-2">
              <button
                className="rounded-md bg-zinc-100 px-3 py-1.5 text-sm font-medium text-zinc-900 hover:bg-white"
                onClick={() => void api.openMicrophoneSettings()}
              >
                Open microphone settings
              </button>
              <button
                className="rounded-md border border-white/10 px-3 py-1.5 text-sm text-zinc-200 hover:bg-white/5"
                onClick={checkMic}
              >
                Check again
              </button>
            </div>
          </div>
        )}
      </div>

      <div>
        <h2 className="text-sm font-medium text-zinc-300">Speech model</h2>
        <div className="mt-3">
          {model && (
            <ModelCard
              compact
              model={model}
              active={view.active === model.id}
              loading={view.loading === model.id}
              progress={view.downloads[model.id]}
              error={errors[model.id]}
              onDownload={() => void download(model.id)}
              onCancel={() => void cancel(model.id)}
              onUse={() => void use(model.id)}
              onDelete={() => {}}
            />
          )}
        </div>
        <button className="mt-3 text-sm text-zinc-400 underline-offset-4 hover:text-zinc-200 hover:underline" onClick={onSeeAllModels}>
          See all models
        </button>
      </div>

      {(ready || anyActive) && (
        <div className="flex animate-rise flex-col items-start gap-3">
          <p className="text-sm text-zinc-300">Hold {hotkey}, speak, then let go.</p>
          <button
            className="rounded-md bg-zinc-100 px-4 py-2 text-sm font-medium text-zinc-900 hover:bg-white"
            onClick={onDone}
          >
            Start dictating
          </button>
        </div>
      )}
    </section>
  );
}
