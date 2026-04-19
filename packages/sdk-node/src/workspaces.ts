import type { HttpClient } from "./http.js";
import type {
  ExecResult,
  Workspace,
  WorkspaceCreateRequest,
  WorkspaceExecRequest,
  WorkspaceList,
} from "./types.js";
import { encodeExecOptions } from "./exec_options.js";

/**
 * Persistent workspaces — long-lived mount trees that survive across
 * multiple exec calls. Use these when you want filesystem mutations
 * (installed deps, generated files, partial builds) to persist between
 * commands, à la a VM with a working directory.
 *
 * ```ts
 * const ws = await aj.workspaces.create({
 *   git: { repo: "https://github.com/org/repo" },
 *   label: "review-bot",
 * });
 * await aj.workspaces.exec(ws.id, { cmd: "bun", args: ["install"] });
 * const test = await aj.workspaces.exec(ws.id, {
 *   cmd: "bun",
 *   args: ["test"],
 * });
 * if (test.exit_code !== 0) {
 *   const snap = await aj.snapshots.create(ws.id, { name: "failed-run" });
 * }
 * ```
 */
export class Workspaces {
  constructor(private readonly http: HttpClient) {}

  /** Create a workspace; optionally clones a git repo into its source dir. */
  async create(params: WorkspaceCreateRequest = {}): Promise<Workspace> {
    const body: Record<string, unknown> = {};
    if (params.git) {
      const git: Record<string, unknown> = { repo: params.git.repo };
      if (params.git.ref) git.ref = params.git.ref;
      body.git = git;
    }
    if (params.label !== undefined)           body.label             = params.label;
    if (params.memoryMb !== undefined)        body.memory_mb         = params.memoryMb;
    if (params.timeoutSecs !== undefined)     body.timeout_secs      = params.timeoutSecs;
    if (params.idleTimeoutSecs !== undefined) body.idle_timeout_secs = params.idleTimeoutSecs;
    encodeExecOptions(body, params);
    return this.http.request<Workspace>({
      method: "POST",
      path: "/v1/workspaces",
      body,
    });
  }

  /** Paginated list, newest first. */
  async list(params: { limit?: number; offset?: number } = {}): Promise<WorkspaceList> {
    return this.http.request<WorkspaceList>({
      method: "GET",
      path: "/v1/workspaces",
      query: {
        limit: params.limit,
        offset: params.offset,
      },
    });
  }

  /** Fetch a workspace by id. Deleted workspaces return 404. */
  async get(id: string): Promise<Workspace> {
    return this.http.request<Workspace>({
      method: "GET",
      path: `/v1/workspaces/${encodeURIComponent(id)}`,
    });
  }

  /** Soft-delete + on-disk cleanup. Snapshots of this workspace survive. */
  async delete(id: string): Promise<void> {
    await this.http.request<void>({
      method: "DELETE",
      path: `/v1/workspaces/${encodeURIComponent(id)}`,
    });
  }

  /**
   * Run a command against the workspace's persistent filesystem. Returns
   * 409 if another exec is in flight against the same workspace.
   */
  async exec(id: string, params: WorkspaceExecRequest): Promise<ExecResult> {
    const body: Record<string, unknown> = { cmd: params.cmd };
    if (params.args)                       body.args         = params.args;
    if (params.timeoutSecs !== undefined)  body.timeout_secs = params.timeoutSecs;
    if (params.memoryMb    !== undefined)  body.memory_mb    = params.memoryMb;
    if (params.env)                        body.env          = params.env;
    return this.http.request<ExecResult>({
      method: "POST",
      path: `/v1/workspaces/${encodeURIComponent(id)}/exec`,
      body,
    });
  }
}
