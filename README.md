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

## Quick Start

```rust
use agentjail::{Jail, JailConfig, preset_build};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = preset_build("/path/to/source", "/path/to/output");

    let jail = Jail::new(config)?;
    let result = jail.run("npm", &["run", "build"]).await?;

    println!("Exit code: {}", result.exit_code);
    println!("Output: {}", String::from_utf8_lossy(&result.stdout));

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
| `preset_build` | npm/cargo/bun builds | None | 4GB | 10 min |
| `preset_agent` | AI agent execution | None | 1GB | 5 min |
| `preset_dev` | Dev servers (HMR) | Loopback | 4GB | 1 hour |

## Security Layers

1. **Namespaces** — Isolated mount, network, IPC, and user views
2. **Chroot** — Process sees minimal filesystem
3. **Seccomp** — Blocks dangerous syscalls (ptrace, mount, reboot, etc.)
4. **Cgroups v2** — Enforces resource limits
5. **Landlock** — Kernel-level filesystem access control (Linux 5.13+)

## What Gets Blocked

| Attack Vector | Protection |
|--------------|------------|
| Read `~/.ssh`, `~/.aws` | Not mounted in jail |
| Network exfiltration | Network namespace isolation |
| Reverse shells | No network + DNS resolution fails |
| Fork bombs | PID limit via cgroups |
| Memory exhaustion | Memory limit via cgroups |
| Escape via `/home`, `/var` | Not mounted |
| Syscall attacks | Seccomp blocklist |

## Requirements

- Linux kernel 5.13+ (for landlock support, optional)
- Rust 1.75+
- For rootless operation: user namespace support enabled

## Installation

```toml
[dependencies]
agentjail = "0.1"
tokio = { version = "1", features = ["rt", "macros"] }
```

## Development

```bash
# Build and test in Docker (required on macOS)
docker compose run --rm dev cargo test

# Run specific test
docker compose run --rm dev cargo test --test integration
```

## License

MIT
