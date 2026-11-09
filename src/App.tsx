import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Chat } from "./app/Chat";
import { Dictionary } from "./app/Dictionary";
import { History } from "./app/History";
import { Home } from "./app/Home";
import { Insights } from "./app/Insights";
import { Models } from "./app/Models";
import { Onboarding } from "./app/Onboarding";
import { Settings } from "./app/Settings";
import { Snippets } from "./app/Snippets";
import { Mark } from "./components/Mark";
import { Wave } from "./components/Wave";
import { api, events } from "./lib/api";
import { APP_NAME } from "./lib/app";
import { hotkeyLabel } from "./lib/keys";
import { withViewTransition } from "./lib/motion";
import { subscribeDictation, useDictation } from "./store/dictation";
import { subscribeModels, useModels } from "./store/models";
import { useHotkeyText, useSettings } from "./store/settings";

type Page = "home" | "insights" | "history" | "dictionary" | "snippets" | "models" | "chat" | "settings";
type Screen = "loading" | "onboarding" | Page;

const NAV: { id: Page; label: string }[] = [
  { id: "home", label: "Home" },
  { id: "insights", label: "Insights" },
  { id: "history", label: "History" },
  { id: "dictionary", label: "Dictionary" },
  { id: "snippets", label: "Snippets" },
  { id: "models", label: "Models" },
  { id: "chat", label: "Chat" },
  { id: "settings", label: "Settings" },
];

/** Half the marker, which sits centred on the rule between nav and page. */
const MARKER_R = 3.5;

/** The foot of the nav: what the app is doing, or the key that starts it, and
 * the model that will write it down. */
function Status() {
  const activity = useDictation((d) => d.activity);
  const view = useModels((m) => m.view);
  useHotkeyText();
  const keys = useSettings((s) => s.settings?.mainHotkey);
  const model = view?.models.find((m) => m.id === view.active);
  return (
    <div className="text-caption">
      {activity === "idle" ? (
        keys && <p className="font-medium">{hotkeyLabel(keys)}</p>
      ) : (
        <p className="flex animate-pop items-center gap-2 font-medium">
          <Wave activity={activity} />
          {activity === "recording" ? "Listening" : "Transcribing"}
        </p>
      )}
      {model && (
        <p className="mt-0.5 line-clamp-2 text-ink-3" title={model.name}>
          {model.name}
        </p>
      )}
    </div>
  );
}

/** A numbered contents list. The amber full stop rides the rule beside it and
 * slides to the page you are on. */
function Nav({ screen, go }: { screen: Screen; go: (p: Page) => void }) {
  const list = useRef<HTMLDivElement>(null);
  const [marker, setMarker] = useState<number | null>(null);
  // No slide on the very first placement, only between pages.
  const [placed, setPlaced] = useState(false);
  const activity = useDictation((d) => d.activity);

  useLayoutEffect(() => {
    const el = list.current?.querySelector<HTMLElement>(`[data-nav="${screen}"]`);
    if (el) setMarker(el.offsetTop + el.offsetHeight / 2);
  }, [screen]);
  useEffect(() => {
    if (marker !== null && !placed) requestAnimationFrame(() => setPlaced(true));
  }, [marker, placed]);

  return (
    <nav className="relative z-10 flex w-44 shrink-0 flex-col border-r border-ink px-5 pt-8 pb-6">
      <span className="flex items-center gap-2 pb-10 font-extrabold tracking-tight">
        <Mark activity={activity} className="h-3 w-auto shrink-0" />
        {APP_NAME}
      </span>
      <div ref={list} className="relative flex flex-col">
        {marker !== null && (
          <span
            aria-hidden
            className="absolute top-0 h-[7px] w-[7px] rounded-full bg-accent shadow-[0_0_0_2px_var(--paper)]"
            style={{
              // Over the nav's padding (1.25 rem) to the middle of its rule.
              right: `calc(-1.25rem - ${MARKER_R + 0.5}px)`,
              transform: `translateY(${marker - MARKER_R}px)`,
              transition: placed ? "transform 360ms var(--ease-out-quint)" : "none",
            }}
          />
        )}
        {NAV.map((item, i) => (
          <button
            key={item.id}
            data-nav={item.id}
            onClick={() => go(item.id)}
            aria-current={screen === item.id ? "page" : undefined}
            className={`group flex items-baseline gap-2.5 py-1.5 text-left ${
              screen === item.id ? "font-bold text-ink" : "text-ink-2 hover:text-ink"
            }`}
          >
            <span
              className={`w-4 text-caption font-normal tabular-nums transition-colors duration-200 ${
                screen === item.id ? "text-accent-text" : "text-ink-3 group-hover:text-accent-text"
              }`}
              aria-hidden
            >
              {String(i + 1).padStart(2, "0")}
            </span>
            <span className="transition-transform duration-300 ease-[var(--ease-out-quint)] group-hover:translate-x-1">
              {item.label}
            </span>
          </button>
        ))}
      </div>
      <div className="mt-auto pt-6">
        <Status />
      </div>
    </nav>
  );
}

