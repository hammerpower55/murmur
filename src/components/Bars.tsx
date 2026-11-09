/** Real numbers drawn as a row of flat bars rising from a baseline. */
export function Bars({
  values,
  hot,
  titles,
  className = "",
}: {
  values: number[];
  /** Index drawn in amber. */
  hot?: number;
  titles?: string[];
  className?: string;
}) {
  const max = Math.max(1, ...values);
  return (
    <div className={`bars ${className}`} aria-hidden>
      {values.map((v, i) => (
        <span
          key={i}
          title={titles?.[i]}
          className={i === hot ? "bg-accent" : "bg-ink"}
          style={{
            transform: `scaleY(${v / max})`,
            animationDelay: `${160 + (i * 480) / Math.max(1, values.length)}ms`,
          }}
        />
      ))}
    </div>
  );
}
