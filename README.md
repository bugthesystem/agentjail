<p align="center">
  <img src="logo.svg" width="100" height="100" alt="agentjail">
</p>

<h1 align="center">agentjail</h1>

<p align="center">
  Minimal Linux sandboxes for running untrusted code.
</p>

---

A Rust library plus optional control plane. One jail is one child
process inside a fresh set of Linux namespaces, pivot-rooted into a
minimal filesystem, seccomp-filtered, cgroup-limited, and optionally
walled behind an egress-proxy allowlist. No VM. No daemon. No setuid
helper.

> **Status — beta.** The core crate (`crates/agentjail`) is the
> load-bearing piece and is covered by a privileged test suite
> (`make test-rust-privileged`). The control plane, TypeScript/Python
> SDKs, web UI, and gateway are useful but not yet production-hardened.
> Pin a version, read the threat model, then depend on it.

## Isolation

- **Namespaces** — mount, network, IPC, PID; optionally user.
- **Filesystem** — `pivot_root` onto a bind-mounted, 128-bit-random
  temp root; old root is `umount2(MNT_DETACH)`-ed. Minimal `/bin`,
  `/lib`, `/usr` binds; tmpfs `/etc` with just what dynamic linking
  and DNS need. Landlock on Linux ≥ 5.13 (hard-fail if enabled on a
  kernel that lacks it).
- **Network** — `None`, `Loopback`, or `Allowlist(domains)`. Allowlist
  mode routes through an in-process HTTP CONNECT proxy that resolves
  the hostname once, rejects private/link-local/loopback/CGNAT IPs,
  and connects to the resolved address (not the hostname) to close
  DNS rebinding. Veth pair configured via netlink; no `ip` binary.
- **Syscalls** — seccomp-BPF blocklist (`Standard` / `Strict`).
  Blocks namespace, mount, module, keyring, BPF, perf, io_uring,
  `chroot`, `name_to_handle_at`, `ptrace`, `personality`, `clone3`,
  `mount_setattr`, `memfd_create`, `fanotify_init`, `quotactl`,
  `syslog`; argument-filters `ioctl(*, TIOCSTI, …)` and
  `socket(AF_NETLINK|AF_PACKET|AF_VSOCK, …)`.
- **Privileges** — `PR_SET_NO_NEW_PRIVS`, `close_range(3, ~0, CLOEXEC)`
  before exec, full bounding-set drop + `SECBIT_NOROOT_LOCKED |
  SECBIT_NO_SETUID_FIXUP_LOCKED` + `capset` zeroing every effective,
  permitted, and inheritable capability, in the grandchild after
  `/proc` is remounted in the new PID namespace.
- **Resources** — memory / CPU / PIDs / disk I/O via cgroup v2, gated
  by a barrier pipe: the child blocks until the parent has assigned
  the cgroup, so there is no unconstrained startup window.

## Requirements

- Linux ≥ 5.13, cgroup v2, user namespaces.
- Rust 1.85+ (edition 2024).
- `CAP_NET_ADMIN` — Allowlist mode only (veth + netlink).

## Use

```toml
[dependencies]
agentjail = "0.1"
tokio = { version = "1", features = ["rt", "macros"] }
```

```rust
use agentjail::{Jail, preset_build};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let jail = Jail::new(preset_build("./src", "./out"))?;
    let out  = jail.run("npm", &["run", "build"]).await?;
    println!("exit={} oom={}", out.exit_code, out.oom_killed);
    Ok(())
}
```

### Presets

| Preset | Network | Memory | Timeout |
|---|---|---|---|
| `preset_build`   | None       | 512 MB | 600 s |
| `preset_install` | Allowlist  | 512 MB | 600 s |
| `preset_agent`   | None       | 256 MB | 300 s |
| `preset_gpu`     | None       | 8 GB   | 3600 s |
| `preset_dev`     | Loopback   | 1 GB   | 3600 s |

`preset_install` requires explicit domains:

```rust
preset_install("./src", "./out", vec![
    "registry.npmjs.org".into(),
    "registry.yarnpkg.com".into(),
])
```

### Config

```rust
use agentjail::{Jail, JailConfig, Network, SeccompLevel};

let jail = Jail::new(JailConfig {
    source:       "/code".into(),       // read-only at /workspace
    output:       "/artifacts".into(),  // read-write at /output
    network:      Network::None,
    seccomp:      SeccompLevel::Standard,
    memory_mb:    512,
    cpu_percent:  100,                  // 100 = 1 core
    max_pids:     64,
    io_read_mbps: 100,
    io_write_mbps: 50,
    timeout_secs: 300,
    ..Default::default()
})?;
```

### Network

```rust
Network::Allowlist(vec![
    "api.anthropic.com".into(),
    "registry.npmjs.org".into(),
    "*.mcp.example.com".into(),
])
```

