#!/usr/bin/env -S bun run
/**
 * App Builder — Lovable / Bolt / V0 shape.
 *
 * Uses `workspaces.create({ git, domains })` to provision a persistent
 * workspace with a hostname route, then issues `workspaces.exec(...)`
 * for each step: install deps, start the dev server backgrounded, poll
 * for readiness.
 *
 * Known gap: `domains: [{ domain, backend_url }]` takes an explicit
 * backend URL — we don't have a host→jail port-forward subsystem today
 * (separate engineering item: veth-NAT + per-jail IP discovery). For
 * the demo we register a placeholder; in production you'd front the
 * jail with `socat` / a reverse tunnel / wireguard and set
 * `backend_url` to that reachable address.
 */

import { aj, cleanup, ok, step } from "./_client.js";

const REPO_URL  = process.env.DEMO_REPO  ?? "https://github.com/bugthesystem/agentjail";
const PROJECT   = process.env.PROJECT_ID ?? `proj-${Math.random().toString(16).slice(2, 8)}`;
const PORT      = 3000;
const DOMAIN    = `${PROJECT}.local.agentjail`;
// In reality you'd point this at a socat/ngrok/wireguard hop that
// reaches the jail's dev server. For this demo we just register a
// placeholder and verify the domain shows up in the workspace record.
const BACKEND_URL = process.env.DEMO_BACKEND_URL ?? `http://127.0.0.1:${PORT}`;

async function main(): Promise<void> {
  step(`Provisioning app-builder workspace for ${PROJECT}`);
  const ws = await aj.workspaces.create({
    git:      { repo: REPO_URL },
    memoryMb: 1024,
    label:    "app-builder",
    // Gateway route (see gap note in the header).
    domains:  [{ domain: DOMAIN, backend_url: BACKEND_URL }],
    // Dev servers want loopback at minimum; allowlist if you need npm/etc.
    network:  { mode: "loopback" },
  });
  ok(`workspace ${ws.id}  domain ${DOMAIN} → ${BACKEND_URL}`);

  try {
    step("bun install (the `workspace.install: true` step)");
    // The demo's jail image may not ship bun — fall through gracefully.
    const inst = await aj.workspaces.exec(ws.id, {
      cmd: "/bin/sh",
      args: ["-c", "command -v bun && bun install || echo 'bun not available; skipping'"],
      timeoutSecs: 120,
    });
    ok(`install exit=${inst.exit_code}`);

    step("Starting `bun run dev` backgrounded (the `task(\"dev\")` step)");
    // Backgrounding a process *inside* an exec lets the exec return
    // quickly. Output goes to /workspace/dev.log so you can tail it
    // from a follow-up exec if needed.
    const start = await aj.workspaces.exec(ws.id, {
      cmd: "/bin/sh",
      args: [
        "-c",
        `nohup bun run dev >/workspace/dev.log 2>&1 & echo $! > /workspace/dev.pid; echo started`,
      ],
      env: [["PORT", String(PORT)]],
    });
    ok(`dev started: ${start.stdout.trim()}`);

    step(`waitFor("curl http://localhost:${PORT}") — jail-internal poll`);
    // The readiness check runs inside the jail via exec. If the curl
    // succeeds, the dev server is listening on the expected port.
    const waited = await aj.workspaces.exec(ws.id, {
      cmd: "/bin/sh",
      args: [
        "-c",
        `for i in $(seq 1 30); do ` +
          `curl -fsS --max-time 1 http://localhost:${PORT}/ >/dev/null && echo up && exit 0; ` +
          `sleep 1; done; echo 'not up (expected unless the repo actually runs a dev server)'; exit 0`,
      ],
      timeoutSecs: 60,
    });
    ok(`waitFor: ${waited.stdout.trim()}`);

    step("Verifying the gateway route is registered on the workspace");
    const refreshed = await aj.workspaces.get(ws.id);
    if (refreshed.domains.some((d) => d.domain === DOMAIN)) {
      console.log(
        `  ${DOMAIN} is routable at the gateway (set AGENTJAIL_GATEWAY_ADDR on the server to serve it)`,
      );
    } else {
      console.warn(`  expected ${DOMAIN} in workspace.domains but didn't find it`);
    }
  } finally {
    await cleanup({ workspaces: [ws.id] });
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
