/** Render "3m ago" / "2h ago" from an ISO timestamp. */
export function RelativeTime({ iso }: { iso: string }) {
  const ms = Date.parse(iso);
  if (Number.isNaN(ms)) return <span>-</span>;
  const diff = Date.now() - ms;
  const abs = Math.abs(diff);
  const fmt = (n: number, unit: string) =>
    `${Math.round(n)}${unit}${diff < 0 ? "" : " ago"}`;
  let label = fmt(abs / 1000, "s");
  if (abs > 60_000) label = fmt(abs / 60_000, "m");
  if (abs > 3_600_000) label = fmt(abs / 3_600_000, "h");
  if (abs > 86_400_000) label = fmt(abs / 86_400_000, "d");
  return (
    <time
      title={iso}
      className="tabular-nums"
      dateTime={iso}
    >
      {label}
    </time>
  );
}
