import type { ReactNode } from "react";

/** Vertical label/value pair. Used inside cards. */
export function KeyValue({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-[11px] uppercase tracking-wider text-muted">
        {label}
      </span>
      <span className="font-mono text-sm">{children}</span>
    </div>
  );
}
