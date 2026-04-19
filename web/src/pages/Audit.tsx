import { useQuery } from "@tanstack/react-query";
import { useApi } from "../lib/auth";
import { Card, CardHeader, CardBody, Badge, EmptyState } from "../components/ui";
import { ScrollText } from "lucide-react";

export function AuditPage() {
  const api = useApi();
  const audit = useQuery({ queryKey: ["audit-full"], queryFn: () => api.audit.recent(200), refetchInterval: 5000 });

  const rows = audit.data?.rows ?? [];

  return (
    <div className="space-y-6 animate-fade-in">
      <div>
        <h1 className="text-xl font-semibold tracking-tight">Audit Log</h1>
        <p className="text-sm text-text-tertiary mt-1">
          {audit.data?.total ?? 0} total phantom proxy requests.
        </p>
      </div>

      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <ScrollText size={14} className="text-text-tertiary" />
            <h2 className="text-sm font-medium">Recent Requests</h2>
            <Badge>{rows.length}</Badge>
          </div>
        </CardHeader>
        <CardBody className="p-0">
          {rows.length === 0 ? (
            <EmptyState
              icon={<ScrollText size={24} />}
              title="No proxy traffic yet"
              description="Once a sandbox hits the phantom proxy, you'll see each request here."
            />
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-border text-left text-xs text-text-tertiary">
                    <th className="px-5 py-2.5 font-medium">Time</th>
                    <th className="px-5 py-2.5 font-medium">Service</th>
                    <th className="px-5 py-2.5 font-medium">Method</th>
                    <th className="px-5 py-2.5 font-medium">Path</th>
                    <th className="px-5 py-2.5 font-medium">Status</th>
                    <th className="px-5 py-2.5 font-medium">Latency</th>
                    <th className="px-5 py-2.5 font-medium">Session</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((row) => (
                    <tr key={row.id} className="border-b border-border-subtle hover:bg-bg-muted/50 transition-colors">
                      <td className="px-5 py-2.5 text-xs text-text-tertiary whitespace-nowrap tabular-nums">
                        {new Date(row.at).toLocaleTimeString()}
                      </td>
                      <td className="px-5 py-2.5">
                        <Badge variant={row.reject_reason ? "error" : "accent"}>{row.service}</Badge>
                      </td>
                      <td className="px-5 py-2.5 font-mono text-xs">{row.method}</td>
                      <td className="px-5 py-2.5 font-mono text-xs text-text-tertiary truncate max-w-56">{row.path}</td>
                      <td className="px-5 py-2.5">
                        <Badge variant={row.status < 300 ? "success" : row.status < 500 ? "warning" : "error"}>
                          {row.status}
                        </Badge>
                      </td>
                      <td className="px-5 py-2.5 tabular-nums text-text-tertiary text-xs">
                        {row.upstream_ms != null ? `${row.upstream_ms}ms` : "—"}
                      </td>
                      <td className="px-5 py-2.5 font-mono text-2xs text-text-tertiary truncate max-w-24">
                        {row.session_id.slice(0, 12)}…
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
