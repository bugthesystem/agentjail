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

/**
 * Network policy for a jail. Match the shapes on the wire:
 *   { mode: "none" }
 *   { mode: "loopback" }
 *   { mode: "allowlist", domains: string[] }
 */
export type NetworkSpec =
  | { mode: "none" }
  | { mode: "loopback" }
  | { mode: "allowlist"; domains: string[] };

/** Seccomp level exposed on the wire. `"disabled"` is intentionally omitted. */
export type SeccompSpec = "standard" | "strict";

/**
 * Optional jail tuning shared by `sessions.exec` and `runs.create`.
 *
 * - `cpuPercent` is clamped server-side to `1..=800` (800 = 8 cores).
 * - `maxPids` is clamped to `1..=1024`.
 * - `network` defaults to `{ mode: "none" }`; `allowlist` domains must be
 *   hostnames or trailing-`*` globs (no scheme).
 */
export interface ExecOptions {
  network?: NetworkSpec;
  seccomp?: SeccompSpec;
  cpuPercent?: number;
  maxPids?: number;
}

/** Parameters for a one-shot run. */
export interface RunRequest extends ExecOptions {
  code: string;
  language?: "javascript" | "python" | "bash";
  timeoutSecs?: number;
  memoryMb?: number;
}

/** Parameters for `aj.runs.fork` — live-forks the parent jail mid-run. */
export interface ForkRequest extends ExecOptions {
  parentCode: string;
  childCode: string;
  language?: "javascript" | "python" | "bash";
  /** How long the parent runs before we freeze + fork. Default 200ms. */
  forkAfterMs?: number;
  timeoutSecs?: number;
  memoryMb?: number;
}

/** Metadata for a completed live_fork. */
export interface ForkMeta {
  clone_ms: number;
  files_cloned: number;
  files_cow: number;
  bytes_cloned: number;
  method: string;
  was_frozen: boolean;
}

/** Response from `aj.runs.fork`. */
export interface ForkResult {
  parent: ExecResult;
  child: ExecResult;
  fork: ForkMeta;
}

/** Kinds of jail run — one per exec endpoint. */
export type JailKind = "run" | "exec" | "fork" | "stream";

/** Jail record lifecycle. */
export type JailStatus = "running" | "completed" | "error";

/** A single row in `GET /v1/jails`. */
export interface JailRecord {
  id: number;
  kind: JailKind;
  started_at: string;        // RFC3339
  ended_at: string | null;
  status: JailStatus;
  session_id: string | null;
  label: string;

  exit_code:   number | null;
  duration_ms: number | null;
  timed_out:   boolean | null;
  oom_killed:  boolean | null;
  memory_peak_bytes: number | null;
  cpu_usage_usec:    number | null;
  io_read_bytes:     number | null;
  io_write_bytes:    number | null;
  stdout: string | null;
  stderr: string | null;
  error:  string | null;
}

export interface JailsList {
  rows:  JailRecord[];
  total: number;
}

/**
 * One event from `aj.runs.stream`. Maps to the server's SSE `event:` frames.
 * `completed` is always the last event before the stream closes.
 */
export type StreamEvent =
  | { type: "started";   pid: number }
  | { type: "stdout";    line: string }
  | { type: "stderr";    line: string }
  | { type: "stats";
      memory_peak_bytes: number;
      cpu_usage_usec: number;
      io_read_bytes: number;
      io_write_bytes: number;
    }
  | { type: "completed";
      exit_code: number;
      duration_ms: number;
      timed_out: boolean;
      oom_killed: boolean;
      memory_peak_bytes: number;
      cpu_usage_usec: number;
    }
  | { type: "error";     message: string };
