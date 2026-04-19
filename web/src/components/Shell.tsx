import { NavLink, Outlet } from "react-router-dom";
import { useAuth } from "../lib/auth";
import { Logo } from "./Logo";
import { cn } from "../lib/cn";

const NAV = [
  { to: "/",            label: "Overview"    },
  { to: "/jails",       label: "Jails"       },
  { to: "/sessions",    label: "Sessions"    },
  { to: "/credentials", label: "Credentials" },
  { to: "/stream",      label: "Stream"      },
  { to: "/playground",  label: "Playground"  },
] as const;

export function Shell() {
  const { auth, logout } = useAuth();
  const host = auth ? new URL(auth.baseUrl).host : "";

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
          {NAV.map((n) => (
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
