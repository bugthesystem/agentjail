import type { AuditRow } from "@/lib/api";
import { Badge } from "./Badge";
import { EmptyState } from "./EmptyState";
import { RelativeTime } from "./RelativeTime";
import { StatusDot } from "./StatusDot";
import { Table, Tbody, Td, Th, Thead, Tr } from "./Table";

/** Audit log table. Pure presentation — takes the rows already fetched. */
export function PhantomAuditTable({ rows }: { rows: AuditRow[] }) {
  if (rows.length === 0) {
    return (
      <EmptyState
        title="No proxy traffic yet"
        hint="Once a sandbox hits the phantom proxy, you'll see each request here."
      />
    );
  }
  return (
    <Table>
      <Thead>
        <Tr>
          <Th>When</Th>
          <Th>Status</Th>
          <Th>Service</Th>
          <Th>Method</Th>
          <Th>Path</Th>
          <Th>Session</Th>
          <Th className="text-right">Upstream</Th>
        </Tr>
      </Thead>
      <Tbody>
        {rows.map((r) => (
          <Tr key={r.id}>
            <Td className="text-muted">
              <RelativeTime iso={r.at} />
            </Td>
            <Td>
              <span className="flex items-center gap-2">
                <StatusDot status={r.status} />
                <span>{r.status}</span>
                {r.reject_reason && (
                  <Badge tone="danger">{r.reject_reason}</Badge>
                )}
              </span>
            </Td>
            <Td>
              {r.service ? (
                <Badge tone="muted">{r.service}</Badge>
              ) : (
                <span className="text-muted">-</span>
              )}
            </Td>
            <Td>{r.method}</Td>
            <Td className="max-w-[22rem] truncate">{r.path}</Td>
            <Td className="text-muted">
              {r.session_id ? r.session_id.slice(0, 14) : "—"}
            </Td>
            <Td className="text-right tabular-nums text-muted">
              {r.upstream_ms !== null ? `${r.upstream_ms}ms` : "—"}
            </Td>
          </Tr>
        ))}
      </Tbody>
    </Table>
  );
}