function App() {
  const [screen, setScreen] = useState<Screen>("loading");
  const main = useRef<HTMLElement>(null);
  // A new page starts at the top, not where the last one was scrolled to.
  useLayoutEffect(() => {
    main.current?.scrollTo({ top: 0 });
  }, [screen]);

  useEffect(() => {
    subscribeModels();
    subscribeDictation();
    api
      .onboardingNeeded()
      .then((needed) => setScreen(needed ? "onboarding" : "home"))
      .catch(() => setScreen("home"));
  }, []);

  /** The old page lifts away and the new one plays its entrance. */
  const go = (to: Page) => withViewTransition(() => setScreen(to));

  const leaveOnboarding = (to: Page) => {
    void api.finishOnboarding();
    go(to);
  };

  if (screen === "loading") return <main className="h-full" />;
  if (screen === "onboarding") {
    return (
      <main className="@container/page h-full overflow-y-auto">
        <Onboarding onDone={() => leaveOnboarding("home")} onSeeAllModels={() => leaveOnboarding("models")} />
      </main>
    );
  }

  return (
    <div className="flex h-full">
      <LearnedNote onOpen={() => go("dictionary")} />
      <Nav screen={screen} go={go} />
      <main ref={main} className="@container/page min-w-0 flex-1 overflow-y-auto">
        {/* Keyed by page so each visit plays its entrance. */}
        <div key={screen} className={screen === "chat" ? "h-full" : ""}>
          {screen === "home" && <Home onSeeHistory={() => go("history")} />}
          {screen === "insights" && <Insights />}
          {screen === "history" && <History />}
          {screen === "dictionary" && <Dictionary />}
          {screen === "snippets" && <Snippets />}
          {screen === "models" && <Models onOpenChat={() => go("chat")} />}
          {screen === "chat" && <Chat onOpenModels={() => go("models")} />}
          {screen === "settings" && <Settings onOpenModels={() => go("models")} />}
        </div>
      </main>
    </div>
  );
}

/** A short note in the corner when a word is learned from your corrections. */
function LearnedNote({ onOpen }: { onOpen: () => void }) {
  const [words, setWords] = useState<string[]>([]);
  useEffect(() => {
    let timer = 0;
    const off = events.wordsLearned((entries) => {
      setWords(entries.map((e) => e.written));
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setWords([]), 6000);
    });
    return () => {
      window.clearTimeout(timer);
      void off.then((f) => f());
    };
  }, []);
  if (words.length === 0) return null;
  return (
    <button
      key={words.join()}
      onClick={() => {
        setWords([]);
        onOpen();
      }}
      className="btn-primary fixed right-6 bottom-6 z-50 animate-rise font-normal"
    >
      Learned&nbsp;<strong className="font-semibold">{words.join(", ")}</strong>
    </button>
  );
}

export default App;
