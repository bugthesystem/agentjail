/** UI primitives — shadcn-grade, composable, typed. */

import { forwardRef, type ButtonHTMLAttributes, type InputHTMLAttributes, type HTMLAttributes } from "react";
import { cn } from "../lib/cn";

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

const buttonBase =
  "inline-flex items-center justify-center font-medium transition-all duration-150 " +
  "disabled:opacity-40 disabled:pointer-events-none select-none";

const buttonVariants = {
  primary:
    "bg-text text-text-inverse hover:bg-text/90 active:bg-text/80",
  accent:
    "bg-accent text-text-inverse font-semibold hover:bg-accent-hover active:bg-accent-dim",
  secondary:
    "bg-bg-emphasis text-text hover:bg-bg-emphasis/80 border border-border",
  ghost:
    "text-text-secondary hover:text-text hover:bg-bg-emphasis/50",
  danger:
    "text-error hover:bg-error/10 border border-transparent hover:border-error/20",
} as const;

const buttonSizes = {
  xs: "h-7 px-2.5 text-xs rounded-md gap-1",
  sm: "h-8 px-3 text-[13px] rounded-[var(--radius-lg)] gap-1.5",
  md: "h-9 px-4 text-sm rounded-[var(--radius-lg)] gap-2",
  lg: "h-10 px-5 text-sm rounded-[var(--radius-lg)] gap-2",
} as const;

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: keyof typeof buttonVariants;
  size?: keyof typeof buttonSizes;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "primary", size = "md", ...props }, ref) => (
    <button
      ref={ref}
      className={cn(buttonBase, buttonVariants[variant], buttonSizes[size], className)}
      {...props}
    />
  ),
);

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  error?: string;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ className, label, error, id, ...props }, ref) => {
    const inputId = id ?? label?.toLowerCase().replace(/\s+/g, "-");
    return (
      <div className="space-y-2">
        {label && (
          <label htmlFor={inputId} className="block text-[13px] font-medium text-text-secondary">
            {label}
          </label>
        )}
        <input
          ref={ref}
          id={inputId}
          className={cn(
            "w-full h-9 px-3 rounded-[var(--radius-lg)] text-sm bg-bg border transition-colors duration-150",
            "placeholder:text-text-tertiary",
            "focus:outline-none focus:ring-1 focus:ring-accent/40 focus:border-accent/30",
            error ? "border-error/40" : "border-border hover:border-border/80",
            className,
          )}
          {...props}
        />
        {error && <p className="text-xs text-error">{error}</p>}
      </div>
    );
  },
);

// ---------------------------------------------------------------------------
// Card
// ---------------------------------------------------------------------------

export function Card({ className, children, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn("rounded-xl border border-border bg-bg-subtle", className)}
      {...props}
    >
      {children}
    </div>
  );
}

export function CardHeader({ className, children, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={cn("px-5 py-3.5 border-b border-border", className)} {...props}>
      {children}
    </div>
  );
}

export function CardBody({ className, children, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("px-5 py-4", className)} {...props}>{children}</div>;
}

// ---------------------------------------------------------------------------
// Badge
// ---------------------------------------------------------------------------

const badgeVariants = {
  default: "bg-bg-emphasis text-text-secondary",
  success: "bg-success/10 text-success",
  warning: "bg-warning/10 text-warning",
  error: "bg-error/10 text-error",
  accent: "bg-accent/8 text-accent",
} as const;

interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: keyof typeof badgeVariants;
}

export function Badge({ className, variant = "default", ...props }: BadgeProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center px-1.5 py-px rounded text-[11px] font-medium",
        badgeVariants[variant],
        className,
      )}
      {...props}
    />
  );
}

// ---------------------------------------------------------------------------
// Kbd
// ---------------------------------------------------------------------------

export function Kbd({ children }: { children: string }) {
  return (
    <kbd className="ml-auto text-[10px] text-text-tertiary/60 font-mono tracking-wider">
      {children}
    </kbd>
  );
}

// ---------------------------------------------------------------------------
// Empty State
// ---------------------------------------------------------------------------

interface EmptyStateProps {
  icon?: React.ReactNode;
  title: string;
  description?: string;
  action?: React.ReactNode;
}

export function EmptyState({ icon, title, description, action }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center py-16 px-4 text-center animate-fade-in">
      {icon && <div className="mb-3 text-text-tertiary/50">{icon}</div>}
      <h3 className="text-[13px] font-medium text-text-secondary">{title}</h3>
      {description && <p className="mt-1 text-[13px] text-text-tertiary max-w-xs">{description}</p>}
      {action && <div className="mt-4 w-full max-w-sm">{action}</div>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Code Block
// ---------------------------------------------------------------------------

interface CodeBlockProps {
  children: string;
  language?: string;
}

export function CodeBlock({ children, language }: CodeBlockProps) {
  const copy = () => navigator.clipboard.writeText(children);
  return (
    <div className="relative group rounded-[var(--radius-lg)] border border-border bg-bg overflow-hidden">
      {language && (
        <div className="px-3.5 py-2 border-b border-border flex items-center justify-between">
          <span className="text-[10px] text-text-tertiary font-mono uppercase tracking-widest">{language}</span>
          <button
            onClick={copy}
            className="opacity-0 group-hover:opacity-100 transition-opacity text-[11px] text-text-tertiary hover:text-text"
            aria-label="Copy"
          >
            Copy
          </button>
        </div>
      )}
      <pre className="px-3.5 py-3 text-[13px] font-mono text-text-secondary leading-relaxed overflow-x-auto">
        <code>{children}</code>
      </pre>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Metric
// ---------------------------------------------------------------------------

interface MetricProps {
  label: string;
  value: number | string;
  hint?: string;
  accent?: boolean;
}

export function Metric({ label, value, hint, accent }: MetricProps) {
  return (
    <div className={cn(
      "rounded-xl border px-4 py-3.5",
      accent ? "border-accent/20 bg-accent/[0.03]" : "border-border bg-bg-subtle",
    )}>
      <p className="text-[11px] font-medium text-text-tertiary uppercase tracking-wider">{label}</p>
      <p className={cn("mt-1 text-xl font-semibold tabular-nums", accent && "text-accent")}>{value}</p>
      {hint && <p className="mt-0.5 text-[11px] text-text-tertiary">{hint}</p>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// StatusDot
// ---------------------------------------------------------------------------

export function StatusDot({ status }: { status: "active" | "idle" | "error" }) {
  const c = { active: "bg-success", idle: "bg-text-tertiary/50", error: "bg-error" };
  return <span className={cn("inline-block w-1.5 h-1.5 rounded-full", c[status])} />;
}
