import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Dictionary } from "./app/Dictionary";
import { History } from "./app/History";
import { Home } from "./app/Home";
import { Insights } from "./app/Insights";
import { Models } from "./app/Models";
import { Onboarding } from "./app/Onboarding";
import { Settings } from "./app/Settings";
import { Snippets } from "./app/Snippets";
import { Mark } from "./components/Mark";
import { api } from "./lib/api";
import { APP_NAME } from "./lib/app";
import { subscribeDictation, useDictation } from "./store/dictation";
import { subscribeModels } from "./store/models";

type Page = "home" | "insights" | "history" | "dictionary" | "snippets" | "models" | "settings";
type Screen = "loading" | "onboarding" | Page;

const NAV: { id: Page; label: string }[] = [
  { id: "home", label: "Home" },
  { id: "insights", label: "Insights" },
  { id: "history", label: "History" },
  { id: "dictionary", label: "Dictionary" },
  { id: "snippets", label: "Snippets" },
  { id: "models", label: "Models" },
  { id: "settings", label: "Settings" },
];

/** The nav with a marker that slides to the page you are on. */
function Nav({ screen, go }: { screen: Screen; go: (p: Page) => void }) {
  const list = useRef<HTMLDivElement>(null);
  const [marker, setMarker] = useState<{ y: number; h: number } | null>(null);
  // No slide on the very first placement, only between pages.
  const [placed, setPlaced] = useState(false);
  const activity = useDictation((d) => d.activity);

  useLayoutEffect(() => {
    const el = list.current?.querySelector<HTMLElement>(`[data-nav="${screen}"]`);
    if (el) setMarker({ y: el.offsetTop, h: el.offsetHeight });
  }, [screen]);
  useEffect(() => {
    if (marker && !placed) requestAnimationFrame(() => setPlaced(true));
  }, [marker, placed]);

  return (
    <nav className="flex w-56 shrink-0 flex-col border-r border-white/6 bg-zinc-950/40 px-4 py-6">
      <span className="flex items-center gap-3 px-3 pb-7 font-display text-[1.25rem] leading-none text-zinc-50">
        <Mark activity={activity} className="h-3.5 w-auto shrink-0 text-zinc-100" />
        {APP_NAME}
      </span>
      <div ref={list} className="relative flex flex-col gap-1">
        {marker && (
          <span
            aria-hidden
            className="absolute inset-x-0 top-0 rounded-md bg-white/7 shadow-[inset_0_1px_0_oklch(1_0_0/0.05)]"
            style={{
              height: marker.h,
              transform: `translateY(${marker.y}px)`,
              transition: placed ? "transform 420ms var(--ease-out-quint), height 420ms var(--ease-out-quint)" : "none",
            }}
          >
            <span className="absolute top-1/2 left-1.5 h-1.5 w-1.5 -translate-y-1/2 rounded-full bg-accent" />
          </span>
        )}
        {NAV.map((item) => (
          <button
            key={item.id}
            data-nav={item.id}
            onClick={() => go(item.id)}
            aria-current={screen === item.id ? "page" : undefined}
            className={`relative rounded-md px-3 py-2.5 pl-6 text-left text-sm ${
              screen === item.id ? "text-zinc-100" : "text-zinc-400 hover:bg-white/4 hover:text-zinc-200"
            }`}
          >
            {item.label}
          </button>
        ))}
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

  const leaveOnboarding = (to: Page) => {
    void api.finishOnboarding();
    setScreen(to);
  };

  if (screen === "loading") return <main className="h-full" />;
  if (screen === "onboarding") {
    return (
      <main className="h-full overflow-y-auto">
        <Onboarding onDone={() => leaveOnboarding("home")} onSeeAllModels={() => leaveOnboarding("models")} />
      </main>
    );
  }

  return (
    <div className="flex h-full">
      <Nav screen={screen} go={setScreen} />
      <main ref={main} className="min-w-0 flex-1 overflow-y-auto">
        {/* Keyed by page so each visit plays its entrance. */}
        <div key={screen} className="animate-page">
          {screen === "home" && <Home onSeeHistory={() => setScreen("history")} />}
          {screen === "insights" && <Insights />}
          {screen === "history" && <History />}
          {screen === "dictionary" && <Dictionary />}
          {screen === "snippets" && <Snippets />}
          {screen === "models" && <Models />}
          {screen === "settings" && <Settings onOpenModels={() => setScreen("models")} />}
        </div>
      </main>
    </div>
  );
}

export default App;