The proxy validates the hostname against the allowlist, resolves it
via DNS, filters every private/loopback/link-local/CGNAT/test-net
address, and connects to the remaining routable IP. TLS passes
through unchanged (HTTPS, SSE, WebSocket).

### GPU (experimental)

Exposes the NVIDIA kernel-driver attack surface. Trusted workloads
only.

```rust
JailConfig { gpu: GpuConfig { enabled: true, devices: vec![0] },
             ..Default::default() }
```

### Resource monitoring

```rust
let handle = jail.spawn("npm", &["run", "build"])?;
if let Some(s) = handle.stats() {
    println!("mem {} / peak {} MB  pids {}",
        s.memory_current_bytes / 1_048_576,
        s.memory_peak_bytes    / 1_048_576,
        s.pids_current);
}
let out = handle.wait().await?;
if out.oom_killed { eprintln!("OOM"); }
```

### Events

```rust
let (_handle, mut rx) = jail.spawn_with_events("npm", &["run", "build"])?;
while let Some(ev) = rx.recv().await {
    match ev {
        JailEvent::Stdout(l)        => println!("{l}"),
        JailEvent::Stderr(l)        => eprintln!("{l}"),
        JailEvent::OomKilled        => eprintln!("OOM"),
        JailEvent::Completed { .. } => break,
        _ => {}
    }
}
```

### Snapshots and live forks

```rust
let snap = Snapshot::create(&output, &snapshot_dir)?;
snap.restore()?;

// Clone a running jail's output without pausing it (reflink on btrfs/xfs,
// fallback to regular copy elsewhere; the jail is frozen sub-millisecond
// via the cgroup freezer for the clone's duration).
let handle = jail.spawn("python", &["train.py"])?;
let (forked, _info) = jail.live_fork(Some(&handle), "/tmp/fork-out")?;
```

Snapshots restored through the incremental pool strip `S_ISUID` /
`S_ISGID` bits and reject manifest entries with absolute or `..`
paths.

## Verified threat model

Each row links to the regression test that would fail if the
protection ever did. All tests live in
[`crates/agentjail/tests/`](crates/agentjail/tests/).

| Attack | Protection | Test |
|---|---|---|
| Read host `~/.ssh` / `~/.aws` | Not mounted | [`test_cannot_read_ssh_keys`](crates/agentjail/tests/security_test.rs) |
| Read `/etc/shadow`, machine-id | Minimal `/etc` | [`test_etc_shadow_not_accessible`](crates/agentjail/tests/audit_regression_test.rs) |
| Network exfiltration | Netns + allowlist proxy | [`test_network_none_blocks_external`](crates/agentjail/tests/security_test.rs), [`test_reverse_shell_blocked`](crates/agentjail/tests/security_test.rs) |
| Fork bomb | PID limit | [`test_pid_limit_blocks_fork_bomb`](crates/agentjail/tests/audit_regression_test.rs) |
| Memory blow-up | Memory limit + OOM detection | [`test_large_stdout_does_not_oom`](crates/agentjail/tests/audit_regression_test.rs) |
| Disk thrashing | I/O bandwidth limits | [`test_io_write_bandwidth_limit_enforced`](crates/agentjail/tests/audit_regression_test.rs) |
| Signal host processes | PID namespace | [`test_pid_namespace_full_sandbox`](crates/agentjail/tests/security_test.rs) |
| Mount manipulation | `mount` + new mount API blocked | [`seccomp_standard_blocks_documented_syscalls`](crates/agentjail/tests/seccomp_blocklist_test.rs) |
| `chroot` escape | `pivot_root` + detach; `chroot` seccomp-blocked | [`test_chroot_no_home`](crates/agentjail/tests/security_test.rs) |
| io_uring bypass | `io_uring_*` blocked | [`seccomp_standard_blocks_documented_syscalls`](crates/agentjail/tests/seccomp_blocklist_test.rs) |
| Compat-mode escape | `personality()` blocked | [`seccomp_standard_blocks_documented_syscalls`](crates/agentjail/tests/seccomp_blocklist_test.rs) |
| Namespace escape | `clone3`, `unshare`, `setns` blocked | [`test_seccomp_blocks_unshare`](crates/agentjail/tests/audit_regression_test.rs) |
| BPF / perf | `bpf`, `perf_event_open`, `userfaultfd` blocked | [`test_seccomp_blocks_bpf`](crates/agentjail/tests/audit_regression_test.rs) |
| Executable memory | `memfd_create` blocked | [`seccomp_standard_blocks_documented_syscalls`](crates/agentjail/tests/seccomp_blocklist_test.rs) |
| Write + exec on `/tmp` | `NOEXEC` | [`test_tmp_noexec`](crates/agentjail/tests/audit_regression_test.rs) |
| Setuid escalation | `PR_SET_NO_NEW_PRIVS` | — |
| Core-dump leak | `RLIMIT_CORE=0` | [`test_rlimit_core_disabled`](crates/agentjail/tests/audit_regression_test.rs) |
| Parent stdout OOM | Output capped at 256 MiB per stream | [`test_large_stdout_does_not_oom`](crates/agentjail/tests/audit_regression_test.rs) |
| FD exhaustion | `RLIMIT_NOFILE` at 4096 | [`test_fd_limit_enforced`](crates/agentjail/tests/audit_regression_test.rs) |
| Symlink traversal | Skipped in snapshots, forks, cleanup | [`test_snapshot_restore_does_not_follow_symlinks`](crates/agentjail/tests/audit_regression_test.rs) |
| Zombie / fd leak | `PR_SET_PDEATHSIG` + Drop kills+reaps | [`test_no_zombie_after_drop`](crates/agentjail/tests/audit_regression_test.rs) |

