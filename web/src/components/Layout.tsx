import { NavLink, Outlet } from "react-router-dom";
import { cn } from "../lib/cn";
import { useAuth } from "../lib/auth";
import { Kbd } from "./ui";
import {
  LayoutDashboard, Key, Shield, ScrollText, Terminal, LogOut, Zap,
} from "lucide-react";

const nav = [
  { to: "/", icon: LayoutDashboard, label: "Dashboard", kbd: "g d" },
  { to: "/sessions", icon: Shield, label: "Sessions", kbd: "g s" },
  { to: "/credentials", icon: Key, label: "Credentials", kbd: "g c" },
  { to: "/runs", icon: Terminal, label: "Runs", kbd: "g r" },
  { to: "/audit", icon: ScrollText, label: "Audit", kbd: "g a" },
] as const;

function NavItem({ to, icon: Icon, label, kbd }: (typeof nav)[number]) {
  return (
    <NavLink
      to={to}
      end={to === "/"}
      className={({ isActive }) =>
        cn(
          "flex items-center gap-2.5 px-2.5 py-[7px] rounded-[var(--radius-lg)] text-[13px] transition-colors duration-100",
          isActive
            ? "bg-bg-emphasis text-text font-medium"
            : "text-text-tertiary hover:text-text-secondary hover:bg-bg-emphasis/40",
        )
      }
    >
      <Icon size={15} strokeWidth={1.5} />
      <span className="flex-1">{label}</span>
      <Kbd>{kbd}</Kbd>
    </NavLink>
  );
}

export function Layout() {
  const { auth, logout } = useAuth();

  return (
    <div className="flex h-dvh">
      {/* Sidebar */}
      <aside className="w-[220px] flex-shrink-0 border-r border-border flex flex-col">
        {/* Brand */}
        <div className="h-14 flex items-center gap-2.5 px-4 border-b border-border">
          <div className="w-6 h-6 rounded-md bg-accent flex items-center justify-center">
            <Zap size={12} className="text-text-inverse" />
          </div>
          <span className="font-semibold text-[14px] tracking-tight">agentjail</span>
        </div>

        {/* Nav */}
        <nav className="flex-1 px-2.5 py-3 space-y-px">
          {nav.map((item) => (
            <NavItem key={item.to} {...item} />
          ))}
        </nav>

        {/* Footer */}
        <div className="px-2.5 py-2.5 border-t border-border">
          <button
            onClick={logout}
            className="flex items-center gap-2.5 w-full px-2.5 py-[7px] rounded-[var(--radius-lg)] text-[13px] text-text-tertiary hover:text-text-secondary hover:bg-bg-emphasis/40 transition-colors"
          >
            <LogOut size={14} strokeWidth={1.5} />
            <span>Disconnect</span>
          </button>
          <p className="mt-1.5 px-2.5 text-[10px] text-text-tertiary/40 tracking-wide">
            {auth?.baseUrl.replace(/^https?:\/\//, "")}
          </p>
        </div>
      </aside>

      {/* Main */}
      <main className="flex-1 overflow-y-auto">
        <div className="max-w-4xl mx-auto px-8 py-8">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
