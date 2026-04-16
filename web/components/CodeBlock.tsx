import { cn } from "@/lib/cn";

/** Minimal, non-syntax-highlighted code block. */
export function CodeBlock({
  code,
  className,
}: {
  code: string;
  className?: string;
}) {
  return (
    <pre
      className={cn(
        "overflow-x-auto rounded-md border border-border bg-fg/[0.02] " +
          "p-3 font-mono text-[12px] leading-5",
        className,
      )}
    >
      <code>{code}</code>
    </pre>
  );
}
