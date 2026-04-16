import type { CredentialRecord } from "@/lib/api";
import { Badge } from "./Badge";
import { Card, CardBody } from "./Card";
import { KeyValue } from "./KeyValue";
import { RelativeTime } from "./RelativeTime";

const SERVICE_LABEL: Record<string, string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
};

/** Single-service credential summary. */
export function CredentialCard({ rec }: { rec: CredentialRecord }) {
  return (
    <Card>
      <CardBody className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          <Badge tone="accent">{SERVICE_LABEL[rec.service] ?? rec.service}</Badge>
          <span className="font-mono text-[12px] text-muted">
            fp {rec.fingerprint.slice(0, 8)}
          </span>
        </div>
        <div className="flex items-center gap-6">
          <KeyValue label="Added">
            <RelativeTime iso={rec.added_at} />
          </KeyValue>
          <KeyValue label="Rotated">
            <RelativeTime iso={rec.updated_at} />
          </KeyValue>
        </div>
      </CardBody>
    </Card>
  );
}
