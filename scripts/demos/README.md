# Demos

Four runnable scripts demonstrating the agentjail workspace surface,
each one mirroring a well-known product pattern. They use the real
[`@agentjail/sdk`](../../packages/sdk-node/) and run end-to-end against
a live control plane.

| Script | Pattern | Key features |
|---|---|---|
| [`1-ai-assistant.ts`](1-ai-assistant.ts)    | OpenClaw / Claude / Cowork    | persistent workspace, idle auto-pause |
| [`2-review-bot.ts`](2-review-bot.ts)        | Code Rabbit / Greptile        | git clone, multi-exec, branch on output |
| [`3-background-agent.ts`](3-background-agent.ts) | Devin / Cursor Agent      | N-way workspace fork, parallel execs |
| [`4-app-builder.ts`](4-app-builder.ts)      | Lovable / Bolt / V0           | dev server + hostname-routed gateway |

## Running

```bash
# Start the stack (if not already up)
make dev

# Then any of:
bun scripts/demos/1-ai-assistant.ts
bun scripts/demos/2-review-bot.ts
bun scripts/demos/3-background-agent.ts
bun scripts/demos/4-app-builder.ts
```

All four respect the same env vars:

- `CTL_URL` — control plane base URL (default `http://localhost:7070`)
- `AGENTJAIL_API_KEY` — bearer token from `.env.local`
- `DEMO_REPO` / `DEMO_REF` — override the repo the git-based demos clone

## `ai(vm, prompt)` stand-in

All four reuse a tiny [`_client.ts`](./_client.ts) helper with an `ai`
placeholder that just echoes the prompt. Real deployments wire phantom
tokens through a session and run a Python/Node script that calls Claude
(or similar) inside the jail — the shape of the loop doesn't change.

## Gap note for demo #4

The App Builder pattern would ideally map a hostname directly to a
**jail-internal** port (`domain + vmPort`). Our gateway forwards to a
caller-supplied `backend_url` instead — there's no host→jail
port-forward subsystem today (a separate item: veth-NAT + per-jail IP
discovery). Demo #4 registers the domain and starts the dev server, but
you supply the `backend_url` yourself (e.g. a `socat` / `ngrok` bridge,
or a Docker-networked sidecar). See the comment at the top of
[`4-app-builder.ts`](4-app-builder.ts) for the details.
