import type { HttpClient } from "./http.js";
import type { ServiceId, Session } from "./types.js";

/**
 * Sessions bundle a set of phantom tokens that can be handed to a sandbox.
 * Each session's tokens die when the session is closed.
 */
export class Sessions {
  constructor(private readonly http: HttpClient) {}

  /** Create a new session. Returns the full record including the env map. */
  async create(params: {
    services: ServiceId[];
    ttlSecs?: number;
  }): Promise<Session> {
    const body: { services: ServiceId[]; ttl_secs?: number } = {
      services: params.services,
    };
    if (params.ttlSecs !== undefined) body.ttl_secs = params.ttlSecs;
    return this.http.request<Session>({
      method: "POST",
      path: "/v1/sessions",
      body,
    });
  }

  /** List every session, newest first. */
  async list(): Promise<Session[]> {
    return this.http.request<Session[]>({
      method: "GET",
      path: "/v1/sessions",
    });
  }

  /** Fetch a session by id. */
  async get(id: string): Promise<Session> {
    return this.http.request<Session>({
      method: "GET",
      path: `/v1/sessions/${encodeURIComponent(id)}`,
    });
  }

  /** Close a session. Revokes every phantom token it issued. */
  async close(id: string): Promise<void> {
    await this.http.request<void>({
      method: "DELETE",
      path: `/v1/sessions/${encodeURIComponent(id)}`,
    });
  }
}
