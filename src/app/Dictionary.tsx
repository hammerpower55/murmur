import { useEffect, useState } from "react";
import { buttonClass, Tabs } from "../components/controls";
import { CsvButtons } from "../components/CsvButtons";
import { PairList, type Pair } from "../components/PairList";
import { useText } from "../store/text";

type Tab = "dictionary" | "romanUrdu";

/** "accha, achha" to ["accha", "achha"]. */
export function splitVariants(text: string): string[] {
  return text
    .split(/[,\s]+/)
    .map((v) => v.trim())
    .filter((v) => v !== "");
}

export function Dictionary() {
  const { data, load, setDictionary, setRomanUrdu, resetRomanUrdu } = useText();
  const [tab, setTab] = useState<Tab>("dictionary");
  const [confirmReset, setConfirmReset] = useState(false);

  useEffect(() => {
    void load();
  }, [load]);

  if (!data) return null;

  return (
    <section className="stagger mx-auto max-w-3xl px-6 py-8">
      <h1 className="text-xl font-semibold text-zinc-100">Dictionary</h1>

      <div className="mt-6 flex items-center gap-1">
        <Tabs
          tabs={[
            ["dictionary", "Words"],
            ["romanUrdu", "Roman Urdu"],
          ]}
          value={tab}
          onChange={(t) => {
            setConfirmReset(false);
            setTab(t);
          }}
        />
        <div key={tab} className="ml-auto animate-pop">
          {tab === "dictionary" ? (
            <CsvButtons kind="dictionary" onImported={() => void load()} />
          ) : confirmReset ? (
            <span className="flex gap-2">
              <button className={buttonClass} onClick={() => setConfirmReset(false)}>
                Cancel
              </button>
              <button
                className={buttonClass}
                onClick={() => {
                  setConfirmReset(false);
                  void resetRomanUrdu();
                }}
              >
                Restore the shipped spellings
              </button>
            </span>
          ) : (
            <button className={buttonClass} onClick={() => setConfirmReset(true)}>
              Restore defaults
            </button>
          )}
        </div>
      </div>

      <div key={tab} className="mt-4 animate-page">
        {tab === "dictionary" ? (
          <PairList
            pairs={data.dictionary.map((e): Pair => [e.spoken, e.written])}
            fields={[
              { label: "Spoken", placeholder: "What you say, like clari tree" },
              { label: "Written", placeholder: "How to write it, like ClariTree" },
            ]}
            save={(pairs) => setDictionary(pairs.map(([spoken, written]) => ({ spoken, written })))}
            addLabel="Add word"
            empty="Add a word to teach Murmur how you spell it."
          />
        ) : (
          <PairList
            pairs={data.romanUrdu.map((r): Pair => [r.canonical, r.variants.join(", ")])}
            fields={[
              { label: "Spelling", placeholder: "Spelling to keep, like acha" },
              { label: "Variants", placeholder: "Spellings to replace, like accha, achha" },
            ]}
            save={(pairs) =>
              setRomanUrdu(pairs.map(([canonical, v]) => ({ canonical: canonical.trim(), variants: splitVariants(v) })))
            }
            addLabel="Add spelling"
            empty="Add a spelling to keep Roman Urdu words consistent."
          />
        )}
      </div>
    </section>
  );
}
