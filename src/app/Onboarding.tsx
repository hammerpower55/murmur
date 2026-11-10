import { useEffect, useState } from "react";
import { buttonClass, linkClass, primaryClass } from "../components/controls";
import { Mark } from "../components/Mark";
import { ModelCard } from "../components/ModelCard";
import { Hang, Title } from "../components/Page";
import { api, errorText } from "../lib/api";
import { APP_NAME } from "../lib/app";
import { useDictation } from "../store/dictation";
import { useModels } from "../store/models";
import { useHotkeyText } from "../store/settings";

type Mic = { state: "checking" } | { state: "ok" } | { state: "blocked"; message: string };

interface Props {
  onDone: () => void;
  onSeeAllModels: () => void;
}

function Step({ n, children }: { n: number; children: string }) {
  return (
    <>
      <span className="numeral block">{n}</span>
      <span className="mt-1 block">{children}</span>
    </>
  );
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
    <section className="stagger page">
      <Title tone="amber" aside={<Mark activity={activity} className="h-14 w-auto text-ink" />}>
        Welcome to
        <br />
        {APP_NAME}
      </Title>

      <Hang label={<Step n={1}>Microphone</Step>}>
        <div className="py-3">
          {mic.state === "checking" && <p className="animate-breathe text-ink-2">Checking the microphone</p>}
          {mic.state === "ok" && <p className="animate-pop">Microphone ready</p>}
          {mic.state === "blocked" && (
            <div className="flex flex-col items-start gap-3">
              <p className="text-danger">{mic.message}</p>
              <div className="flex items-center gap-4">
                <button className={primaryClass} onClick={() => void api.openMicrophoneSettings()}>
                  Open microphone settings
                </button>
                <button className={buttonClass} onClick={checkMic}>
                  Check again
                </button>
              </div>
            </div>
          )}
        </div>
      </Hang>

      <Hang label={<Step n={2}>Speech model</Step>} className="mt-8">
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
            device={view.device}
          />
        )}
        <div className="border-t border-rule py-3">
          <button className={`${linkClass} text-ink-2`} onClick={onSeeAllModels}>
            See all models
          </button>
        </div>
      </Hang>

      {(ready || anyActive) && (
        <Hang label={<Step n={3}>Start</Step>} className="mt-8 animate-rise">
          <div className="flex flex-col items-start gap-4 py-3">
            <p className="statement max-w-[28ch]">Hold {hotkey}, speak, then let go.</p>
            <button className={primaryClass} onClick={onDone}>
              Start dictating
            </button>
          </div>
        </Hang>
      )}
    </section>
  );
}