## Limits

- Linux-only. Not a VM; a kernel exploit escapes. For stronger
  isolation pair with [gVisor](https://gvisor.dev) or run inside a
  [Firecracker](https://firecracker-microvm.github.io) microVM.
- GPU mode widens the attack surface to the NVIDIA driver.
- Allowlist mode costs one veth pair per concurrent jail; stale
  interfaces are reaped at `agentjail-server` startup via
  `cleanup_stale_veths()`.

## Control plane

An optional HTTP server (`agentjail-server`) sits in front of the
library: phantom-token credential broker, jail/workspace/snapshot
ledgers in Postgres, an SSE stream of upstream API calls, and a web
UI. Installed pre-release; APIs may move. Useful for local dev,
demos, and staging.

**Surface:**

- `POST /v1/credentials` · `POST /v1/sessions` · `POST /v1/runs` (`fork`, `stream`)
- `POST /v1/workspaces` (`/fork`, `/exec`) · `POST /v1/workspaces/:id/snapshot`
- `GET /v1/workspaces?q=…` · `GET /v1/snapshots?q=…`
- `GET /v1/snapshots/:id/manifest` — file listing for pool-backed snapshots
- `GET /v1/jails/:id` — result + the exact `JailConfig` the jail ran with
- `GET /v1/audit` — upstream-proxy audit log
- `GET /v1/config` — read-only snapshot of server settings

### Web UI

![control plane](media/control-plane.png)

React 19 + Vite + Tailwind. Task-first pages (Dashboard, Projects,
API Sessions, Integrations, Playground, Docs) with operator views
(Execution Ledger, Snapshots, API Audit, System Settings) tucked
behind an `Advanced` menu.

```bash
export AGENTJAIL_API_KEY=aj_local_$(openssl rand -hex 16)
docker compose -f docker-compose.platform.yml up --build
# UI:  http://localhost:3000
# API: http://localhost:7000
```

### TypeScript SDK

`@agentjail/sdk` — zero deps, Node ≥ 18. Sandboxes never see real
API keys: they get phantom tokens (`phm_<hex>`) plus `*_BASE_URL`
env vars pointing at the proxy.

```ts
import { Agentjail } from "@agentjail/sdk";

const aj = new Agentjail({
  baseUrl: "http://localhost:7000",
  apiKey:  process.env.AGENTJAIL_API_KEY!,
});

await aj.credentials.put({ service: "openai", secret: process.env.OPENAI_API_KEY! });

const result = await aj.runs.create({ code: "print('hi')", language: "python" });

for await (const ev of aj.runs.stream({ code, language: "python" })) {
  if (ev.type === "stdout") process.stdout.write(ev.line + "\n");
}

const session = await aj.sessions.create({
  services: ["openai", "github"],
  scopes:   { github: ["/repos/my-org/*"] },
  ttlSecs:  600,
});
spawn("node", ["agent.js"], { env: { ...process.env, ...session.env } });
```

Surface: `credentials`, `sessions`, `runs` (`create` / `fork` /
`stream`), `workspaces`, `snapshots`, `jails`, `audit`. Reference:
[`packages/sdk-node/README.md`](packages/sdk-node/README.md).

### Python SDK

`agentjail` — Python ≥ 3.10, depends on `httpx`. Symmetrical with
the Node SDK; see [`packages/sdk-python/README.md`](packages/sdk-python/README.md).

## Build and test

```bash
make test-rust              # low-privilege unit slice (in Docker)
make test-rust-privileged   # full security suite, --privileged Docker
( cd packages/sdk-node    && npm test )
( cd packages/sdk-python  && pytest )
( cd web && npm run build )
```

GPU tests need an NVIDIA GPU + the
[Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/install-guide.html):

```bash
docker compose run --rm gpu cargo test --test gpu_test -- --nocapture
```

## License

MIT. See [LICENSE](LICENSE).
