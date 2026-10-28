import { useEffect, useState } from "react";
import { Models } from "./app/Models";
import { Onboarding } from "./app/Onboarding";
import { api } from "./lib/api";
import { APP_NAME } from "./lib/app";
import { subscribeModels } from "./store/models";

type Screen = "loading" | "onboarding" | "models";

/** Pages are added to this list as later phases build them. */
const NAV: { id: Exclude<Screen, "loading" | "onboarding">; label: string }[] = [
  { id: "models", label: "Models" },
];

function App() {
  const [screen, setScreen] = useState<Screen>("loading");

  useEffect(() => {
    subscribeModels();
    api
      .onboardingNeeded()
      .then((needed) => setScreen(needed ? "onboarding" : "models"))
      .catch(() => setScreen("models"));
  }, []);

  const leaveOnboarding = () => {
    void api.finishOnboarding();
    setScreen("models");
  };

  if (screen === "loading") return <main className="h-full" />;
  if (screen === "onboarding") {
    return (
      <main className="h-full overflow-y-auto">
        <Onboarding onDone={leaveOnboarding} onSeeAllModels={leaveOnboarding} />
      </main>
    );
  }

  return (
    <div className="flex h-full">
      <nav className="flex w-52 shrink-0 flex-col gap-1 border-r border-white/8 px-3 py-5">
        <span className="px-3 pb-4 text-sm font-semibold text-zinc-100">{APP_NAME}</span>
        {NAV.map((item) => (
          <button
            key={item.id}
            onClick={() => setScreen(item.id)}
            className={`rounded-md px-3 py-2 text-left text-sm ${
              screen === item.id ? "bg-white/8 text-zinc-100" : "text-zinc-400 hover:bg-white/5 hover:text-zinc-200"
            }`}
          >
            {item.label}
          </button>
        ))}
      </nav>
      <main className="min-w-0 flex-1 overflow-y-auto">{screen === "models" && <Models />}</main>
    </div>
  );
}

export default App;
