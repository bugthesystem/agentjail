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
- `sessions.create({services, ttlSecs?})` / `list()` / `get(id)` / `close(id)`
- `audit.recent(limit?)`

Zero runtime dependencies. Works on Node ≥ 18 and any runtime that
exposes a global `fetch`.

[agentjail]: https://github.com/bugthesystem/agentjail
