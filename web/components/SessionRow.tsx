import Link from "next/link";
import type { Session } from "@/lib/api";
import { Badge } from "./Badge";
import { Td, Tr } from "./Table";
import { RelativeTime } from "./RelativeTime";

/** Single session row for the sessions list. */
export function SessionRow({ s }: { s: Session }) {
  return (
    <Tr>
      <Td className="max-w-[12rem] truncate">
        <Link className="hover:underline" href={`/sessions/${s.id}`}>
          {s.id}
        </Link>
      </Td>
      <Td>
        <div className="flex flex-wrap gap-1">
          {s.services.map((svc) => (
            <Badge key={svc} tone="muted">
              {svc}
            </Badge>
          ))}
        </div>
      </Td>
      <Td className="text-muted">
        <RelativeTime iso={s.created_at} />
      </Td>
      <Td className="text-muted">
        {s.expires_at ? <RelativeTime iso={s.expires_at} /> : "—"}
      </Td>
    </Tr>
  );
}
