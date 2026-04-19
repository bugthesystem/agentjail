import type { Session } from "../../lib/api";
import { Button } from "../Button";
import { ServiceStack } from "../ServiceBadge";
import { timeAgo } from "../../lib/format";
import { cn } from "../../lib/cn";

export function SessionRow({
  session,
  open,
  onToggle,
  onClose,
  closing,
}: {
  session: Session;
  open: boolean;
  onToggle: () => void;
  onClose: () => void;
  closing: boolean;
}) {
  return (
    <div className="border-b border-ink-800 last:border-b-0">
      <button
        onClick={onToggle}
        className="w-full grid grid-cols-[auto_1fr_auto_auto] gap-4 items-center px-5 py-3 text-left hover:bg-ink-850/40 transition-colors"
      >
        <span className="w-1.5 h-6 rounded-full bg-[var(--color-phantom)] opacity-80" />
        <div className="min-w-0">
          <div className="mono text-[13px] text-ink-100 truncate">{session.id}</div>
          <div className="text-[11px] text-ink-500 mt-0.5">
            created {timeAgo(session.created_at)}
            {session.expires_at
              ? ` · expires ${timeAgo(session.expires_at)}`
              : " · no expiry"}
          </div>
        </div>
        <ServiceStack services={session.services} size={20} />
        <span
          className={cn("text-ink-500 text-sm transition-transform", open && "rotate-180")}
        >
          ⌄
        </span>
      </button>
      {open && (
        <div className="px-5 pb-5 space-y-3">
          <EnvTable env={session.env} />
          <div className="flex justify-end">
            <Button variant="danger" size="sm" onClick={onClose} disabled={closing}>
              {closing ? "revoking…" : "revoke session"}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

function EnvTable({ env }: { env: Record<string, string> }) {
  return (
    <div className="panel !bg-ink-950/60 p-4 space-y-2">
      {Object.entries(env).map(([k, v]) => (
        <div
          key={k}
          className="grid grid-cols-[180px_1fr_auto] gap-3 items-center text-[12px] mono"
        >
          <span className="text-ink-400">{k}</span>
          <span className="text-ink-200 truncate">{v}</span>
          <button
            className="text-ink-500 hover:text-ink-200 text-[10px] uppercase tracking-widest"
            onClick={() => navigator.clipboard.writeText(v)}
          >
            copy
          </button>
        </div>
      ))}
    </div>
  );
}
