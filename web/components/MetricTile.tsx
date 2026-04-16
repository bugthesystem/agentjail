import type { ReactNode } from "react";
import { Card, CardBody } from "./Card";

/** Single headline metric on the dashboard. */
export function MetricTile({
  label,
  value,
  hint,
}: {
  label: string;
  value: ReactNode;
  hint?: string;
}) {
  return (
    <Card>
      <CardBody className="flex flex-col gap-1">
        <span className="text-[11px] uppercase tracking-wider text-muted">
          {label}
        </span>
        <span className="font-mono text-xl tabular-nums">{value}</span>
        {hint && <span className="text-[11px] text-muted">{hint}</span>}
      </CardBody>
    </Card>
  );
}
