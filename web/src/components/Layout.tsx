import { NavLink, Outlet } from "react-router-dom";
import { cn } from "../lib/cn";
import { useAuth } from "../lib/auth";
import { Kbd } from "./ui";
import {
  LayoutDashboard,
  Key,
  Shield,
  ScrollText,
  Terminal,
  LogOut,
  Zap,
} from "lucide-react";

const nav = [
  { to: "/", icon: LayoutDashboard, label: "Dashboard", kbd: "g d" },
  { to: "/sessions", icon: Shield, label: "Sessions", kbd: "g s" },
  { to: "/credentials", icon: Key, label: "Credentials", kbd: "g c" },
  { to: "/runs", icon: Terminal, label: "Runs", kbd: "g r" },
  { to: "/audit", icon: ScrollText, label: "Audit", kbd: "g a" },
] as const;

function SidebarLink({ to, icon: Icon, label, kbd }: (typeof nav)[number]) {
  return (
    <NavLink
      to={to}
      className={({ isActive }) =>
        cn(
          "flex items-center gap-3 px-3 py-2 rounded-lg text-sm transition-colors duration-100",
          isActive
            ? "bg-bg-emphasis text-text font-medium"
            : "text-text-secondary hover:text-text hover:bg-bg-muted",
        )
      }
    >
      <Icon size={16} strokeWidth={1.75} />
      <span>{label}</span>
      <Kbd>{kbd}</Kbd>
    </NavLink>
  );
}

export function Layout() {
  const { auth, logout } = useAuth();

  return (
    <div className="flex h-dvh">
      {/* Sidebar */}
      <aside className="w-56 flex-shrink-0 border-r border-border flex flex-col bg-bg-subtle">
        {/* Brand */}
        <div className="px-4 py-5 border-b border-border">
          <div className="flex items-center gap-2">
            <div className="w-6 h-6 rounded-md bg-accent flex items-center justify-center">
              <Zap size={14} className="text-text-inverse" />
            </div>
            <span className="font-semibold text-sm tracking-tight">agentjail</span>
          </div>
          <p className="mt-1 text-2xs text-text-tertiary truncate">
            {auth?.baseUrl.replace(/^https?:\/\//, "")}
          </p>
        </div>

        {/* Nav */}
        <nav className="flex-1 px-3 py-3 space-y-0.5">
          {nav.map((item) => (
            <SidebarLink key={item.to} {...item} />
          ))}
        </nav>

        {/* Footer */}
        <div className="px-3 py-3 border-t border-border">
          <button
            onClick={logout}
            className="flex items-center gap-2 w-full px-3 py-2 rounded-lg text-sm text-text-tertiary hover:text-text hover:bg-bg-muted transition-colors"
          >
            <LogOut size={14} />
            <span>Disconnect</span>
          </button>
          <p className="mt-2 px-3 text-2xs text-text-tertiary">
            secure sandbox platform
          </p>
        </div>
      </aside>

      {/* Main */}
      <main className="flex-1 overflow-y-auto">
        <div className="max-w-5xl mx-auto px-8 py-8">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
