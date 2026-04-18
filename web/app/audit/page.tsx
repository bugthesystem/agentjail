import { api } from "@/lib/api";
import { Card, CardBody, CardHeader, CardTitle } from "@/components/Card";
import { PageHeader } from "@/components/PageHeader";
import { PhantomAuditTable } from "@/components/PhantomAuditTable";

export default async function AuditPage() {
  const audit = await api.audit
    .recent(200)
    .catch(() => ({ rows: [], total: 0 }));
  return (
    <div className="mx-auto max-w-6xl">
      <PageHeader
        title="Audit"
        subtitle={`${audit.total} total proxy requests recorded.`}
      />
      <Card>
        <CardHeader>
          <CardTitle>Recent requests</CardTitle>
        </CardHeader>
        <CardBody className="p-0">
          <PhantomAuditTable rows={audit.rows} />
        </CardBody>
      </Card>
    </div>
  );
}
