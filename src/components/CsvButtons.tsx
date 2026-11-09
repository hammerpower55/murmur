import { useState } from "react";
import { api, errorText, type ListKind } from "../lib/api";
import { plural } from "../lib/format";
import { linkClass } from "./controls";

/** Import and export buttons for a list, with one line of outcome. */
export function CsvButtons({ kind, onImported }: { kind: ListKind; onImported: () => void }) {
  const [status, setStatus] = useState<string | null>(null);
  const run = async (action: () => Promise<string | null>) => {
    setStatus(null);
    try {
      setStatus(await action());
    } catch (err) {
      setStatus(errorText(err));
    }
  };
  return (
    <div className="flex items-center gap-4">
      {status && <span className="text-ink-2">{status}</span>}
      <button
        className={linkClass}
        onClick={() =>
          void run(async () => {
            const count = await api.importCsv(kind);
            if (count === null) return null;
            onImported();
            return `Imported ${plural(count, "row", "rows")}`;
          })
        }
      >
        Import CSV
      </button>
      <button
        className={linkClass}
        onClick={() => void run(async () => ((await api.exportCsv(kind)) ? "Exported" : null))}
      >
        Export CSV
      </button>
    </div>
  );
}
