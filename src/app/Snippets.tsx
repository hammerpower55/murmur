import { useEffect } from "react";
import { CsvButtons } from "../components/CsvButtons";
import { Title } from "../components/Page";
import { PairList, type Pair } from "../components/PairList";
import { useText } from "../store/text";

export function Snippets() {
  const { data, load, setSnippets } = useText();

  useEffect(() => {
    void load();
  }, [load]);

  if (!data) return null;

  return (
    <section className="stagger page">
      <Title tone="amber" seed={15} aside={<CsvButtons kind="snippets" onImported={() => void load()} />}>
        Snippets
      </Title>

      <PairList
        pairs={data.snippets.map((s): Pair => [s.trigger, s.text])}
        fields={[
          { label: "Trigger", placeholder: "What you say, like my address" },
          {
            label: "Text",
            placeholder: "Text to paste. {date}, {time} and {clipboard} are filled in.",
            multiline: true,
          },
        ]}
        save={(pairs) => setSnippets(pairs.map(([trigger, text]) => ({ trigger, text })))}
        addLabel="Add snippet"
        empty="Add a snippet to paste longer text with a short phrase."
      />
    </section>
  );
}
