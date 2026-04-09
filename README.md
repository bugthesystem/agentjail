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
- **Network isolation** — Block all network or allow loopback only
- **Filesystem isolation** — Chroot with minimal system mounts
- **Resource limits** — Memory, CPU, PIDs, and disk I/O via cgroups v2
- **Syscall filtering** — Seccomp-BPF blocks dangerous operations
- **OOM detection** — Know when builds fail due to memory limits
- **Snapshotting** — Save/restore output directory for faster rebuilds
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
    network: Network::None,           // Or Network::Loopback
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

## Presets

| Preset | Use Case | Network | Memory | Timeout |
|--------|----------|---------|--------|---------|
| `preset_build` | npm/cargo/bun builds | None | 512MB | 10 min |
| `preset_agent` | AI agent execution | None | 256MB | 5 min |
| `preset_dev` | Dev servers (HMR) | Loopback | 1GB | 1 hour |

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
| Network exfiltration | Network namespace |
| Reverse shells | No network + no DNS |
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
- **No GPU** — GPU passthrough not supported
- **Cgroups v2 only** — Won't work with cgroups v1

For stronger isolation: [gVisor](https://gvisor.dev) or [Firecracker](https://firecracker-microvm.github.io).

## Requirements

- Linux kernel 5.13+ (Landlock optional)
- Rust 1.75+
- User namespace support

## Development

```bash
docker compose run --rm dev cargo test
docker compose run --rm dev cargo build -p agentjail-cli
```

## License

MIT
