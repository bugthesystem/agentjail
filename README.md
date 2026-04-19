<p align="center">
  <img src="logo.svg" width="100" height="100" alt="agentjail logo">
</p>

<h1 align="center">agentjail</h1>

<p align="center">
  Minimal Linux sandbox for running untrusted code
</p>

---

Built for AI agents, build systems, and any scenario where you need to execute code you didn't write.

## Features

- **Rootless** — No root required, uses user namespaces
- **Network isolation** — None, loopback-only, or domain allowlist via built-in proxy
- **Filesystem isolation** — Chroot with minimal system mounts
- **Resource limits** — Memory, CPU, PIDs, and disk I/O via cgroups v2
- **Syscall filtering** — Seccomp-BPF with comprehensive blocklist
- **GPU passthrough** _(experimental)_ — NVIDIA CUDA/PyTorch with per-GPU isolation
- **OOM detection** — Know when builds fail due to memory limits
- **Snapshotting** — Save/restore output directory for faster rebuilds
- **Live forking** — Clone a running jail in milliseconds via COW filesystem cloning
- **Event streaming** — Real-time stdout/stderr for build servers

## Installation

```toml
[dependencies]
agentjail = "0.1"
tokio = { version = "1", features = ["rt", "macros"] }
```

## Quick Start

```rust
use agentjail::{Jail, preset_build};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let jail = Jail::new(preset_build("./src", "./out"))?;
    let result = jail.run("npm", &["run", "build"]).await?;

    println!("Exit: {} | OOM: {}", result.exit_code, result.oom_killed);
    Ok(())
}
```

## Configuration

```rust
use agentjail::{Jail, JailConfig, Network, SeccompLevel};

let config = JailConfig {
    source: "/code".into(),           // Read-only at /workspace
    output: "/artifacts".into(),      // Read-write at /output
    network: Network::None,           // Or Loopback, Allowlist
    seccomp: SeccompLevel::Standard,  // Or Strict, Disabled
    memory_mb: 512,
    cpu_percent: 100,                 // 100 = 1 core
    max_pids: 64,
    io_read_mbps: 100,                // 0 = unlimited
    io_write_mbps: 50,
    timeout_secs: 300,
    ..Default::default()
};

let jail = Jail::new(config)?;
```

## Network Modes

| Mode | Access | Use Case |
|------|--------|----------|
| `Network::None` | No network | Builds with vendored deps |
| `Network::Loopback` | localhost only | Dev servers, local services |
| `Network::Allowlist(vec![...])` | Whitelisted domains | AI agents, npm install |

### Allowlist Mode

For agents that need external access (MCP, APIs, npm):

```rust
let config = JailConfig {
    network: Network::Allowlist(vec![
        "api.anthropic.com".into(),
        "api.openai.com".into(),
        "registry.npmjs.org".into(),
        "*.mcp.company.com".into(),  // Wildcards supported
    ]),
    ..Default::default()
};
```

A built-in CONNECT proxy runs in the parent process with real network
access. The jailed process connects through a veth pair — it can only
reach the proxy, not the internet directly. DNS is resolved by the proxy
at connection time (not stale). Veth pairs and routing are configured
via direct netlink syscalls (no external tools required). Uses HTTP
CONNECT tunneling, so all TLS-based protocols work:
- HTTPS APIs
- SSE streams over HTTPS
- WebSocket connections (MCP)

## Presets

| Preset | Use Case | Network | Memory | Timeout |
|--------|----------|---------|--------|---------|
| `preset_build` | Offline builds (vendored deps) | None | 512MB | 10 min |
| `preset_install` | npm install, cargo build | Allowlist | 512MB | 10 min |
| `preset_agent` | AI agent execution | None | 256MB | 5 min |
| `preset_gpu` | CUDA / PyTorch / ML training | None | 8GB | 1 hour |
| `preset_dev` | Dev servers (HMR) | Loopback | 1GB | 1 hour |

`preset_install` requires the caller to specify allowed domains:

