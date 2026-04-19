/** Composable UI primitives — refined, no bloat. */

import { forwardRef, type ButtonHTMLAttributes, type InputHTMLAttributes, type HTMLAttributes } from "react";
import { cn } from "../lib/cn";

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

const buttonVariants = {
  primary:
    "bg-gradient-to-b from-accent to-accent-dim text-text-inverse font-semibold " +
    "hover:from-accent-hover hover:to-accent shadow-sm shadow-accent/10 " +
    "active:shadow-none active:translate-y-px",
  secondary:
    "bg-bg-muted text-text border border-border hover:border-text-tertiary hover:bg-bg-emphasis",
  ghost:
    "text-text-secondary hover:text-text hover:bg-bg-muted",
  danger:
    "bg-error/8 text-error hover:bg-error/15 border border-error/15",
} as const;

const buttonSizes = {
  sm: "h-8 px-3 text-[13px] rounded-lg gap-1.5",
  md: "h-9 px-4 text-sm rounded-lg gap-2",
  lg: "h-10 px-5 text-sm rounded-lg gap-2",
} as const;

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: keyof typeof buttonVariants;
  size?: keyof typeof buttonSizes;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "primary", size = "md", ...props }, ref) => (
    <button
      ref={ref}
      className={cn(
        "inline-flex items-center justify-center font-medium",
        "transition-all duration-150 ease-out",
        "disabled:opacity-40 disabled:pointer-events-none",
        buttonVariants[variant],
        buttonSizes[size],
        className,
      )}
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
            "w-full h-10 px-3 rounded-lg text-sm bg-bg border transition-all duration-150",
            "placeholder:text-text-tertiary",
            "focus:outline-none focus:ring-1 focus:ring-accent/50 focus:border-accent/30",
            error ? "border-error/40" : "border-border hover:border-text-tertiary/50",
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
      className={cn(
        "rounded-xl border border-border bg-bg-subtle/80 backdrop-blur-sm",
        "transition-colors duration-200",
        className,
      )}
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
  return (
    <div className={cn("px-5 py-4", className)} {...props}>
      {children}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Badge
// ---------------------------------------------------------------------------

const badgeVariants = {
  default: "bg-bg-emphasis text-text-secondary border border-border",
  success: "bg-success/8 text-success border border-success/15",
  warning: "bg-warning/8 text-warning border border-warning/15",
  error: "bg-error/8 text-error border border-error/15",
  accent: "bg-accent-subtle text-accent border border-accent/15",
} as const;

interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: keyof typeof badgeVariants;
}

export function Badge({ className, variant = "default", ...props }: BadgeProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center px-2 py-0.5 rounded-md text-[11px] font-medium tracking-wide",
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
    <kbd className="ml-auto text-[10px] text-text-tertiary bg-bg-emphasis/50 px-1.5 py-0.5 rounded font-mono border border-border-subtle">
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
    <div className="flex flex-col items-center justify-center py-20 px-4 text-center animate-fade-in">
      {icon && (
        <div className="mb-4 p-3 rounded-2xl bg-bg-muted border border-border text-text-tertiary">
          {icon}
        </div>
      )}
      <h3 className="text-sm font-medium text-text-secondary">{title}</h3>
      {description && <p className="mt-1.5 text-sm text-text-tertiary max-w-sm">{description}</p>}
      {action && <div className="mt-5">{action}</div>}
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
    <div className="relative group rounded-xl border border-border bg-bg overflow-hidden">
      {language && (
        <div className="px-4 py-2 border-b border-border flex items-center justify-between">
          <span className="text-[11px] text-text-tertiary font-mono uppercase tracking-widest">{language}</span>
          <button
            onClick={copy}
            className="opacity-0 group-hover:opacity-100 transition-opacity text-[11px] text-text-tertiary hover:text-text px-2 py-0.5 rounded-md hover:bg-bg-emphasis"
            aria-label="Copy code"
          >
            Copy
          </button>
        </div>
      )}
      <pre className="p-4 text-[13px] font-mono text-text-secondary leading-relaxed overflow-x-auto">
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
      "rounded-xl border bg-bg-subtle/80 px-5 py-4 transition-colors duration-200",
      accent ? "border-accent/20 bg-accent-subtle" : "border-border hover:border-border-accent",
    )}>
      <p className="text-[11px] font-medium text-text-tertiary uppercase tracking-widest">{label}</p>
      <p className={cn(
        "mt-1.5 text-2xl font-bold tabular-nums tracking-tight",
        accent && "text-accent",
      )}>
        {value}
      </p>
      {hint && <p className="mt-1 text-[11px] text-text-tertiary">{hint}</p>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// StatusDot
// ---------------------------------------------------------------------------

export function StatusDot({ status }: { status: "active" | "idle" | "error" }) {
  const colors = {
    active: "bg-success shadow-sm shadow-success/50",
    idle: "bg-text-tertiary",
    error: "bg-error shadow-sm shadow-error/50",
  };
  return (
    <span className={cn("inline-block w-2 h-2 rounded-full", colors[status])} />
  );
}
