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
      end={to === "/"}
      className={({ isActive }) =>
        cn(
          "flex items-center gap-3 px-3 py-2 rounded-lg text-[13px] transition-all duration-100",
          isActive
            ? "bg-accent-subtle text-accent font-medium border border-accent/10"
            : "text-text-secondary hover:text-text hover:bg-bg-muted",
        )
      }
    >
      <Icon size={15} strokeWidth={1.75} />
      <span>{label}</span>
      <Kbd>{kbd}</Kbd>
    </NavLink>
  );
}

export function Layout() {
  const { auth, logout } = useAuth();

  return (
    <div className="flex h-dvh noise">
      {/* Sidebar */}
      <aside className="w-56 flex-shrink-0 border-r border-border flex flex-col bg-bg-subtle/50 backdrop-blur-sm">
        {/* Brand */}
        <div className="px-4 py-5">
          <div className="flex items-center gap-2.5">
            <div className="w-7 h-7 rounded-lg bg-gradient-to-br from-accent to-accent-dim flex items-center justify-center shadow-sm shadow-accent/20">
              <Shield size={13} className="text-text-inverse" />
            </div>
            <span className="font-bold text-[15px] tracking-tight">agentjail</span>
          </div>
          <p className="mt-2 text-[11px] text-text-tertiary truncate pl-[38px]">
            {auth?.baseUrl.replace(/^https?:\/\//, "")}
          </p>
        </div>

        {/* Divider */}
        <div className="mx-4 h-px bg-gradient-to-r from-transparent via-border to-transparent" />

        {/* Nav */}
        <nav className="flex-1 px-3 py-4 space-y-0.5">
          {nav.map((item) => (
            <SidebarLink key={item.to} {...item} />
          ))}
        </nav>

        {/* Footer */}
        <div className="px-3 py-3">
          <div className="mx-1 mb-2 h-px bg-gradient-to-r from-transparent via-border to-transparent" />
          <button
            onClick={logout}
            className="flex items-center gap-2.5 w-full px-3 py-2 rounded-lg text-[13px] text-text-tertiary hover:text-error hover:bg-error/5 transition-all duration-150"
          >
            <LogOut size={14} />
            <span>Disconnect</span>
          </button>
        </div>
      </aside>

      {/* Main */}
      <main className="flex-1 overflow-y-auto bg-bg">
        <div className="max-w-5xl mx-auto px-8 py-8">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