```rust
use agentjail::{Jail, preset_install};

let jail = Jail::new(preset_install("./src", "./out", vec![
    "registry.npmjs.org".into(),
    "registry.yarnpkg.com".into(),
]))?;
let result = jail.run("npm", &["install"]).await?;
```

## GPU Passthrough (Experimental)

> **Warning:** GPU passthrough exposes the NVIDIA kernel driver attack
> surface. Use for trusted workloads only.

```rust
use agentjail::{Jail, preset_gpu};

let jail = Jail::new(preset_gpu("./src", "./out"))?;
let result = jail.run("python3", &["train.py"]).await?;
```

Or enable GPU on any preset:

```rust
use agentjail::{Jail, JailConfig, GpuConfig};

let config = JailConfig {
    gpu: GpuConfig { enabled: true, devices: vec![0] }, // GPU 0 only
    ..Default::default()
};
```

## Resource Monitoring

```rust
let handle = jail.spawn("npm", &["run", "build"])?;

if let Some(stats) = handle.stats() {
    println!("Memory: {} MB", stats.memory_peak_bytes / 1024 / 1024);
    println!("I/O: {} MB written", stats.io_write_bytes / 1024 / 1024);
}

let output = handle.wait().await?;
if output.oom_killed {
    eprintln!("Build killed by OOM!");
}
```

## Snapshotting

```rust
use agentjail::Snapshot;

let snap = Snapshot::create(&output_dir, &snapshot_dir)?;
snap.restore()?;
println!("Snapshot: {} MB", snap.size_bytes() / 1024 / 1024);
```

## Live Forking

Clone a running jail without pausing it — get full copies in milliseconds:

```rust
let jail = Jail::new(config)?;
let handle = jail.spawn("python", &["train.py"])?;

// Fork while running. Filesystem state is COW-cloned instantly.
let (forked, info) = jail.live_fork(Some(&handle), "/tmp/fork-output")?;
println!("Forked in {:?} ({:?})", info.clone_duration, info.clone_method);

// Use the normal spawn/run API on the forked jail.
let result = forked.run("python", &["evaluate.py"]).await?;
```

Uses `FICLONE` ioctl for instant reflink copies on btrfs/xfs. Falls back
to regular copy on other filesystems. The original jail is frozen for
sub-millisecond via the cgroup freezer, then immediately resumed. Multiple
forks from the same running jail work independently.

## Event Streaming

```rust
let (handle, mut rx) = jail.spawn_with_events("npm", &["run", "build"])?;

while let Some(event) = rx.recv().await {
    match event {
        JailEvent::Stdout(line) => println!("{}", line),
        JailEvent::Stderr(line) => eprintln!("{}", line),
        JailEvent::OomKilled => eprintln!("OOM!"),
        JailEvent::Completed { exit_code, .. } => break,
        _ => {}
    }
}
```

## Security

### Layers

1. **Namespaces** — Isolated mount, network, IPC, PID, user
2. **Chroot** — Minimal filesystem (no host /etc, /home, /root)
3. **Seccomp** — Comprehensive syscall blocklist
4. **Cgroups v2** — Resource limits, assigned before child executes
5. **Landlock** — Filesystem access control (Linux 5.13+)
6. **Hardening** — `PR_SET_NO_NEW_PRIVS`, `RLIMIT_NOFILE`, `RLIMIT_CORE=0`

### What Gets Blocked

