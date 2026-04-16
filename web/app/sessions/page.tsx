import { api } from "@/lib/api";
import { Card, CardBody, CardHeader, CardTitle } from "@/components/Card";
import { EmptyState } from "@/components/EmptyState";
import { PageHeader } from "@/components/PageHeader";
import { SessionRow } from "@/components/SessionRow";
import { Table, Tbody, Th, Thead, Tr } from "@/components/Table";

export default async function SessionsPage() {
  const sessions = await api.sessions.list().catch(() => []);
  return (
    <div className="mx-auto max-w-5xl">
      <PageHeader
        title="Sessions"
        subtitle="Each session bundles phantom tokens for one sandbox."
      />
      <Card>
        <CardHeader>
          <CardTitle>All sessions</CardTitle>
        </CardHeader>
        <CardBody className="p-0">
          {sessions.length === 0 ? (
            <EmptyState
              title="No active sessions"
              hint="Create one from the SDK: aj.sessions.create({ services: ['openai'] })"
            />
          ) : (
            <Table>
              <Thead>
                <Tr>
                  <Th>ID</Th>
                  <Th>Services</Th>
                  <Th>Created</Th>
                  <Th>Expires</Th>
                </Tr>
              </Thead>
              <Tbody>
                {sessions.map((s) => (
                  <SessionRow key={s.id} s={s} />
                ))}
              </Tbody>
            </Table>
          )}
        </CardBody>
      </Card>
    </div>
  );
}
