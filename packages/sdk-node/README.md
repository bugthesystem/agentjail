# @agentjail/sdk

TypeScript client for the [agentjail] control plane.

Your sandbox sees `OPENAI_API_KEY=phm_...` and an `OPENAI_BASE_URL` that
points at a local proxy; the proxy swaps the phantom token for the real
key and streams the request upstream. No real credentials ever enter the
jail.

```ts
import { Agentjail } from "@agentjail/sdk";

const aj = new Agentjail({
  baseUrl: "http://localhost:7000",
  apiKey: process.env.AGENTJAIL_API_KEY!,
});

await aj.credentials.put({ service: "openai", secret: "sk-real" });

const session = await aj.sessions.create({
  services: ["openai"],
  ttlSecs: 600,
});

// Hand `session.env` to whatever sandbox you use:
spawn("node", ["my-agent.js"], { env: { ...process.env, ...session.env } });
```

## API

- `credentials.list()` / `put({service, secret})` / `delete(service)`
- `sessions.create({services, scopes?, ttlSecs?})` / `list()` / `get(id)` / `close(id)`
- `sessions.exec(id, {cmd, args?, ...})`
- `runs.create({code, language?, ...})` / `fork({parentCode, childCode | children})` / `stream(...)`
- `workspaces.create(...)` / `list()` / `get(id)` / `delete(id)` / `exec(id, {cmd, args?})`
- `snapshots.create(workspaceId, {name?})` / `list(...)` / `get(id)` / `delete(id)` / `createWorkspaceFrom(snapshotId, {label?})`
- `audit.recent(limit?)`
- `jails.list(...)` / `jails.get(id)`
- `public.health()` / `public.stats()` — no auth

Supported services: `openai`, `anthropic`, `github`, `stripe`.

**Scopes** are a per-service allow-list of path globs (trailing `*`
supported). Requests outside the scope get rejected at the proxy with
a 403 — the upstream is never contacted.

```ts
await aj.sessions.create({
  services: ["github"],
  scopes:   { github: ["/repos/my-org/*/issues*"] },
});
```

### Persistent workspaces + snapshots

Workspaces are long-lived mount trees. Multiple execs share the same
filesystem, so `bun install` in one call sticks around for the next. A
snapshot captures the workspace's output dir; snapshots rehydrate into
new workspaces.

```ts
const ws = await aj.workspaces.create({
  git: { repo: "https://github.com/my-org/app", ref: "main" },
  label: "ci",
});

await aj.workspaces.exec(ws.id, { cmd: "bun", args: ["install"] });

const baseline = await aj.snapshots.create(ws.id, { name: "deps-ready" });

const lint = await aj.workspaces.exec(ws.id, { cmd: "bun", args: ["run", "lint"] });
if (lint.exit_code !== 0) {
  // Roll back to the post-install state in a fresh workspace.
  const clean = await aj.snapshots.createWorkspaceFrom(baseline.id, {
    label: "recovered",
  });
  // …retry against `clean.id`.
}
```

Snapshots taken while an exec is in flight freeze the jail's cgroup
around the filesystem copy (sub-ms on cgroup v2; on systems without
cgroup freeze the copy falls back to a plain read).

Zero runtime dependencies. Works on Node ≥ 18 and any runtime that
exposes a global `fetch`.

[agentjail]: https://github.com/bugthesystem/agentjail