| Attack | Protection |
|--------|------------|
| Read ~/.ssh, ~/.aws | Not mounted |
| Read /etc/shadow, ssh keys | Minimal /etc (only ld.so, resolv.conf, ssl) |
| Network exfiltration | Network namespace + allowlist proxy |
| Fork bombs | PID limit via cgroup |
| Memory exhaustion | Memory limit + OOM detection |
| Disk thrashing | I/O bandwidth limits |
| Signal host processes | PID namespace |
| Mount manipulation | `mount`, `mount_setattr`, new mount API blocked |
| io_uring bypass | `io_uring_setup`/`enter`/`register` blocked |
| 32-bit compat escape | `personality()` blocked |
| Namespace escape | `clone3`, `unshare`, `setns` blocked |
| BPF/perf abuse | `bpf`, `perf_event_open`, `userfaultfd` blocked |
| Executable memory | `memfd_create` blocked |
| Write+execute on /tmp | NOEXEC mount flag |
| Setuid escalation | `PR_SET_NO_NEW_PRIVS` before exec |
| Core dump leaks | `RLIMIT_CORE=0` |
| Stdout OOM of parent | Output capped at 256 MiB per stream |
| FD exhaustion | `RLIMIT_NOFILE` capped at 4096 |
| Symlink traversal | Skipped in snapshots, forks, and cleanup |
| Zombie/fd leak on crash | `PR_SET_PDEATHSIG` + kill+reap in Drop |
| Unconstrained child | Cgroup assigned via barrier pipe before exec |
| PID reuse kill | Reaped flag prevents killing recycled PIDs |

### Audit Status

The codebase has been through 4 rounds of security audit covering every
source file. All critical and high severity issues have been fixed with
72 regression tests. See the test suite for specific attack scenarios
that are verified on every build.

## TypeScript SDK

`@agentjail/sdk` talks to the agentjail control plane (HTTP). Sandboxes
get phantom tokens (`phm_<hex>`) and a `*_BASE_URL` pointing at a local
proxy — real API keys never enter the jail. Zero runtime dependencies,
Node ≥ 18.

```ts
import { Agentjail } from "@agentjail/sdk";

const aj = new Agentjail({
  baseUrl: "http://localhost:7000",
  apiKey: process.env.AGENTJAIL_API_KEY!,
});

await aj.credentials.put({ service: "openai", secret: process.env.OPENAI_API_KEY! });

// One-shot run in a fresh jail.
const result = await aj.runs.create({
  code: "print('hi from jail')",
  language: "python",
  timeoutSecs: 30,
});

// Stream stdout/stderr as they happen.
for await (const ev of aj.runs.stream({ code, language: "python" })) {
  if (ev.type === "stdout") process.stdout.write(ev.line + "\n");
}

// Or mint a session and hand its phantom env to your own sandbox.
const session = await aj.sessions.create({
  services: ["openai", "github"],
  scopes:   { github: ["/repos/my-org/*"] },
  ttlSecs:  600,
});
spawn("node", ["agent.js"], { env: { ...process.env, ...session.env } });
```

Surface area: `credentials`, `sessions`, `runs` (`create` / `fork` / `stream`),
`audit`. See [packages/sdk-node/README.md](packages/sdk-node/README.md) for
the full SDK reference.

## Web UI

Admin dashboard for the control plane: credentials, sessions, live event
stream, and a code playground. React 19 + Vite + Tailwind, served on
`http://localhost:3000`.

```bash
export AGENTJAIL_API_KEY=aj_local_$(openssl rand -hex 16)
docker compose -f docker-compose.platform.yml up --build
```

Pages: Overview · Sessions · Credentials · Stream · Playground.

## Limitations

- **Linux only** — Uses namespaces, seccomp, cgroups
- **Not a VM** — Kernel exploits could escape
- **GPU requires trust** — GPU passthrough exposes the NVIDIA kernel driver
- **Cgroups v2 only** — Won't work with cgroups v1
- **Allowlist proxy** — One veth pair per jail. Cleaned up automatically via `PR_SET_PDEATHSIG`; call `cleanup_stale_veths()` at startup for extra safety

For stronger isolation: [gVisor](https://gvisor.dev) or [Firecracker](https://firecracker-microvm.github.io).

## Requirements

- Linux kernel 5.13+ (Landlock optional)
- Rust 1.85+ (edition 2024)
- User namespace support
- `CAP_NET_ADMIN` or root (for `Network::Allowlist` veth setup)

## Development

```bash
docker compose run --rm dev cargo test
( cd packages/sdk-node && npm test )
( cd web && npm run build )
```

### GPU Testing

Requires a machine with an NVIDIA GPU and [NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/install-guide.html):

```bash
docker compose run --rm gpu cargo test --test gpu_test -- --nocapture
```

## License

MIT
