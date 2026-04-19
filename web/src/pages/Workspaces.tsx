import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useApi } from "../lib/auth";
import { Panel, PanelHeader } from "../components/Panel";
import { Button } from "../components/Button";
import { Empty } from "../components/Empty";
import { Field } from "../components/Input";
import type { Workspace, WorkspaceCreateRequest } from "../lib/api";

export function Workspaces() {
  const api = useApi();
  const qc = useQueryClient();

  const { data } = useQuery({
    queryKey: ["workspaces"],
    queryFn:  () => api.workspaces.list({ limit: 100 }),
    refetchInterval: 4000,
  });

  const del = useMutation({
    mutationFn: (id: string) => api.workspaces.delete(id),
    onSuccess:  () => qc.invalidateQueries({ queryKey: ["workspaces"] }),
  });

  return (
    <div className="grid grid-cols-[1fr_420px] gap-4">
      <Panel padded={false}>
        <div className="px-5 py-4 flex items-center justify-between">
          <PanelHeader
            eyebrow="Workspaces"
            title={`${data?.total ?? 0} active`}
            className="!mb-0"
          />
          <div className="text-[11px] mono text-ink-500">auto-refreshing</div>
        </div>
        <div className="hairline" />
        {(data?.rows.length ?? 0) === 0 ? (
          <Empty
            title="No workspaces yet"
            hint="Create one on the right — clone a repo, run multi-step builds, snapshot the result."
          />
        ) : (
          data!.rows.map((w) => (
            <WorkspaceRow
              key={w.id}
              ws={w}
              onDelete={() => del.mutate(w.id)}
              deleting={del.isPending && del.variables === w.id}
            />
          ))
        )}
      </Panel>

      <CreateWorkspaceForm />
    </div>
  );
}

function WorkspaceRow({
  ws,
  onDelete,
  deleting,
}: {
  ws: Workspace;
  onDelete: () => void;
  deleting: boolean;
}) {
  const paused = ws.paused_at !== null;
  return (
    <div className="px-5 py-4 grid grid-cols-[1fr_auto] gap-4 items-center hairline-after">
      <div className="min-w-0">
        <div className="flex items-center gap-2">
          <span className="mono text-[13px] text-ink-100 truncate">{ws.id}</span>
          {ws.label && (
            <span className="text-[11px] text-ink-400">· {ws.label}</span>
          )}
          {paused && (
            <span className="text-[10px] uppercase tracking-[0.18em] px-1.5 py-0.5 rounded bg-ink-850 text-[var(--color-flare)] ring-1 ring-[var(--color-flare)]/30">
              paused
            </span>
          )}
          {ws.config.idle_timeout_secs > 0 && !paused && (
            <span className="text-[10px] uppercase tracking-[0.18em] px-1.5 py-0.5 rounded bg-ink-850 text-ink-400 ring-1 ring-ink-700">
              idle {ws.config.idle_timeout_secs}s
            </span>
          )}
        </div>
        <div className="text-[11.5px] text-ink-500 mt-1 flex gap-3 flex-wrap">
          <span>created {new Date(ws.created_at).toLocaleString()}</span>
          {ws.last_exec_at && (
            <span>last exec {new Date(ws.last_exec_at).toLocaleString()}</span>
          )}
          {ws.git_repo && (
            <span className="mono truncate">{ws.git_repo}</span>
          )}
        </div>
      </div>
      <div className="flex items-center gap-2">
        <Button variant="outline" size="sm" onClick={onDelete} disabled={deleting}>
          {deleting ? "deleting…" : "delete"}
        </Button>
      </div>
    </div>
  );
}

function CreateWorkspaceForm() {
  const api = useApi();
  const qc = useQueryClient();
  const [repo, setRepo] = useState("");
  const [ref, setRef] = useState("");
  const [label, setLabel] = useState("");
  const [idle, setIdle] = useState("");
  const [memory, setMemory] = useState("");

  const create = useMutation({
    mutationFn: (req: WorkspaceCreateRequest) => api.workspaces.create(req),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["workspaces"] });
      setRepo("");
      setRef("");
      setLabel("");
      setIdle("");
      setMemory("");
    },
  });

  function submit(e: React.FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const req: WorkspaceCreateRequest = {};
    if (repo.trim())  req.git = { repo: repo.trim(), ...(ref.trim() ? { ref: ref.trim() } : {}) };
    if (label.trim()) req.label = label.trim();
    const idleN = parseInt(idle, 10);
    if (!Number.isNaN(idleN) && idleN > 0) req.idle_timeout_secs = idleN;
    const memN = parseInt(memory, 10);
    if (!Number.isNaN(memN) && memN > 0) req.memory_mb = memN;
    create.mutate(req);
  }

  return (
    <Panel padded={false}>
      <div className="px-5 py-4">
        <PanelHeader eyebrow="Create" title="New workspace" className="!mb-0" />
      </div>
      <div className="hairline" />
      <form onSubmit={submit} className="p-5 space-y-3">
        <Field
          label="Git repo (optional)"
          placeholder="https://github.com/my-org/app"
          value={repo}
          onChange={(e: React.ChangeEvent<HTMLInputElement>) => setRepo(e.target.value)}
        />
        <Field
          label="Ref (optional)"
          placeholder="main"
          value={ref}
          onChange={(e: React.ChangeEvent<HTMLInputElement>) => setRef(e.target.value)}
        />
        <Field
          label="Label (optional)"
          placeholder="review-bot"
          value={label}
          onChange={(e: React.ChangeEvent<HTMLInputElement>) => setLabel(e.target.value)}
        />
        <div className="grid grid-cols-2 gap-3">
          <Field
            label="Idle timeout (s)"
            placeholder="0"
            inputMode="numeric"
            value={idle}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) => setIdle(e.target.value)}
          />
          <Field
            label="Memory (MB)"
            placeholder="512"
            inputMode="numeric"
            value={memory}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) => setMemory(e.target.value)}
          />
        </div>
        <div className="pt-1">
          <Button type="submit" variant="primary" size="md" disabled={create.isPending}>
            {create.isPending ? "creating…" : "create workspace"}
          </Button>
        </div>
        {create.error && (
          <div className="text-[11.5px] text-[var(--color-siren)]">
            {(create.error as Error).message}
          </div>
        )}
      </form>
    </Panel>
  );
}
