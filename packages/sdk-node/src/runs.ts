import type { HttpClient } from "./http.js";
import type { ExecResult, RunRequest } from "./types.js";

/** One-shot code execution. No session management needed. */
export class Runs {
  constructor(private readonly http: HttpClient) {}

  /** Run code in a fresh jail and return the result. */
  async create(params: RunRequest): Promise<ExecResult> {
    const body: Record<string, unknown> = { code: params.code };
    if (params.language) body.language = params.language;
    if (params.timeoutSecs !== undefined) body.timeout_secs = params.timeoutSecs;
    if (params.memoryMb !== undefined) body.memory_mb = params.memoryMb;
    return this.http.request<ExecResult>({
      method: "POST",
      path: "/v1/runs",
      body,
    });
  }
}
