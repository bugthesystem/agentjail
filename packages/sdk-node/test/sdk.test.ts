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

  it("passes scopes through", async () => {
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
          services: ["github"],
          env: {},
        });
      }),
    });
    await aj.sessions.create({
      services: ["github"],
      scopes: { github: ["/repos/foo/*"] },
    });
    expect(JSON.parse(bodyText)).toEqual({
      services: ["github"],
      scopes: { github: ["/repos/foo/*"] },
    });
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

  it("exec sends POST with cmd and args", async () => {
    let seenUrl = "";
    let bodyText = "";
    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "k",
      fetch: fakeFetch(({ url, init }) => {
        seenUrl = url;
        bodyText = init.body as string;
        return json({
          stdout: "hi\n",
          stderr: "",
          exit_code: 0,
          duration_ms: 42,
          timed_out: false,
          oom_killed: false,
        });
      }),
    });
    const r = await aj.sessions.exec("sess_1", {
      cmd: "echo",
      args: ["hi"],
      timeoutSecs: 10,
    });
    expect(seenUrl).toBe("http://api/v1/sessions/sess_1/exec");
    expect(JSON.parse(bodyText)).toEqual({
      cmd: "echo",
      args: ["hi"],
      timeout_secs: 10,
    });
    expect(r.stdout).toBe("hi\n");
    expect(r.exit_code).toBe(0);
  });
});

describe("Runs", () => {
  it("create sends POST /v1/runs with code", async () => {
    let seenUrl = "";
    let bodyText = "";
    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "k",
      fetch: fakeFetch(({ url, init }) => {
        seenUrl = url;
        bodyText = init.body as string;
        return json({
          stdout: "hello\n",
          stderr: "",
          exit_code: 0,
          duration_ms: 100,
          timed_out: false,
          oom_killed: false,
        });
      }),
    });
    const r = await aj.runs.create({
      code: "console.log('hello')",
      language: "javascript",
      timeoutSecs: 30,
    });
    expect(seenUrl).toBe("http://api/v1/runs");
    expect(JSON.parse(bodyText)).toEqual({
      code: "console.log('hello')",
      language: "javascript",
      timeout_secs: 30,
    });
    expect(r.stdout).toBe("hello\n");
  });

  it("forwards ExecOptions (network allowlist, seccomp, cpu, pids) in snake_case", async () => {
    let bodyText = "";
    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "k",
      fetch: fakeFetch(({ init }) => {
        bodyText = init.body as string;
        return json({
          stdout: "", stderr: "", exit_code: 0,
          duration_ms: 1, timed_out: false, oom_killed: false,
        });
      }),
    });
    await aj.runs.create({
      code: "fetch('https://api.openai.com')",
      language: "javascript",
      network: { mode: "allowlist", domains: ["api.openai.com"] },
      seccomp: "strict",
      cpuPercent: 200,
      maxPids: 128,
    });
    expect(JSON.parse(bodyText)).toEqual({
      code: "fetch('https://api.openai.com')",
      language: "javascript",
      network: { mode: "allowlist", domains: ["api.openai.com"] },
      seccomp: "strict",
      cpu_percent: 200,
      max_pids: 128,
    });
  });

  it("fork sends POST /v1/runs/fork with parent_code/child_code + options", async () => {
    let seenUrl = "";
    let bodyText = "";
    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "k",
      fetch: fakeFetch(({ url, init }) => {
        seenUrl = url;
        bodyText = init.body as string;
        const parentExec = {
          stdout: "parent\n", stderr: "", exit_code: 0,
          duration_ms: 220, timed_out: false, oom_killed: false,
        };
        const childExec = {
          stdout: "child\n", stderr: "", exit_code: 0,
          duration_ms: 180, timed_out: false, oom_killed: false,
        };
        return json({
          parent: parentExec,
          child: childExec,
          fork: {
            clone_ms: 3, files_cloned: 1, files_cow: 1,
            bytes_cloned: 42, method: "reflink", was_frozen: true,
          },
        });
      }),
    });
    const r = await aj.runs.fork({
      parentCode: 'require("fs").writeFileSync("/output/ckpt","42")',
      childCode:  'console.log(require("fs").readFileSync("/output/ckpt","utf8"))',
      language: "javascript",
      forkAfterMs: 300,
      memoryMb: 256,
      seccomp: "strict",
    });
    expect(seenUrl).toBe("http://api/v1/runs/fork");
    expect(JSON.parse(bodyText)).toEqual({
      parent_code: 'require("fs").writeFileSync("/output/ckpt","42")',
      child_code:  'console.log(require("fs").readFileSync("/output/ckpt","utf8"))',
      language: "javascript",
      fork_after_ms: 300,
      memory_mb: 256,
      seccomp: "strict",
    });
    expect(r.parent.stdout).toBe("parent\n");
    expect(r.child.stdout).toBe("child\n");
    expect(r.fork.method).toBe("reflink");
    expect(r.fork.was_frozen).toBe(true);
  });

  it("stream parses SSE frames into typed events", async () => {
    const sse =
      `event: started\ndata: {"pid":1234}\n\n` +
      `event: stdout\ndata: hello\n\n` +
      `event: stdout\ndata: world\n\n` +
      `event: stderr\ndata: warn\n\n` +
      `event: completed\ndata: {"exit_code":0,"duration_ms":42,"timed_out":false,"oom_killed":false,"memory_peak_bytes":1048576,"cpu_usage_usec":1000}\n\n`;

    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "k",
      fetch: fakeFetch(() =>
        new Response(sse, {
          status: 200,
          headers: { "content-type": "text/event-stream" },
        })
      ),
    });

    const events = [];
    for await (const ev of aj.runs.stream({ code: "console.log('hi')", language: "javascript" })) {
      events.push(ev);
    }
    expect(events).toEqual([
      { type: "started", pid: 1234 },
      { type: "stdout", line: "hello" },
      { type: "stdout", line: "world" },
      { type: "stderr", line: "warn" },
      {
        type: "completed", exit_code: 0, duration_ms: 42,
        timed_out: false, oom_killed: false,
        memory_peak_bytes: 1048576, cpu_usage_usec: 1000,
      },
    ]);
  });

  it("sessions.exec forwards ExecOptions too", async () => {
    let bodyText = "";
    const aj = new Agentjail({
      baseUrl: "http://api",
      apiKey: "k",
      fetch: fakeFetch(({ init }) => {
        bodyText = init.body as string;
        return json({
          stdout: "", stderr: "", exit_code: 0,
          duration_ms: 1, timed_out: false, oom_killed: false,
        });
      }),
    });
    await aj.sessions.exec("sess_x", {
      cmd: "node",
      args: ["-e", "1+1"],
      memoryMb: 256,
      network: { mode: "loopback" },
      seccomp: "standard",
    });
    expect(JSON.parse(bodyText)).toEqual({
      cmd: "node",
      args: ["-e", "1+1"],
      memory_mb: 256,
      network: { mode: "loopback" },
      seccomp: "standard",
    });
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
