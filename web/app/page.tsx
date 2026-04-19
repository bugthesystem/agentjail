import { api } from "@/lib/api";
import { MetricTile } from "@/components/MetricTile";
import { PageHeader } from "@/components/PageHeader";
import { PhantomAuditTable } from "@/components/PhantomAuditTable";
import { Card, CardBody, CardHeader, CardTitle } from "@/components/Card";

export default async function DashboardPage() {
  const [stats, audit] = await Promise.all([
    api.stats().catch(() => ({
      active_execs: 0,
      total_execs: 0,
      sessions: 0,
      credentials: 0,
    })),
    api.audit.recent(20).catch(() => ({ rows: [], total: 0 })),
  ]);

  const rejectedCount = audit.rows.filter((r) => r.reject_reason).length;

  return (
    <div className="mx-auto max-w-5xl">
      <PageHeader
        title="Dashboard"
        subtitle="Live state of the control plane."
      />
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-5">
        <MetricTile
          label="Active execs"
          value={stats.active_execs}
          hint={stats.active_execs > 0 ? "jails running now" : "idle"}
        />
        <MetricTile label="Total execs" value={stats.total_execs} />
        <MetricTile label="Sessions" value={stats.sessions} />
        <MetricTile label="Credentials" value={stats.credentials} />
        <MetricTile
          label="Proxy requests"
          value={audit.total}
          hint={rejectedCount > 0 ? `${rejectedCount} rejected` : undefined}
        />
      </div>
      <Card className="mt-8">
        <CardHeader>
          <CardTitle>Recent proxy traffic</CardTitle>
        </CardHeader>
        <CardBody className="p-0">
          <PhantomAuditTable rows={audit.rows} />
        </CardBody>
      </Card>
    </div>
  );
}
