"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { cn } from "@/lib/cn";

interface NavItem {
  href: string;
  label: string;
  kbd: string;
}

const NAV: NavItem[] = [
  { href: "/", label: "Dashboard", kbd: "g d" },
  { href: "/sessions", label: "Sessions", kbd: "g s" },
  { href: "/credentials", label: "Credentials", kbd: "g c" },
  { href: "/audit", label: "Audit", kbd: "g a" },
];

/** Primary navigation on the left. */
export function Sidebar() {
  const active = usePathname() ?? "/";
  return (
    <aside className="flex w-52 shrink-0 flex-col gap-1 border-r border-border px-3 py-4">
      <div className="mb-4 flex items-center gap-2 px-2">
        <span
          aria-hidden
          className="inline-block h-2 w-2 rounded-full bg-accent"
        />
        <span className="font-mono text-sm font-semibold tracking-tight">
          agentjail
        </span>
      </div>
      <nav className="flex flex-col gap-0.5">
        {NAV.map((item) => {
          const isActive =
            item.href === "/" ? active === "/" : active.startsWith(item.href);
          return (
            <Link
              key={item.href}
              href={item.href}
              className={cn(
                "flex items-center justify-between rounded-md px-2 py-1.5 " +
                  "text-sm transition-colors",
                isActive
                  ? "bg-fg/[0.06] text-fg"
                  : "text-muted hover:bg-fg/[0.03] hover:text-fg",
              )}
            >
              <span>{item.label}</span>
              <span className="font-mono text-[10px] text-muted/60">
                {item.kbd}
              </span>
            </Link>
          );
        })}
      </nav>
      <div className="mt-auto px-2 text-[10px] text-muted/60">
        phantom-token sandbox · open source
      </div>
    </aside>
  );
}
