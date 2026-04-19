import { useQuery } from "@tanstack/react-query";
import { useApi } from "../lib/auth";
import { Metric, Card, CardHeader, CardBody, Badge, EmptyState, CodeBlock } from "../components/ui";
import { Activity, Zap } from "lucide-react";

export function DashboardPage() {
  const api = useApi();
  const stats = useQuery({ queryKey: ["stats"], queryFn: () => api.stats(), refetchInterval: 3000 });
  const audit = useQuery({ queryKey: ["audit"], queryFn: () => api.audit.recent(10) });

  const s = stats.data;
  const rows = audit.data?.rows ?? [];

  return (
    <div className="space-y-8 animate-fade-in">
      <div>
        <h1 className="text-xl font-semibold tracking-tight">Dashboard</h1>
        <p className="text-sm text-text-tertiary mt-1">Live state of your control plane.</p>
      </div>

      {/* Metrics */}
      <div className="grid grid-cols-2 lg:grid-cols-5 gap-3">
        <Metric
          label="Active Execs"
          value={s?.active_execs ?? 0}
          hint={s?.active_execs ? "jails running" : "idle"}
          accent={!!s?.active_execs}
        />
        <Metric label="Total Execs" value={s?.total_execs ?? 0} />
        <Metric label="Sessions" value={s?.sessions ?? 0} />
        <Metric label="Credentials" value={s?.credentials ?? 0} />
        <Metric
          label="Proxy Reqs"
          value={audit.data?.total ?? 0}
          hint={rows.filter((r) => r.reject_reason).length > 0 ? "has rejections" : undefined}
        />
      </div>

      {/* Quick Start */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Zap size={14} className="text-accent" />
              <h2 className="text-sm font-medium">Quick Start</h2>
            </div>
          </div>
        </CardHeader>
        <CardBody className="space-y-3">
          <CodeBlock language="bash">{`bun add @agentjail/sdk`}</CodeBlock>
          <CodeBlock language="typescript">{`import { Agentjail } from "@agentjail/sdk";

const aj = new Agentjail({
  baseUrl: "${window.location.origin.replace("3000", "7000")}",
  apiKey: process.env.AGENTJAIL_API_KEY,
});

// Attach a real key (never enters any sandbox)
await aj.credentials.put({ service: "openai", secret: "sk-..." });

// Create session → get phantom tokens
const session = await aj.sessions.create({ services: ["openai"] });

// Execute code in a real Linux jail
const result = await aj.sessions.exec(session.id, {
  cmd: "/bin/sh",
  args: ["-c", "echo $OPENAI_API_KEY"],
});
// stdout: "phm_..." (phantom token, not the real key)`}</CodeBlock>
        </CardBody>
      </Card>

      {/* Recent traffic */}
      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <Activity size={14} className="text-text-tertiary" />
            <h2 className="text-sm font-medium">Recent Proxy Traffic</h2>
          </div>
        </CardHeader>
        <CardBody className="p-0">
          {rows.length === 0 ? (
            <EmptyState
              title="No proxy traffic yet"
              description="Once a sandbox hits the phantom proxy, you'll see each request here."
            />
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-border text-left text-text-tertiary text-xs">
                    <th className="px-5 py-2 font-medium">Service</th>
                    <th className="px-5 py-2 font-medium">Method</th>
                    <th className="px-5 py-2 font-medium">Path</th>
                    <th className="px-5 py-2 font-medium">Status</th>
                    <th className="px-5 py-2 font-medium">Latency</th>
                    <th className="px-5 py-2 font-medium">Time</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((row) => (
                    <tr key={row.id} className="border-b border-border-subtle hover:bg-bg-muted transition-colors">
                      <td className="px-5 py-2.5">
                        <Badge variant={row.reject_reason ? "error" : "default"}>{row.service}</Badge>
                      </td>
                      <td className="px-5 py-2.5 font-mono text-xs text-text-secondary">{row.method}</td>
                      <td className="px-5 py-2.5 font-mono text-xs text-text-tertiary truncate max-w-48">{row.path}</td>
                      <td className="px-5 py-2.5">
                        <Badge variant={row.status < 300 ? "success" : row.status < 500 ? "warning" : "error"}>
                          {row.status}
                        </Badge>
                      </td>
                      <td className="px-5 py-2.5 tabular-nums text-text-tertiary">
                        {row.upstream_ms != null ? `${row.upstream_ms}ms` : "—"}
                      </td>
                      <td className="px-5 py-2.5 text-text-tertiary text-xs">
                        {new Date(row.at).toLocaleTimeString()}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </CardBody>
      </Card>
    </div>
  );
}
