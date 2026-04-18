import { api } from "@/lib/api";
import { MetricTile } from "@/components/MetricTile";
import { PageHeader } from "@/components/PageHeader";
import { PhantomAuditTable } from "@/components/PhantomAuditTable";
import { Card, CardBody, CardHeader, CardTitle } from "@/components/Card";

export default async function DashboardPage() {
  const [sessions, credentials, audit] = await Promise.all([
    api.sessions.list().catch(() => []),
    api.credentials.list().catch(() => []),
    api.audit.recent(20).catch(() => ({ rows: [], total: 0 })),
  ]);

  const rejectedCount = audit.rows.filter((r) => r.reject_reason).length;

  return (
    <div className="mx-auto max-w-5xl">
      <PageHeader
        title="Dashboard"
        subtitle="Live state of the control plane."
      />
      <div className="grid grid-cols-4 gap-3">
        <MetricTile label="Sessions" value={sessions.length} />
        <MetricTile label="Credentials" value={credentials.length} />
        <MetricTile label="Proxy requests" value={audit.total} />
        <MetricTile
          label="Rejected (recent)"
          value={rejectedCount}
          hint={rejectedCount > 0 ? "inspect on /audit" : "none"}
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
