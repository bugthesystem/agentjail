import type { HttpClient } from "./http.js";
import type { SnapshotList, SnapshotRecord, Workspace } from "./types.js";

/**
 * Named snapshots — capture the current state of a workspace's output
 * directory and restore it into a new workspace later. Uses the engine's
 * freeze-before-copy path when a snapshot is taken mid-exec.
 *
 * ```ts
 * const snap = await aj.snapshots.create(ws.id, { name: "baseline" });
 * // …some risky exec…
 * const restored = await aj.snapshots.createWorkspaceFrom(snap.id);
 * // `restored.output_dir` now mirrors the baseline.
 * ```
 */
export class Snapshots {
  constructor(private readonly http: HttpClient) {}

  /**
   * Capture a snapshot of `workspace_id`'s output dir. If an exec is
   * currently running against the workspace, its cgroup is frozen for the
   * duration of the copy — callers see a consistent view.
   */
  async create(
    workspaceId: string,
    params: { name?: string } = {},
  ): Promise<SnapshotRecord> {
    const body: Record<string, unknown> = {};
    if (params.name !== undefined) body.name = params.name;
    return this.http.request<SnapshotRecord>({
      method: "POST",
      path: `/v1/workspaces/${encodeURIComponent(workspaceId)}/snapshot`,
      body,
    });
  }

  /** List snapshots; optionally filtered to a single workspace. */
  async list(params: {
    workspaceId?: string;
    limit?: number;
    offset?: number;
  } = {}): Promise<SnapshotList> {
    return this.http.request<SnapshotList>({
      method: "GET",
      path: "/v1/snapshots",
      query: {
        workspace_id: params.workspaceId,
        limit: params.limit,
        offset: params.offset,
      },
    });
  }

  /** Fetch a snapshot's metadata. */
  async get(id: string): Promise<SnapshotRecord> {
    return this.http.request<SnapshotRecord>({
      method: "GET",
      path: `/v1/snapshots/${encodeURIComponent(id)}`,
    });
  }

  /** Remove a snapshot + its on-disk dir. Idempotent. */
  async delete(id: string): Promise<void> {
    await this.http.request<void>({
      method: "DELETE",
      path: `/v1/snapshots/${encodeURIComponent(id)}`,
    });
  }

  /**
   * Rehydrate a snapshot into a brand-new workspace. The new workspace
   * inherits its parent's jail config (memory/network/etc) when the parent
   * is still around; otherwise sensible defaults apply.
   */
  async createWorkspaceFrom(
    snapshotId: string,
    params: { label?: string } = {},
  ): Promise<Workspace> {
    const body: Record<string, unknown> = { snapshot_id: snapshotId };
    if (params.label !== undefined) body.label = params.label;
    return this.http.request<Workspace>({
      method: "POST",
      path: "/v1/workspaces/from-snapshot",
      body,
    });
  }
}
