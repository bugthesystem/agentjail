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

- **Rootless by default** — No root required, uses user namespaces
- **Network isolation** — Block all network access or allow loopback only
- **Filesystem isolation** — Chroot with minimal system mounts
- **Resource limits** — Memory, CPU, and process limits via cgroups v2
- **Syscall filtering** — Seccomp-BPF blocks dangerous operations
- **Timeout handling** — Automatic cleanup of hung processes
- **Event streaming** — Real-time stdout/stderr for build server integration

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
    let config = preset_build("/path/to/source", "/path/to/output");
    let jail = Jail::new(config)?;
    let result = jail.run("npm", &["run", "build"]).await?;

    println!("Exit code: {}", result.exit_code);
    Ok(())
}
```

## Configuration

```rust
use agentjail::{Jail, JailConfig, Network, SeccompLevel};

let config = JailConfig {
    source: "/code".into(),           // Mounted read-only at /workspace
    output: "/artifacts".into(),      // Mounted read-write at /output
    network: Network::None,           // Or Network::Loopback
    seccomp: SeccompLevel::Standard,  // Or Strict, Disabled
    memory_mb: 512,
    cpu_percent: 100,                 // 100 = 1 core
    max_pids: 64,
    timeout_secs: 300,
    ..Default::default()
};

let jail = Jail::new(config)?;
```

## Presets

| Preset | Use Case | Network | Memory | Timeout |
|--------|----------|---------|--------|---------|
| `preset_build` | npm/cargo/bun builds | None | 1GB | 10 min |
| `preset_agent` | AI agent execution | None | 512MB | 5 min |
| `preset_dev` | Dev servers (HMR) | Loopback | 2GB | 1 hour |

## Resource Monitoring

Track memory and CPU usage per jail:

```rust
let handle = jail.spawn("npm", &["run", "build"])?;

// Live stats while running
if let Some(stats) = handle.stats() {
    println!("Memory: {} bytes", stats.memory_peak_bytes);
    println!("CPU: {} µs", stats.cpu_usage_usec);
}

// Final stats after completion
let output = handle.wait().await?;
if let Some(stats) = output.stats {
    println!("Peak memory: {} bytes", stats.memory_peak_bytes);
}
```

## Event Streaming

For build servers needing real-time output:

```rust
use agentjail::{Jail, JailEvent, preset_build};

let jail = Jail::new(preset_build("./src", "./out"))?;
let (handle, mut rx) = jail.spawn_with_events("npm", &["run", "build"])?;

while let Some(event) = rx.recv().await {
    match event {
        JailEvent::Stdout(line) => println!("{}", line),
        JailEvent::Stderr(line) => eprintln!("{}", line),
        JailEvent::Completed { exit_code, .. } => break,
        _ => {}
    }
}
```

## Security

### Layers

1. **Namespaces** — Isolated mount, network, IPC, PID, and user views
2. **Chroot** — Process sees minimal filesystem
3. **Seccomp** — Blocks dangerous syscalls (ptrace, mount, reboot, etc.)
4. **Cgroups v2** — Enforces resource limits
5. **Landlock** — Kernel-level filesystem access control (Linux 5.13+)

### What Gets Blocked

| Attack Vector | Protection |
|--------------|------------|
| Read `~/.ssh`, `~/.aws` | Not mounted in jail |
| Network exfiltration | Network namespace isolation |
| Reverse shells | No network + DNS resolution fails |
| Fork bombs | PID limit via cgroups |
| Memory exhaustion | Memory limit via cgroups |
| Escape via `/home`, `/var` | Not mounted |
| Syscall attacks | Seccomp blocklist |
| Signal host processes | PID namespace isolation |

## CLI

```bash
# Run a command in a jail
agentjail run -s ./src -o ./out npm run build

# TUI dashboard
agentjail tui

# Demo mode
agentjail demo
```

### TUI Controls

| Key | Action |
|-----|--------|
| `j`/`k` | Navigate |
| `Enter` | Details |
| `Esc` | Back |
| `K` | Kill |
| `C` | Clear completed |
| `q` | Quit |

## Limitations

- **Linux only** — Uses Linux-specific APIs (namespaces, seccomp, cgroups)
- **Not a VM** — Shares kernel with host; kernel exploits could escape
- **No GPU isolation** — GPU passthrough not supported
- **PID namespace overhead** — Uses double-fork pattern, slight process tree complexity
- **Cgroups v2 only** — Won't work on older systems with cgroups v1
- **Root in container** — Process runs as root inside jail (mapped to unprivileged user outside)

For higher isolation guarantees, consider [gVisor](https://gvisor.dev) or [Firecracker](https://firecracker-microvm.github.io).

## Requirements

- Linux kernel 5.13+ (for Landlock, optional)
- Rust 1.75+
- User namespace support for rootless mode

## Development

```bash
# Build and test (Docker required on macOS)
docker compose run --rm dev cargo test

# Build CLI
docker compose run --rm dev cargo build -p agentjail-cli
```

## License

MIT
