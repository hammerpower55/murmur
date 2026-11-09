import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useRef, useState } from "react";
import {
  buttonClass,
  Keys,
  Row,
  Section,
  selectClass,
  Slider,
  Toggle,
  inputClass,
} from "../components/controls";
import { api, errorText, events, type OverlayPosition, type Settings as Prefs } from "../lib/api";
import { SOURCE_URL } from "../lib/app";
import { hotkeyLabel, keyLabel, recordHotkey } from "../lib/keys";
import { languagesFor } from "../lib/languages";
import { IS_MAC } from "../lib/platform";
import { useInsights } from "../store/insights";
import { useModels } from "../store/models";
import { useSettings } from "../store/settings";

const UNLOAD_CHOICES: [number, string][] = [
  [0, "Never"],
  [5, "After 5 min"],
  [10, "After 10 min"],
  [30, "After 30 min"],
  [60, "After 1 hour"],
];

const POSITIONS: [OverlayPosition, string][] = [
  ["bottomCentre", "Bottom centre"],
  ["topCentre", "Top centre"],
  ["bottomRight", "Bottom right"],
];

/** A value that follows the control at once and saves after a short pause. */
function useDebounced(value: number, save: (v: number) => void, ms = 350) {
  const [local, setLocal] = useState(value);
  const timer = useRef(0);
  useEffect(() => setLocal(value), [value]);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const change = (v: number) => {
    setLocal(v);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => save(v), ms);
  };
  return [local, change] as const;
}

function HotkeyRow({
  label,
  keys,
  onChange,
  disabled,
}: {
  label: string;
  keys: string[];
  onChange: (keys: string[]) => void;
  disabled?: boolean;
}) {
  const [capturing, setCapturing] = useState(false);
  const capture = async () => {
    setCapturing(true);
    // Focus leaves the button so Space or Enter is recorded, not a click.
    (document.activeElement as HTMLElement | null)?.blur();
    await api.hotkeyRecording(true).catch(() => {});
    let pressed: string[] | null = null;
    try {
      pressed = await recordHotkey();
    } finally {
      await api.hotkeyRecording(false, pressed).catch(() => {});
      setCapturing(false);
    }
    if (pressed) onChange(pressed);
  };
  return (
    <Row label={label}>
      {capturing ? (
        <span className="animate-breathe text-sm font-medium text-accent">Press keys</span>
      ) : (
        <span title={hotkeyLabel(keys)} className={disabled ? "opacity-40" : ""}>
          <Keys names={keys} label={keyLabel} />
        </span>
      )}
      <button className={buttonClass} disabled={capturing || disabled} onClick={() => void capture()}>
        Change
      </button>
    </Row>
  );
}

function MicrophoneTest() {
  const [on, setOn] = useState(false);
  const [level, setLevel] = useState(0);
  useEffect(() => {
    if (!on) return;
    let stop = () => {};
    void events.micLevel((rms) => setLevel(rms)).then((un) => (stop = un));
    void api.testMicrophone(true);
    // Rust stops the meter after a minute too.
    const limit = window.setTimeout(() => setOn(false), 60_000);
    return () => {
      window.clearTimeout(limit);
      stop();
      void api.testMicrophone(false);
      setLevel(0);
    };
  }, [on]);
  return (
    <Row label="Test microphone">
      <div className="h-2 w-40 overflow-hidden rounded-full bg-zinc-800" aria-hidden>
        <div
          className="h-full origin-left rounded-full bg-accent transition-transform duration-75 ease-out"
          style={{ transform: `scaleX(${Math.min(1, level)})` }}
        />
      </div>
      <button className={`${buttonClass} w-16`} onClick={() => setOn(!on)}>
        {on ? "Stop" : "Start"}
      </button>
    </Row>
  );
}

function ClearHistory() {
  const [confirming, setConfirming] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const clear = async () => {
    setConfirming(false);
    try {
      await api.clearHistory();
      void useInsights.getState().refresh();
      setStatus("History cleared");
    } catch (err) {
      setStatus(errorText(err));
    }
  };
  return (
    <Row label="Clear history">
      {status && <span className="text-sm text-zinc-400">{status}</span>}
      {confirming ? (
        <>
          <button className={buttonClass} onClick={() => setConfirming(false)}>
            Cancel
          </button>
          <button
            onClick={() => void clear()}
            className="rounded-md bg-red-400 px-3 py-1.5 text-sm font-medium text-zinc-950 hover:bg-red-300"
          >
            Delete every transcript
          </button>
        </>
      ) : (
        <button
          className={buttonClass}
          onClick={() => {
            setStatus(null);
            setConfirming(true);
          }}
        >
          Clear
        </button>
      )}
    </Row>
  );
}

