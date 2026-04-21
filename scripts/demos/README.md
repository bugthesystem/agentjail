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

## Gateway forwarding for demo #4

`domains: [{ domain, vm_port }]` maps a hostname directly to a port
bound inside the workspace's jail. The gateway resolves the port to
`http://<live_jail_ip>:<vm_port>/` at each request time — no
`backend_url`, no sidecar, no socat / ngrok. Requires the workspace to
run with `network: { mode: "allowlist", domains: [...] }` so the veth
pair that the gateway routes over gets created.

When no exec is in flight (dev server not started, or previous exec
exited), the gateway returns `503 Service Unavailable` with a message
telling the caller to spin up an exec that binds the port. Start the
dev server backgrounded inside a long-running exec, and subsequent
gateway requests will land.
