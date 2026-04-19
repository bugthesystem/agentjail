import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "../lib/auth";
import { Panel, PanelHeader } from "../components/Panel";
import { Button } from "../components/Button";
import { Empty } from "../components/Empty";
import type { SnapshotRecord } from "../lib/api";

export function Snapshots() {
  const api = useApi();
  const qc = useQueryClient();

  const { data } = useQuery({
    queryKey: ["snapshots"],
    queryFn:  () => api.snapshots.list({ limit: 100 }),
    refetchInterval: 5000,
  });

  const del = useMutation({
    mutationFn: (id: string) => api.snapshots.delete(id),
    onSuccess:  () => qc.invalidateQueries({ queryKey: ["snapshots"] }),
  });

  const restore = useMutation({
    mutationFn: (id: string) => api.snapshots.restoreToNew(id),
    onSuccess:  () => qc.invalidateQueries({ queryKey: ["workspaces"] }),
  });

  return (
    <Panel padded={false}>
      <div className="px-5 py-4 flex items-center justify-between">
        <PanelHeader
          eyebrow="Snapshots"
          title={`${data?.total ?? 0} captured`}
          className="!mb-0"
        />
        <div className="text-[11px] mono text-ink-500">auto-refreshing</div>
      </div>
      <div className="hairline" />
      {(data?.rows.length ?? 0) === 0 ? (
        <Empty
          title="No snapshots yet"
          hint="Create a workspace and snapshot it to bookmark a point-in-time state."
        />
      ) : (
        data!.rows.map((s) => (
          <SnapshotRow
            key={s.id}
            snap={s}
            onDelete={() => del.mutate(s.id)}
            deleting={del.isPending && del.variables === s.id}
            onRestore={() => restore.mutate(s.id)}
            restoring={restore.isPending && restore.variables === s.id}
          />
        ))
      )}
    </Panel>
  );
}

function SnapshotRow({
  snap,
  onDelete,
  deleting,
  onRestore,
  restoring,
}: {
  snap: SnapshotRecord;
  onDelete: () => void;
  deleting: boolean;
  onRestore: () => void;
  restoring: boolean;
}) {
  const mb = (snap.size_bytes / (1024 * 1024)).toFixed(2);
  const auto = snap.name?.startsWith("auto:");
  return (
    <div className="px-5 py-4 grid grid-cols-[1fr_auto] gap-4 items-center hairline-after">
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <span className="mono text-[13px] text-ink-100 truncate">{snap.id}</span>
          {snap.name && !auto && (
            <span className="text-[11px] text-ink-400">· {snap.name}</span>
          )}
          {auto && (
            <span className="text-[10px] uppercase tracking-[0.18em] px-1.5 py-0.5 rounded bg-ink-850 text-[var(--color-iris)] ring-1 ring-[var(--color-iris)]/30">
              auto
            </span>
          )}
        </div>
        <div className="text-[11.5px] text-ink-500 mt-1 flex gap-3 flex-wrap">
          <span>created {new Date(snap.created_at).toLocaleString()}</span>
          <span>{mb} MB</span>
          {snap.workspace_id && (
            <span className="mono">from {snap.workspace_id}</span>
          )}
        </div>
      </div>
      <div className="flex items-center gap-2">
        <Button variant="outline" size="sm" onClick={onRestore} disabled={restoring}>
          {restoring ? "restoring…" : "restore"}
        </Button>
        <Button variant="outline" size="sm" onClick={onDelete} disabled={deleting}>
          {deleting ? "deleting…" : "delete"}
        </Button>
      </div>
    </div>
  );
}
