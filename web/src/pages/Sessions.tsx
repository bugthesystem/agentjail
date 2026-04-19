import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../lib/auth";
import { Card, CardHeader, CardBody, Badge, Button, EmptyState, CodeBlock } from "../components/ui";
import { Shield, Trash2, Clock, Copy } from "lucide-react";
import type { Session } from "../lib/api";

function SessionRow({ session, onClose }: { session: Session; onClose: () => void }) {
  const expired = session.expires_at && new Date(session.expires_at) < new Date();
  const copyEnv = () => {
    const envStr = Object.entries(session.env)
      .map(([k, v]) => `${k}=${v}`)
      .join("\n");
    navigator.clipboard.writeText(envStr);
  };

  return (
    <div className="px-5 py-4 border-b border-border-subtle hover:bg-bg-muted/50 transition-colors animate-fade-in">
      <div className="flex items-start justify-between">
        <div className="space-y-1.5">
          <div className="flex items-center gap-2">
            <code className="text-sm font-mono text-text">{session.id}</code>
            {expired && <Badge variant="error">expired</Badge>}
          </div>
          <div className="flex items-center gap-3 text-xs text-text-tertiary">
            <span className="flex items-center gap-1">
              <Clock size={12} />
              {new Date(session.created_at).toLocaleString()}
            </span>
            {session.services.map((s) => (
              <Badge key={s} variant="accent">{s}</Badge>
            ))}
          </div>
        </div>
        <div className="flex items-center gap-1.5">
          <Button variant="ghost" size="sm" onClick={copyEnv} aria-label="Copy env vars">
            <Copy size={14} /> Env
          </Button>
          <Button variant="danger" size="sm" onClick={onClose} aria-label="Close session">
            <Trash2 size={14} />
          </Button>
        </div>
      </div>
      {/* Env preview */}
      <div className="mt-3 grid grid-cols-2 gap-x-4 gap-y-1 text-xs font-mono">
        {Object.entries(session.env).slice(0, 4).map(([k, v]) => (
          <div key={k} className="flex gap-2 truncate">
            <span className="text-text-tertiary">{k}=</span>
            <span className="text-text-secondary truncate">{v.slice(0, 24)}…</span>
          </div>
        ))}
      </div>
    </div>
  );
}

export function SessionsPage() {
  const api = useApi();
  const qc = useQueryClient();
  const sessions = useQuery({ queryKey: ["sessions"], queryFn: () => api.sessions.list(), refetchInterval: 5000 });
  const closeMut = useMutation({
    mutationFn: (id: string) => api.sessions.close(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sessions"] }),
  });

  const list = sessions.data ?? [];

  return (
    <div className="space-y-6 animate-fade-in">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Sessions</h1>
          <p className="text-sm text-text-tertiary mt-1">
            Each session bundles phantom tokens for one sandbox.
          </p>
        </div>
      </div>

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Shield size={14} className="text-text-tertiary" />
              <h2 className="text-sm font-medium">All Sessions</h2>
              <Badge>{list.length}</Badge>
            </div>
          </div>
        </CardHeader>
        <CardBody className="p-0">
          {list.length === 0 ? (
            <EmptyState
              icon={<Shield size={24} />}
              title="No active sessions"
              description="Create one from the SDK to get phantom tokens."
              action={
                <CodeBlock language="typescript">{`await aj.sessions.create({ services: ["openai"] })`}</CodeBlock>
              }
            />
          ) : (
            list.map((s) => (
              <SessionRow key={s.id} session={s} onClose={() => closeMut.mutate(s.id)} />
            ))
          )}
        </CardBody>
      </Card>
    </div>
  );
}
