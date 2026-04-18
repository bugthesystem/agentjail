import type { ButtonHTMLAttributes } from "react";
import { cn } from "@/lib/cn";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "default" | "ghost" | "danger";
  size?: "sm" | "md";
}

export function Button({
  variant = "default",
  size = "md",
  className,
  ...props
}: ButtonProps) {
  const base =
    "inline-flex items-center justify-center gap-1.5 rounded-md font-medium " +
    "border transition-colors disabled:opacity-50 disabled:pointer-events-none " +
    "focus-visible:outline-none";
  const sizes = { sm: "h-7 px-2.5 text-xs", md: "h-9 px-3 text-sm" };
  const variants = {
    default:
      "border-border bg-fg/[0.04] text-fg hover:bg-fg/[0.08]",
    ghost:
      "border-transparent text-muted hover:text-fg hover:bg-fg/[0.04]",
    danger:
      "border-danger/30 bg-danger/10 text-danger hover:bg-danger/20",
  };
  return (
    <button
      className={cn(base, sizes[size], variants[variant], className)}
      {...props}
    />
  );
}
