/** Composable UI primitives. No boolean prop soup. */

import { forwardRef, type ButtonHTMLAttributes, type InputHTMLAttributes, type HTMLAttributes } from "react";
import { cn } from "../lib/cn";

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

const buttonVariants = {
  primary: "bg-accent text-text-inverse hover:bg-accent-hover font-medium",
  secondary: "bg-bg-muted text-text border border-border hover:bg-bg-emphasis",
  ghost: "text-text-secondary hover:text-text hover:bg-bg-muted",
  danger: "bg-error/10 text-error hover:bg-error/20 border border-error/20",
} as const;

const buttonSizes = {
  sm: "h-8 px-3 text-sm rounded-md gap-1.5",
  md: "h-9 px-4 text-sm rounded-lg gap-2",
  lg: "h-10 px-5 text-base rounded-lg gap-2",
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
        "inline-flex items-center justify-center font-medium transition-colors duration-150",
        "disabled:opacity-50 disabled:pointer-events-none",
        "focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-bg",
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
      <div className="space-y-1.5">
        {label && (
          <label htmlFor={inputId} className="text-sm font-medium text-text-secondary">
            {label}
          </label>
        )}
        <input
          ref={ref}
          id={inputId}
          className={cn(
            "w-full h-9 px-3 rounded-lg text-sm bg-bg-subtle border transition-colors duration-150",
            "placeholder:text-text-tertiary",
            "focus:outline-none focus:ring-2 focus:ring-accent focus:ring-offset-1 focus:ring-offset-bg",
            error ? "border-error" : "border-border hover:border-text-tertiary",
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
        "rounded-xl border border-border bg-bg-subtle",
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
    <div className={cn("px-5 py-4 border-b border-border", className)} {...props}>
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
  default: "bg-bg-emphasis text-text-secondary",
  success: "bg-success/10 text-success",
  warning: "bg-warning/10 text-warning",
  error: "bg-error/10 text-error",
  accent: "bg-accent/10 text-accent",
} as const;

interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: keyof typeof badgeVariants;
}

export function Badge({ className, variant = "default", ...props }: BadgeProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center px-2 py-0.5 rounded-md text-xs font-medium",
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
    <kbd className="ml-auto text-2xs text-text-tertiary bg-bg-emphasis px-1.5 py-0.5 rounded font-mono border border-border-subtle">
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
      {icon && <div className="mb-4 text-text-tertiary">{icon}</div>}
      <h3 className="text-sm font-medium text-text">{title}</h3>
      {description && <p className="mt-1 text-sm text-text-tertiary max-w-sm">{description}</p>}
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Code Block
// ---------------------------------------------------------------------------

interface CodeBlockProps {
  children: string;
  language?: string;
  copyable?: boolean;
}

export function CodeBlock({ children, language, copyable = true }: CodeBlockProps) {
  const copy = () => navigator.clipboard.writeText(children);
  return (
    <div className="relative group rounded-lg border border-border bg-bg overflow-hidden">
      {language && (
        <div className="px-3 py-1.5 border-b border-border text-2xs text-text-tertiary font-mono uppercase tracking-wider">
          {language}
        </div>
      )}
      <pre className="p-3 text-sm font-mono text-text-secondary overflow-x-auto">
        <code>{children}</code>
      </pre>
      {copyable && (
        <button
          onClick={copy}
          className="absolute top-2 right-2 opacity-0 group-hover:opacity-100 transition-opacity p-1.5 rounded-md bg-bg-emphasis hover:bg-bg-muted text-text-tertiary hover:text-text"
          aria-label="Copy code"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <rect x="9" y="9" width="13" height="13" rx="2" />
            <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" />
          </svg>
        </button>
      )}
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
  trend?: "up" | "down" | "neutral";
}

export function Metric({ label, value, hint }: MetricProps) {
  return (
    <Card className="px-5 py-4">
      <p className="text-xs font-medium text-text-tertiary uppercase tracking-wider">{label}</p>
      <p className="mt-1 text-2xl font-semibold tabular-nums">{value}</p>
      {hint && <p className="mt-0.5 text-xs text-text-tertiary">{hint}</p>}
    </Card>
  );
}

// ---------------------------------------------------------------------------
// StatusDot
// ---------------------------------------------------------------------------

export function StatusDot({ status }: { status: "active" | "idle" | "error" }) {
  const colors = {
    active: "bg-success",
    idle: "bg-text-tertiary",
    error: "bg-error",
  };
  return (
    <span className={cn("inline-block w-2 h-2 rounded-full", colors[status])} />
  );
}
