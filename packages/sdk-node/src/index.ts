/**
 * `@agentjail/sdk` — TypeScript client for the agentjail control plane.
 *
 * ```ts
 * import { Agentjail } from "@agentjail/sdk";
 *
 * const aj = new Agentjail({
 *   baseUrl: "http://localhost:7000",
 *   apiKey: process.env.AGENTJAIL_API_KEY!,
 * });
 *
 * await aj.credentials.put({ service: "openai", secret: "sk-real" });
 *
 * const session = await aj.sessions.create({
 *   services: ["openai"],
 *   ttlSecs: 600,
 * });
 * // session.env is the set of env vars to export in the sandbox.
 * // The sandbox sees only phantom tokens; real secrets never leave the host.
 * ```
 */

import { Audit } from "./audit.js";
import { Credentials } from "./credentials.js";
import { HttpClient, type HttpConfig } from "./http.js";
import { Jails } from "./jails.js";
import { Runs } from "./runs.js";
import { Sessions } from "./sessions.js";

export { AgentjailError } from "./http.js";
export type {
  AuditList,
  AuditRow,
  CredentialRecord,
  ExecOptions,
  ExecResult,
  ForkMeta,
  ForkRequest,
  ForkResult,
  JailKind,
  JailRecord,
  JailsList,
  JailStatus,
  NetworkSpec,
  ResourceStats,
  RunRequest,
  SeccompSpec,
  ServiceId,
  Session,
  StreamEvent,
} from "./types.js";

/** Top-level client. Sub-namespaces are independently usable. */
export class Agentjail {
  /** Credentials sub-API. */
  public readonly credentials: Credentials;
  /** Sessions sub-API (create, exec, close). */
  public readonly sessions: Sessions;
  /** One-shot code execution. */
  public readonly runs: Runs;
  /** Audit sub-API. */
  public readonly audit: Audit;
  /** Jail-run ledger sub-API. */
  public readonly jails: Jails;

  constructor(config: HttpConfig) {
    const http = new HttpClient(config);
    this.credentials = new Credentials(http);
    this.sessions = new Sessions(http);
    this.runs = new Runs(http);
    this.audit = new Audit(http);
    this.jails = new Jails(http);
  }
}
