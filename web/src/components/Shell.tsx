import { NavLink, Outlet, useLocation } from "react-router-dom";
import { useEffect, useRef, useState } from "react";
import { useAuth } from "../lib/auth";
import { Logo } from "./Logo";
import { cn } from "../lib/cn";

const PRIMARY = [
  { to: "/",             label: "Dashboard"    },
  { to: "/projects",     label: "Projects"     },
  { to: "/sessions",     label: "API Sessions" },
  { to: "/integrations", label: "Integrations" },
  { to: "/playground",   label: "Playground"   },
  { to: "/docs",         label: "Docs"         },
] as const;

const OPERATOR = [
  { to: "/operator/ledger",    label: "Execution Ledger" },
  { to: "/operator/snapshots", label: "Snapshots"        },
  { to: "/operator/audit",     label: "API Audit"        },
  { to: "/operator/settings",  label: "System Settings"  },
] as const;

export function Shell() {
  const { auth, logout } = useAuth();
  const host = auth ? new URL(auth.baseUrl).host : "";

  const location = useLocation();
  const [advOpen, setAdvOpen] = useState(false);
  const advRef = useRef<HTMLDivElement | null>(null);
  const advActive = location.pathname.startsWith("/operator/");

  useEffect(() => { setAdvOpen(false); }, [location.pathname]);

  useEffect(() => {
    if (!advOpen) return;
    const onDown = (e: MouseEvent) => {
      if (advRef.current && !advRef.current.contains(e.target as Node)) {
        setAdvOpen(false);
      }
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") setAdvOpen(false); };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [advOpen]);

  return (
    <div className="h-full flex flex-col">
      <header className="flex items-center justify-between px-6 h-14 border-b border-ink-700/80 bg-ink-900/60 backdrop-blur-md relative z-10">
        <div className="flex items-center gap-3">
          <Logo size={24} />
          <div className="flex items-baseline gap-1.5">
            <span className="display text-[15px] font-semibold tracking-tight">agentjail</span>
            <span className="text-[11px] text-ink-400 mono">/ control plane</span>
          </div>
        </div>

        <nav className="flex items-center gap-1">
          {PRIMARY.map((n) => (
            <NavLink
              key={n.to}
              to={n.to}
              end={n.to === "/"}
              className={({ isActive }) =>
                cn(
                  "relative h-9 px-3.5 flex items-center text-[13px] rounded-md transition-colors",
                  isActive
                    ? "text-ink-100"
                    : "text-ink-400 hover:text-ink-200",
                )
              }
            >
              {({ isActive }) => (
                <>
                  <span>{n.label}</span>
                  {isActive && (
                    <span
                      className="absolute left-3 right-3 -bottom-[13px] h-px"
                      style={{
                        background:
                          "linear-gradient(90deg, transparent, var(--color-phantom), transparent)",
                      }}
                    />
                  )}
                </>
              )}
            </NavLink>
          ))}

          <span className="mx-1 w-px h-4 bg-ink-700/80" aria-hidden />

          <div ref={advRef} className="relative">
            <button
              type="button"
              onClick={() => setAdvOpen((o) => !o)}
              aria-haspopup="menu"
              aria-expanded={advOpen}
              className={cn(
                "relative h-9 px-3.5 flex items-center gap-1.5 text-[13px] rounded-md transition-colors",
                advActive || advOpen ? "text-ink-100" : "text-ink-400 hover:text-ink-200",
              )}
            >
              <span>Advanced</span>
              <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden
                className={cn("transition-transform", advOpen && "rotate-180")}>
                <path d="M2 3.5 L5 6.5 L8 3.5" fill="none" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
              {advActive && (
                <span
                  className="absolute left-3 right-3 -bottom-[13px] h-px"
                  style={{
                    background:
                      "linear-gradient(90deg, transparent, var(--color-phantom), transparent)",
                  }}
                />
              )}
            </button>

            {advOpen && (
              <div
                role="menu"
                className="absolute right-0 top-[calc(100%+8px)] min-w-[220px] rounded-md border border-ink-700/80 bg-ink-900/95 backdrop-blur-md shadow-lg py-1.5 z-20"
              >
                <div className="px-3 py-1 text-[10px] uppercase tracking-wider text-ink-500 mono">
                  Operator tools
                </div>
                {OPERATOR.map((n) => (
                  <NavLink
                    key={n.to}
                    to={n.to}
                    role="menuitem"
                    className={({ isActive }) =>
                      cn(
                        "block px-3 py-1.5 text-[13px] transition-colors",
                        isActive
                          ? "text-ink-100 bg-ink-800/60"
                          : "text-ink-300 hover:text-ink-100 hover:bg-ink-800/40",
                      )
                    }
                  >
                    {n.label}
                  </NavLink>
                ))}
              </div>
            )}
          </div>
        </nav>

        <div className="flex items-center gap-3">
          <div className="hidden md:flex items-center gap-2 text-[11px] mono text-ink-400">
            <span className="relative inline-flex w-1.5 h-1.5 rounded-full bg-[var(--color-phantom)] pulse-dot" />
            <span>{host}</span>
          </div>
          <button
            onClick={logout}
            className="text-[11px] mono text-ink-400 hover:text-ink-200 transition-colors"
          >
            disconnect
          </button>
        </div>
      </header>

      <main className="flex-1 overflow-y-auto">
        <div className="max-w-[1680px] mx-auto px-8 py-5 fade-up">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