export function Settings({ onOpenModels }: { onOpenModels: () => void }) {
  const { settings: s, error, load, update, clearError } = useSettings();
  const view = useModels((m) => m.view);
  const [mics, setMics] = useState<string[]>([]);
  const [version, setVersion] = useState("");

  useEffect(() => {
    void load();
    void api.listMicrophones().then(setMics);
    void api.appInfo().then((i) => setVersion(i.version));
  }, [load]);

  const set = (patch: Partial<Prefs>) => void update(patch);
  const [gain, setGain] = useDebounced(s?.inputGain ?? 100, (v) => set({ inputGain: v }));
  const [volume, setVolume] = useDebounced(s?.soundVolume ?? 60, (v) => set({ soundVolume: v }));
  const [wpm, setWpm] = useState("");
  useEffect(() => setWpm(String(s?.typingWpm ?? "")), [s?.typingWpm]);

  if (!s) return null;

  const active = view?.models.find((m) => m.id === view.active);
  const whisper = active?.family === "whisper";
  const fixerInstalled = view?.models.some((m) => m.family === "fixup" && m.installed) ?? false;
  const pinnedToEnglish = whisper && s.romanUrdu;
  const languages = [...languagesFor(active?.languages ?? ["*"])];
  if (s.language !== "auto" && !languages.some(([c]) => c === s.language)) {
    languages.unshift([s.language, languagesFor(["*"]).find(([c]) => c === s.language)?.[1] ?? s.language]);
  }
  const micMissing = s.microphone !== null && !mics.includes(s.microphone);

  const saveWpm = () => {
    const n = Number(wpm);
    if (Number.isInteger(n) && n !== s.typingWpm) set({ typingWpm: n });
    else setWpm(String(s.typingWpm));
  };

  return (
    <section className="stagger page pb-24">
      <h1 className="page-title">Settings</h1>

      <Section title="General">
        <Row label="Launch at login">
          <Toggle label="Launch at login" checked={s.launchAtLogin} onChange={(v) => set({ launchAtLogin: v })} />
        </Row>
        <Row label="Show in tray">
          <Toggle label="Show in tray" checked={s.showInTray} onChange={(v) => set({ showInTray: v })} />
        </Row>
        <Row label="Close to tray">
          <Toggle
            label="Close to tray"
            checked={s.closeToTray}
            disabled={!s.showInTray}
            onChange={(v) => set({ closeToTray: v })}
          />
        </Row>
        <Row label="Theme">
          <span className="text-sm text-zinc-400">Dark</span>
        </Row>
      </Section>

      <Section title="Hotkeys">
        <HotkeyRow label="Push to talk" keys={s.mainHotkey} onChange={(k) => set({ mainHotkey: k })} />
        <Row label="Hands free hotkey">
          <Toggle
            label="Hands free hotkey"
            checked={s.toggleHotkeyEnabled}
            onChange={(v) => set({ toggleHotkeyEnabled: v })}
          />
        </Row>
        <HotkeyRow
          label="Hands free keys"
          keys={s.toggleHotkey}
          disabled={!s.toggleHotkeyEnabled}
          onChange={(k) => set({ toggleHotkey: k })}
        />
        <Row label="Screenplay hotkey">
          <Toggle
            label="Screenplay hotkey"
            checked={s.scriptHotkeyEnabled}
            onChange={(v) => set({ scriptHotkeyEnabled: v })}
          />
        </Row>
        <HotkeyRow
          label="Screenplay keys"
          keys={s.scriptHotkey}
          disabled={!s.scriptHotkeyEnabled}
          onChange={(k) => set({ scriptHotkey: k })}
        />
      </Section>

      <Section title="Microphone">
        <Row label="Microphone" htmlFor="mic">
          <select
            id="mic"
            className={selectClass}
            value={s.microphone ?? ""}
            onChange={(e) => set({ microphone: e.target.value || null })}
          >
            <option value="">{IS_MAC ? "System default" : "Windows default"}</option>
            {micMissing && <option value={s.microphone ?? ""}>{s.microphone} (not connected)</option>}
            {mics.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        </Row>
        <Row label="Input gain">
          <Slider label="Input gain" value={gain} min={25} max={400} step={5} onChange={setGain} format={(v) => `${v}%`} />
        </Row>
        <MicrophoneTest />
      </Section>

      <Section title="Models">
        <Row label="Active model">
          <span className="text-sm text-zinc-400">{active?.name ?? "None"}</span>
          <button className={buttonClass} onClick={onOpenModels}>
            Change
          </button>
        </Row>
        <Row label="Unload model when idle" htmlFor="unload">
          <select
            id="unload"
            className={selectClass}
            value={s.unloadAfterMinutes}
            onChange={(e) => set({ unloadAfterMinutes: Number(e.target.value) })}
          >
            {UNLOAD_CHOICES.map(([v, label]) => (
              <option key={v} value={v}>
                {label}
              </option>
            ))}
          </select>
        </Row>
      </Section>

      <Section title="Language">
        <Row label="Language" htmlFor="language">
          <select
            id="language"
            className={selectClass}
            disabled={pinnedToEnglish}
            value={pinnedToEnglish ? "en" : s.language}
            onChange={(e) => set({ language: e.target.value })}
          >
            <option value="auto">Auto detect</option>
            {languages.map(([code, name]) => (
              <option key={code} value={code}>
                {name}
              </option>
            ))}
          </select>
        </Row>
        <Row label="Roman Urdu mixing">
          <Toggle label="Roman Urdu mixing" checked={s.romanUrdu} onChange={(v) => set({ romanUrdu: v })} />
        </Row>
      </Section>

      <Section title="Overlay">
        <Row label="Show overlay">
          <Toggle label="Show overlay" checked={s.overlayEnabled} onChange={(v) => set({ overlayEnabled: v })} />
        </Row>
        <Row label="Position" htmlFor="position">
          <select
            id="position"
            className={selectClass}
            disabled={!s.overlayEnabled}
            value={s.overlayPosition}
            onChange={(e) => set({ overlayPosition: e.target.value as OverlayPosition })}
          >
            {POSITIONS.map(([v, label]) => (
              <option key={v} value={v}>
                {label}
              </option>
            ))}
          </select>
        </Row>
        <Row label="Wave colour" htmlFor="colour">
          <input
            id="colour"
            type="color"
            disabled={!s.overlayEnabled}
            value={s.waveColour}
            onChange={(e) => set({ waveColour: e.target.value })}
            className="h-7 w-10 cursor-pointer rounded border border-white/10 bg-transparent disabled:opacity-40"
          />
        </Row>
      </Section>

      <Section title="Sounds">
        <Row label="Play sounds">
          <Toggle label="Play sounds" checked={s.soundsEnabled} onChange={(v) => set({ soundsEnabled: v })} />
        </Row>
        <Row label="Volume">
          <Slider
            label="Volume"
            value={volume}
            min={0}
            max={100}
            disabled={!s.soundsEnabled}
            onChange={setVolume}
            format={(v) => `${v}`}
          />
        </Row>
      </Section>

      <Section title="Text">
        <Row label="Smart formatting">
          <Toggle label="Smart formatting" checked={s.smartFormatting} onChange={(v) => set({ smartFormatting: v })} />
        </Row>
        <Row label="Fix misheard words">
          {s.fixMisheardWords && !fixerInstalled && (
            <button className={buttonClass} onClick={onOpenModels}>
              Download the word fixer
            </button>
          )}
          <Toggle
            label="Fix misheard words"
            checked={s.fixMisheardWords}
            onChange={(v) => set({ fixMisheardWords: v })}
          />
        </Row>
        <Row label="Check every word, not only your own words">
          <Toggle
            label="Check every word"
            checked={s.fixAnyWord}
            disabled={!s.fixMisheardWords}
            onChange={(v) => set({ fixAnyWord: v })}
          />
        </Row>
        {view?.npu && (
          <Row label="Use the NPU">
            <Toggle label="Use the NPU" checked={s.useNpu} onChange={(v) => set({ useNpu: v })} />
          </Row>
        )}
        <Row label="Learn words from your corrections">
          <Toggle
            label="Learn words from your corrections"
            checked={s.learnWords}
            onChange={(v) => set({ learnWords: v })}
          />
        </Row>
        <Row label="Remove filler words">
          <Toggle label="Remove filler words" checked={s.removeFillers} onChange={(v) => set({ removeFillers: v })} />
        </Row>
        <Row label="Add space after dictation">
          <Toggle
            label="Add space after dictation"
            checked={s.addTrailingSpace}
            onChange={(v) => set({ addTrailingSpace: v })}
          />
        </Row>
        <Row label="Voice punctuation">
          <Toggle
            label="Voice punctuation"
            checked={s.voicePunctuation}
            onChange={(v) => set({ voicePunctuation: v })}
          />
        </Row>
        <Row label="Typing speed for time saved" htmlFor="wpm">
          <input
            id="wpm"
            inputMode="numeric"
            value={wpm}
            onChange={(e) => setWpm(e.target.value.replace(/\D/g, ""))}
            onBlur={saveWpm}
            onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
            className={`${inputClass} w-16 text-right tabular-nums`}
          />
          <span className="text-sm text-zinc-400">wpm</span>
        </Row>
      </Section>

      <Section title="Privacy">
        <Row label="Keep audio clips">
          <Toggle label="Keep audio clips" checked={s.keepAudioClips} onChange={(v) => set({ keepAudioClips: v })} />
        </Row>
        <ClearHistory />
        <Row label="Data folder">
          <button className={buttonClass} onClick={() => void api.openDataFolder()}>
            Open
          </button>
        </Row>
      </Section>

      <Section title="About">
        <Row label="Version">
          <span className="text-sm text-zinc-400 tabular-nums">{version}</span>
        </Row>
        <Row label="Open source licences">
          <button className={buttonClass} onClick={() => void api.openLicences()}>
            Open
          </button>
        </Row>
        {SOURCE_URL && (
          <Row label="Source code">
            <button className={buttonClass} onClick={() => void openUrl(SOURCE_URL)}>
              Open
            </button>
          </Row>
        )}
      </Section>

      {error && (
        <div
          role="alert"
          className="surface fixed inset-x-0 bottom-6 mx-auto flex w-fit max-w-[90%] animate-rise items-center gap-4 rounded-lg px-4 py-2.5 text-sm text-red-200"
        >
          {error}
          <button className="text-zinc-400 hover:text-zinc-200" onClick={clearError}>
            Close
          </button>
        </div>
      )}
    </section>
  );
}
