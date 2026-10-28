import { useCountUp } from "../lib/motion";

/** A number that eases up to its value when it appears or changes. */
export function CountUp({ value, format }: { value: number; format: (n: number) => string }) {
  const shown = useCountUp(value);
  return <>{format(Math.round(shown))}</>;
}
