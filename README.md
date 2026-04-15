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
- **Syscall filtering** — Seccomp-BPF blocks dangerous operations
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
- HTTPS APIs (Claude, OpenAI, npm registry)
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

> **Warning:** GPU passthrough is experimental and not yet verified on
> real hardware. Use at your own risk. See [GPU Testing](#gpu-testing)
> for how to validate on a machine with an NVIDIA GPU.

NVIDIA GPU access for CUDA/PyTorch workloads:

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

Automatically discovers and mounts `/dev/nvidia*` devices and host
NVIDIA libraries. Sets `LD_LIBRARY_PATH` and `CUDA_VISIBLE_DEVICES`.

**Security note:** GPU passthrough gives the jailed process direct
`ioctl` access to the NVIDIA kernel driver. This is a large, closed-source
attack surface. Use for trusted workloads (your own training jobs), not
for fully adversarial code.

## Resource Monitoring

```rust
let handle = jail.spawn("npm", &["run", "build"])?;

// Live stats
if let Some(stats) = handle.stats() {
    println!("Memory: {} MB", stats.memory_peak_bytes / 1024 / 1024);
    println!("I/O: {} MB written", stats.io_write_bytes / 1024 / 1024);
}

// Final results
let output = handle.wait().await?;
if output.oom_killed {
    eprintln!("Build killed by OOM!");
}
```

## Snapshotting

Save and restore output directory state for faster rebuilds:

```rust
use agentjail::Snapshot;

// After successful build, save snapshot
let snap = Snapshot::create(&output_dir, &snapshot_dir)?;

// Later, restore before next build
snap.restore()?;

// Check size
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

The original jail is frozen for sub-millisecond (cgroup freezer) while the
output directory is cloned, then immediately resumed. On COW-capable
filesystems (btrfs, xfs with reflink) the clone uses `FICLONE` — data
blocks are shared and only diverge on write, so disk usage stays near zero
until the fork actually modifies files. Falls back to regular copy on
other filesystems.

Fork without freezing (best-effort snapshot) by passing `None`:

```rust
let (forked, _) = jail.live_fork(None, "/tmp/fork-output")?;
let result = forked.run("python", &["evaluate.py"]).await?;
```

Multiple forks from the same running jail work — each gets an independent
copy of the filesystem state.

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
2. **Chroot** — Minimal filesystem view
3. **Seccomp** — Syscall filtering
4. **Cgroups v2** — Resource limits
5. **Landlock** — Filesystem access control (Linux 5.13+)

### What Gets Blocked

| Attack | Protection |
|--------|------------|
| Read ~/.ssh, ~/.aws | Not mounted |
| Network exfiltration | Network namespace + allowlist proxy |
| Reverse shells | No network or allowlist only |
| Fork bombs | PID limit |
| Memory exhaustion | Memory limit + OOM detection |
| Disk thrashing | I/O bandwidth limits |
| Signal host processes | PID namespace |
| Syscall exploits | Seccomp blocklist |

## CLI

```bash
agentjail run -s ./src -o ./out npm run build
agentjail tui   # Dashboard
agentjail demo  # Demo mode
```

### TUI Controls

| Key | Action |
|-----|--------|
| `j`/`k` | Navigate |
| `Enter` | Details |
| `K` | Kill |
| `C` | Clear |
| `q` | Quit |

## Limitations

- **Linux only** — Uses namespaces, seccomp, cgroups
- **Not a VM** — Kernel exploits could escape
- **GPU requires trust** — GPU passthrough exposes the NVIDIA kernel driver attack surface
- **Cgroups v2 only** — Won't work with cgroups v1
- **Allowlist proxy** — Uses veth pairs (one per jail). Jailed processes die automatically if the parent is killed (`PR_SET_PDEATHSIG`), which destroys both veth ends. Call `cleanup_stale_veths()` at startup for extra safety.

For stronger isolation: [gVisor](https://gvisor.dev) or [Firecracker](https://firecracker-microvm.github.io).

## Requirements

- Linux kernel 5.13+ (Landlock optional)
- Rust 1.85+ (edition 2024)
- User namespace support
- `CAP_NET_ADMIN` or root (for `Network::Allowlist` veth setup)

## Development

```bash
docker compose run --rm dev cargo test
docker compose run --rm dev cargo build -p agentjail-cli
```

### GPU Testing

Requires a machine with an NVIDIA GPU and [NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/install-guide.html):

```bash
docker compose run --rm gpu cargo test --test gpu_test -- --nocapture
```

This runs real GPU tests: `nvidia-smi` inside the jail, CUDA device
query via `libcuda.so`, and per-GPU device filtering. Without a GPU,
these tests skip gracefully.

## License

MIT
