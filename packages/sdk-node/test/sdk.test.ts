/**
 * Vitest suite for @agentjail/sdk. Uses a stub fetch so we stay hermetic.
 */

import { describe, expect, it } from "vitest";
import { Agentjail, AgentjailError } from "../src/index.js";

function fakeFetch(
  handler: (req: { url: string; init: RequestInit }) => Response,
): typeof fetch {
  return async (input, init) => {
    const url = typeof input === "string" ? input : input.toString();
    return handler({ url, init: init ?? {} });
  };
}

function json(body: unknown, init?: ResponseInit): Response {
  return new Response(JSON.stringify(body), {
    ...init,
    headers: { "content-type": "application/json", ...(init?.headers ?? {}) },
  });
}

describe("HttpClient", () => {
  it("rejects missing baseUrl", () => {
    expect(() => new Agentjail({ baseUrl: "" })).toThrow();
  });

  it("strips trailing slashes from baseUrl", async () => {
    let seen = "";
    const aj = new Agentjail({
      baseUrl: "http://api.example///",
      fetch: fakeFetch(({ url }) => {
        seen = url;
        return json([]);
      }),
    });
    await aj.credentials.list();
    expect(seen).toBe("http://api.example/v1/credentials");
  });

  it("sends Authorization when apiKey is set", async () => {
    let seenAuth: string | null = null;
    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "aj_test",
      fetch: fakeFetch(({ init }) => {
        const headers = new Headers(init.headers as HeadersInit);
        seenAuth = headers.get("authorization");
        return json([]);
      }),
    });
    await aj.credentials.list();
    expect(seenAuth).toBe("Bearer aj_test");
  });

  it("omits Authorization when no apiKey", async () => {
    let seenAuth: string | null = "unset";
    const aj = new Agentjail({
      baseUrl: "http://api",
      fetch: fakeFetch(({ init }) => {
        const headers = new Headers(init.headers as HeadersInit);
        seenAuth = headers.get("authorization");
        return json([]);
      }),
    });
    await aj.credentials.list();
    expect(seenAuth).toBeNull();
  });

  it("wraps non-2xx responses in AgentjailError", async () => {
    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "k",
      fetch: fakeFetch(() =>
        json({ error: "bad request: bad secret" }, { status: 400 })
      ),
    });
    try {
      await aj.credentials.put({ service: "openai", secret: "" });
      throw new Error("expected to throw");
    } catch (err) {
      expect(err).toBeInstanceOf(AgentjailError);
      expect((err as AgentjailError).status).toBe(400);
      expect((err as AgentjailError).message).toContain("bad secret");
    }
  });
});

describe("Credentials", () => {
  it("PUT /v1/credentials sends service+secret as JSON", async () => {
    let method = "";
    let bodyText = "";
    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "k",
      fetch: fakeFetch(({ init }) => {
        method = init.method as string;
        bodyText = init.body as string;
        return json({
          service: "openai",
          added_at: "2026-04-16T12:00:00Z",
          updated_at: "2026-04-16T12:00:00Z",
          fingerprint: "deadbeefdeadbeef",
        });
      }),
    });
    const r = await aj.credentials.put({
      service: "openai",
      secret: "sk-real",
    });
    expect(method).toBe("POST");
    expect(JSON.parse(bodyText)).toEqual({
      service: "openai",
      secret: "sk-real",
    });
    expect(r.service).toBe("openai");
  });

  it("DELETE /v1/credentials/:service returns void on 204", async () => {
    let seenUrl = "";
    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "k",
      fetch: fakeFetch(({ url }) => {
        seenUrl = url;
        return new Response(null, { status: 204 });
      }),
    });
    await aj.credentials.delete("openai");
    expect(seenUrl).toBe("http://api/v1/credentials/openai");
  });
});

describe("Sessions", () => {
  it("renames ttlSecs to ttl_secs in the body", async () => {
    let bodyText = "";
    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "k",
      fetch: fakeFetch(({ init }) => {
        bodyText = init.body as string;
        return json({
          id: "sess_abc",
          created_at: "2026-04-16T00:00:00Z",
          expires_at: "2026-04-16T00:10:00Z",
          services: ["openai"],
          env: { OPENAI_API_KEY: "phm_...", OPENAI_BASE_URL: "http://p" },
        });
      }),
    });
    const r = await aj.sessions.create({
      services: ["openai"],
      ttlSecs: 600,
    });
    const parsed = JSON.parse(bodyText);
    expect(parsed).toEqual({ services: ["openai"], ttl_secs: 600 });
    expect(r.id).toBe("sess_abc");
    expect(r.env.OPENAI_API_KEY).toBeTypeOf("string");
  });

  it("omits ttl_secs when ttlSecs is undefined", async () => {
    let bodyText = "";
    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "k",
      fetch: fakeFetch(({ init }) => {
        bodyText = init.body as string;
        return json({
          id: "sess_abc",
          created_at: "2026-04-16T00:00:00Z",
          expires_at: null,
          services: ["openai"],
          env: {},
        });
      }),
    });
    await aj.sessions.create({ services: ["openai"] });
    expect(Object.keys(JSON.parse(bodyText))).toEqual(["services"]);
  });

  it("close sends DELETE", async () => {
    let seenMethod = "";
    let seenUrl = "";
    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "k",
      fetch: fakeFetch(({ url, init }) => {
        seenMethod = init.method as string;
        seenUrl = url;
        return new Response(null, { status: 204 });
      }),
    });
    await aj.sessions.close("sess_abc");
    expect(seenMethod).toBe("DELETE");
    expect(seenUrl).toBe("http://api/v1/sessions/sess_abc");
  });
});

describe("Audit", () => {
  it("includes limit as a query param when set", async () => {
    let seenUrl = "";
    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "k",
      fetch: fakeFetch(({ url }) => {
        seenUrl = url;
        return json({ rows: [], total: 0 });
      }),
    });
    await aj.audit.recent(50);
    expect(seenUrl).toBe("http://api/v1/audit?limit=50");
  });

  it("omits limit when not set", async () => {
    let seenUrl = "";
    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "k",
      fetch: fakeFetch(({ url }) => {
        seenUrl = url;
        return json({ rows: [], total: 0 });
      }),
    });
    await aj.audit.recent();
    expect(seenUrl).toBe("http://api/v1/audit");
  });
});
