/**
 * Minimal HTTP client. Zero runtime deps — uses global `fetch`.
 */

/** An error from the control plane. */
export class AgentjailError extends Error {
  public readonly status: number;
  public readonly body: unknown;

  constructor(status: number, body: unknown, fallback: string) {
    const message =
      (typeof body === "object" && body !== null && "error" in body &&
        typeof (body as { error: unknown }).error === "string")
        ? (body as { error: string }).error
        : fallback;
    super(`agentjail ${status}: ${message}`);
    this.name = "AgentjailError";
    this.status = status;
    this.body = body;
  }
}

/** Options for an HTTP call. */
export interface RequestOptions {
  method: string;
  path: string;
  body?: unknown;
  query?: Record<string, string | number | undefined>;
  signal?: AbortSignal;
}

/** Per-client HTTP config. */
export interface HttpConfig {
  /** Base URL of the control plane, no trailing slash. */
  baseUrl: string;
  /** API key sent as `Authorization: Bearer <key>`. */
  apiKey?: string;
  /** Fetch implementation. Defaults to `globalThis.fetch`. */
  fetch?: typeof fetch;
}

/** Tiny typed fetch wrapper. */
export class HttpClient {
  private readonly baseUrl: string;
  private readonly apiKey: string | undefined;
  private readonly fetchFn: typeof fetch;

  constructor(config: HttpConfig) {
    if (!config.baseUrl) {
      throw new Error("baseUrl is required");
    }
    this.baseUrl = config.baseUrl.replace(/\/+$/, "");
    this.apiKey = config.apiKey;
    this.fetchFn = config.fetch ?? globalThis.fetch;
    if (!this.fetchFn) {
      throw new Error(
        "no fetch implementation — pass one via `fetch` on Node < 18",
      );
    }
  }

  async request<T>(opts: RequestOptions): Promise<T> {
    const url = new URL(`${this.baseUrl}${opts.path}`);
    if (opts.query) {
      for (const [k, v] of Object.entries(opts.query)) {
        if (v !== undefined) url.searchParams.set(k, String(v));
      }
    }
    const headers: Record<string, string> = {
      accept: "application/json",
    };
    if (this.apiKey) {
      headers.authorization = `Bearer ${this.apiKey}`;
    }
    let bodyInit: BodyInit | undefined;
    if (opts.body !== undefined) {
      headers["content-type"] = "application/json";
      bodyInit = JSON.stringify(opts.body);
    }
    const init: RequestInit = {
      method: opts.method,
      headers,
    };
    if (bodyInit !== undefined) init.body = bodyInit;
    if (opts.signal) init.signal = opts.signal;
    const res = await this.fetchFn(url.toString(), init);
    if (res.status === 204) {
      return undefined as T;
    }
    const text = await res.text();
    const parsed: unknown = text.length > 0 ? JSON.parse(text) : null;
    if (!res.ok) {
      throw new AgentjailError(res.status, parsed, res.statusText);
    }
    return parsed as T;
  }
}
