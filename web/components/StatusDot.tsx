import { cn } from "@/lib/cn";

/** Tiny coloured dot that encodes HTTP status at a glance. */
export function StatusDot({ status }: { status: number }) {
  const tone =
    status >= 500
      ? "bg-danger"
      : status >= 400
        ? "bg-danger/70"
        : status >= 300
          ? "bg-accent"
          : "bg-success";
  return (
    <span
      aria-label={`status ${status}`}
      className={cn("inline-block h-1.5 w-1.5 rounded-full", tone)}
    />
  );
}
