/**
 * Thin wrapper around `fetch` for server components and server actions.
 * All calls go through the control plane's HTTP API.
 */

const BASE_URL =
  process.env.AGENTJAIL_BASE_URL ?? "http://localhost:7000";
const API_KEY = process.env.AGENTJAIL_API_KEY ?? "";

async function call<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const headers: Record<string, string> = {
    accept: "application/json",
  };
  if (API_KEY) headers.authorization = `Bearer ${API_KEY}`;
  if (body !== undefined) headers["content-type"] = "application/json";

  const res = await fetch(`${BASE_URL}${path}`, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
    cache: "no-store",
  });
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  const parsed = text ? JSON.parse(text) : null;
  if (!res.ok) {
    const msg =
      parsed && typeof parsed === "object" && "error" in parsed
        ? String((parsed as { error: unknown }).error)
        : res.statusText;
    throw new Error(`agentjail ${res.status}: ${msg}`);
  }
  return parsed as T;
}

export type ServiceId = "openai" | "anthropic" | "github" | "stripe";

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

export const api = {
  health: () => call<string>("GET", "/healthz"),
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
    get: (id: string) => call<Session>("GET", `/v1/sessions/${id}`),
    close: (id: string) => call<void>("DELETE", `/v1/sessions/${id}`),
  },
  audit: {
    recent: (limit = 100) =>
      call<{ rows: AuditRow[]; total: number }>(
        "GET",
        `/v1/audit?limit=${limit}`,
      ),
  },
};
