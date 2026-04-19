import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../lib/auth";
import type { ServiceId } from "../lib/api";
import { Panel, PanelHeader } from "../components/Panel";
import { ServiceCard } from "../components/credentials/ServiceCard";
import { AttachForm } from "../components/credentials/AttachForm";
import { SERVICES } from "../lib/format";

/** Credentials page = grid of 4 service cards + attach form. */
export function Credentials() {
  const api = useApi();
  const qc = useQueryClient();

  const { data: creds } = useQuery({
    queryKey: ["credentials"],
    queryFn: () => api.credentials.list(),
  });

  const del = useMutation({
    mutationFn: (svc: ServiceId) => api.credentials.delete(svc),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["credentials"] }),
  });

  const byService = new Map((creds ?? []).map((c) => [c.service, c] as const));
  const existing = new Set(byService.keys());

  return (
    <div className="grid grid-cols-[1fr_420px] gap-4">
      <Panel padded={false}>
        <div className="px-5 py-4">
          <PanelHeader
            eyebrow="Vault"
            title="Upstream credentials"
            action={
              <span className="text-[11px] mono text-ink-500">
                {creds?.length ?? 0} of {SERVICES.length}
              </span>
            }
            className="!mb-0"
          />
        </div>
        <div className="hairline" />
        <div className="p-5 grid grid-cols-2 gap-3">
          {SERVICES.map((svc) => (
            <ServiceCard
              key={svc}
              svc={svc}
              rec={byService.get(svc)}
              onDelete={() => del.mutate(svc)}
              deleting={del.isPending && del.variables === svc}
            />
          ))}
        </div>
      </Panel>

      <AttachForm hasExisting={existing} />
    </div>
  );
}
