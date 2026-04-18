import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

/** A single keyboard-shortcut hint. */
export function Kbd({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <kbd
      className={cn(
        "rounded border border-border bg-fg/[0.04] px-1.5 font-mono " +
          "text-[11px] leading-5 text-muted",
        className,
      )}
    >
      {children}
    </kbd>
  );
}
