import { cn } from "../lib/cn";

export function Logo({ size = 28, className }: { size?: number; className?: string }) {
  return (
    <svg
      viewBox="0 0 64 64"
      width={size}
      height={size}
      className={cn("shrink-0", className)}
      aria-label="agentjail"
    >
      <defs>
        <linearGradient id="logo-shield" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="var(--color-phantom)" />
          <stop offset="1" stopColor="var(--color-phantom-dim)" />
        </linearGradient>
      </defs>
      <path
        d="M32 10 L50 20 V38 Q50 52 32 56 Q14 52 14 38 V20 Z"
        fill="none"
        stroke="url(#logo-shield)"
        strokeWidth="2.2"
        strokeLinejoin="round"
      />
      <circle cx="32" cy="32" r="3.5" fill="var(--color-phantom)" />
      <circle cx="32" cy="32" r="8" fill="none" stroke="var(--color-phantom)" strokeWidth="0.8" opacity="0.55" />
    </svg>
  );
}
