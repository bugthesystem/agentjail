import type { InputHTMLAttributes } from "react";
import { cn } from "@/lib/cn";

export function Input({
  className,
  ...props
}: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={cn(
        "h-9 w-full rounded-md border border-border bg-transparent px-3 text-sm",
        "font-mono placeholder:text-muted focus-visible:outline-none",
        className,
      )}
      {...props}
    />
  );
}
