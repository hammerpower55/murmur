import { Stop } from "./Page";

/** An empty state: one flush left sentence, ended by the full stop that comes
 * alive the moment you dictate. */
export function Empty({ children }: { children: string }) {
  return (
    <p className="statement max-w-[28ch] animate-rise py-3 text-balance">
      {children.replace(/\.$/, "")}
      <Stop />
    </p>
  );
}
