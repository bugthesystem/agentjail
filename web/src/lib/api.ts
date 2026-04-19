import type { ServiceId } from "./format";

export type { ServiceId };

export interface CredentialRecord {
  service: ServiceId;
  added_at: string;
  updated_at: string;
  fingerprint: string;
}

export interface Session {
  id: string;
  created_at: string;
  expires_at: string | null;
  services: ServiceId[];
  env: Record<string, string>;
}

export interface AuditRow {
  id: number;
  at: string;
  session_id: string;
  service: string;
  method: string;
  path: string;
  status: number;
  reject_reason: string | null;
  upstream_ms: number | null;
}

export interface AuditList {
  rows: AuditRow[];
  total: number;
}

export interface Stats {
  active_execs: number;
  total_execs: number;
  sessions: number;
  credentials: number;
}

export interface ExecResult {
  stdout: string;
  stderr: string;
  exit_code: number;
  duration_ms: number;
  timed_out: boolean;
  oom_killed: boolean;
  stats?: {
    memory_peak_bytes: number;
    cpu_usage_usec: number;
    io_read_bytes: number;
    io_write_bytes: number;
  };
}

export class ApiError extends Error {
  readonly status: number;
  constructor(status: number, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

export function createApi(baseUrl: string, apiKey: string) {
  async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
    const headers: Record<string, string> = { accept: "application/json" };
    if (apiKey) headers.authorization = `Bearer ${apiKey}`;
    if (body !== undefined) headers["content-type"] = "application/json";

    const res = await fetch(`${baseUrl}${path}`, {
      method,
      headers,
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });

    if (res.status === 204) return undefined as T;
    const text = await res.text();
    const parsed = text ? safeParse(text) : null;
    if (!res.ok) {
      const msg =
        parsed && typeof parsed === "object" && "error" in parsed
          ? String((parsed as { error: unknown }).error)
          : res.statusText || `HTTP ${res.status}`;
      throw new ApiError(res.status, msg);
    }
    return (parsed ?? text) as T;
  }

  function safeParse(text: string): unknown {
    try {
      return JSON.parse(text);
    } catch {
      return null;
    }
  }

  return {
    health: () => call<string>("GET", "/healthz"),
    stats: () => call<Stats>("GET", "/v1/stats"),

    credentials: {
      list: () => call<CredentialRecord[]>("GET", "/v1/credentials"),
      put: (service: ServiceId, secret: string) =>
        call<CredentialRecord>("POST", "/v1/credentials", { service, secret }),
      delete: (service: ServiceId) =>
        call<void>("DELETE", `/v1/credentials/${service}`),
    },

    sessions: {
      list: () => call<Session[]>("GET", "/v1/sessions"),
      create: (services: ServiceId[], ttlSecs?: number) =>
        call<Session>("POST", "/v1/sessions", {
          services,
          ...(ttlSecs !== undefined ? { ttl_secs: ttlSecs } : {}),
        }),
      close: (id: string) => call<void>("DELETE", `/v1/sessions/${id}`),
    },

    runs: {
      create: (code: string, language = "javascript", timeoutSecs?: number) =>
        call<ExecResult>("POST", "/v1/runs", {
          code,
          language,
          ...(timeoutSecs !== undefined ? { timeout_secs: timeoutSecs } : {}),
        }),
    },

    audit: {
      recent: (limit = 100) =>
        call<AuditList>("GET", `/v1/audit?limit=${limit}`),
    },
  };
}

export type Api = ReturnType<typeof createApi>;
