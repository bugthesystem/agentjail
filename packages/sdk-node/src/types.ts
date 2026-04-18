/**
 * Shared types across the SDK. These mirror the control-plane's JSON schemas
 * exactly — keep them in sync with `crates/agentjail-ctl/src/routes.rs`.
 */

/** Stable string identifiers for upstream services. */
export type ServiceId = "openai" | "anthropic" | "github" | "stripe";

/** Public credential metadata. Never carries the secret. */
export interface CredentialRecord {
  service: ServiceId;
  /** RFC3339 timestamp. */
  added_at: string;
  /** RFC3339 timestamp. */
  updated_at: string;
  /** Non-reversible fingerprint of the secret (for rotation detection). */
  fingerprint: string;
}

/** Creating a session returns this. */
export interface Session {
  id: string;
  /** RFC3339. */
  created_at: string;
  /** RFC3339 or null. */
  expires_at: string | null;
  services: ServiceId[];
  /**
   * The environment variables to export in the sandbox. Contains the
   * phantom token(s) and matching *_BASE_URL entries pointing at the
   * phantom proxy.
   */
  env: Record<string, string>;
}

/** One row of the phantom-proxy audit log. */
export interface AuditRow {
  id: number;
  /** RFC3339 timestamp. */
  at: string;
  session_id: string;
  service: string;
  method: string;
  path: string;
  status: number;
  reject_reason: string | null;
  upstream_ms: number | null;
}

/** Paginated audit response. */
export interface AuditList {
  rows: AuditRow[];
  total: number;
}

/** Result of executing a command in a session's jail. */
export interface ExecResult {
  stdout: string;
  stderr: string;
  exit_code: number;
  duration_ms: number;
  timed_out: boolean;
  oom_killed: boolean;
  stats?: ResourceStats;
}

/** Jail resource usage statistics. */
export interface ResourceStats {
  memory_peak_bytes: number;
  cpu_usage_usec: number;
  io_read_bytes: number;
  io_write_bytes: number;
}

/** Parameters for a one-shot run. */
export interface RunRequest {
  code: string;
  language?: "javascript" | "python" | "bash";
  timeoutSecs?: number;
  memoryMb?: number;
}
