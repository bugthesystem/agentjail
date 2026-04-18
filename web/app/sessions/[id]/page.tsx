import { notFound } from "next/navigation";
import { api } from "@/lib/api";
import { Badge } from "@/components/Badge";
import { Card, CardBody, CardHeader, CardTitle } from "@/components/Card";
import { KeyValue } from "@/components/KeyValue";
import { PageHeader } from "@/components/PageHeader";
import { RelativeTime } from "@/components/RelativeTime";
import { SessionEnv } from "@/components/SessionEnv";

export default async function SessionDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  let session;
  try {
    session = await api.sessions.get(id);
  } catch {
    notFound();
  }
  return (
    <div className="mx-auto max-w-4xl">
      <PageHeader title={session.id} subtitle="Session detail" />
      <Card>
        <CardHeader>
          <CardTitle>Overview</CardTitle>
        </CardHeader>
        <CardBody className="grid grid-cols-3 gap-6">
          <KeyValue label="Services">
            <div className="flex gap-1">
              {session.services.map((s) => (
                <Badge key={s} tone="muted">
                  {s}
                </Badge>
              ))}
            </div>
          </KeyValue>
          <KeyValue label="Created">
            <RelativeTime iso={session.created_at} />
          </KeyValue>
          <KeyValue label="Expires">
            {session.expires_at ? (
              <RelativeTime iso={session.expires_at} />
            ) : (
              "no expiry"
            )}
          </KeyValue>
        </CardBody>
      </Card>
      <Card className="mt-6">
        <CardHeader>
          <CardTitle>Env</CardTitle>
        </CardHeader>
        <CardBody>
          <SessionEnv env={session.env} />
        </CardBody>
      </Card>
    </div>
  );
}
