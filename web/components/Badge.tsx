import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

export type BadgeTone = "default" | "success" | "danger" | "accent" | "muted";

export function Badge({
  children,
  tone = "default",
  className,
}: {
  children: ReactNode;
  tone?: BadgeTone;
  className?: string;
}) {
  const tones: Record<BadgeTone, string> = {
    default: "border-border bg-fg/[0.04] text-fg",
    success: "border-success/30 bg-success/10 text-success",
    danger: "border-danger/30 bg-danger/10 text-danger",
    accent: "border-accent/30 bg-accent/10 text-accent",
    muted: "border-border bg-transparent text-muted",
  };
  return (
    <span
      className={cn(
        "inline-flex h-5 items-center rounded border px-1.5 text-[11px] " +
          "font-mono leading-none tracking-tight",
        tones[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}
