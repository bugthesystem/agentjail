import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useApi } from "../lib/auth";
import { Panel, PanelHeader } from "../components/Panel";
import { Empty } from "../components/Empty";
import { SessionRow } from "../components/sessions/SessionRow";
import { MintForm } from "../components/sessions/MintForm";

/** Sessions page = list + mint form. */
export function Sessions() {
  const api = useApi();
  const qc = useQueryClient();
  const [openId, setOpenId] = useState<string | null>(null);

  const { data: sessions } = useQuery({
    queryKey: ["sessions"],
    queryFn: () => api.sessions.list(),
    refetchInterval: 4000,
  });
  const { data: creds } = useQuery({
    queryKey: ["credentials"],
    queryFn: () => api.credentials.list(),
  });

  const available = new Set((creds ?? []).map((c) => c.service));

  const close = useMutation({
    mutationFn: (id: string) => api.sessions.close(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["sessions"] }),
  });

  return (
    <div className="grid grid-cols-[1fr_380px] gap-5">
      <Panel padded={false}>
        <div className="px-5 py-4 flex items-center justify-between">
          <PanelHeader
            eyebrow="Sessions"
            title={`${sessions?.length ?? 0} active`}
            className="!mb-0"
          />
          <div className="text-[11px] mono text-ink-500">auto-refreshing</div>
        </div>
        <div className="hairline" />
        {(sessions?.length ?? 0) === 0 ? (
          <Empty
            title="No sessions yet"
            hint="Mint one on the right — you'll get a set of phantom env vars ready to drop into a sandbox."
          />
        ) : (
          (sessions ?? []).map((s) => (
            <SessionRow
              key={s.id}
              session={s}
              open={openId === s.id}
              onToggle={() => setOpenId(openId === s.id ? null : s.id)}
              onClose={() => close.mutate(s.id)}
              closing={close.isPending && close.variables === s.id}
            />
          ))
        )}
      </Panel>

      <MintForm available={available} />
    </div>
  );
}
