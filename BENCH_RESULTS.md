# Performance — baseline vs current

> Reproduce: `docker compose run --rm dev bash`, then see the
> **Reproduce** block at the bottom. JSON artifacts live in
> [`bench-results/`](bench-results/) as `{scenario}-baseline.json`
> (pre-work reference) and `{scenario}-current.json` (as of `HEAD`).
> Every JSON embeds kernel / CPU / cgroup-v2 / userns / git-rev in its
> `env` block.

## Scoreboard

| Scenario                       | Metric      | Baseline                       | Current                | Delta     |
|--------------------------------|-------------|--------------------------------|------------------------|-----------|
| `noop`                         | p50 latency | **53 696 µs**                  | **951 µs**             | **−56×**  |
| `noop`                         | p99 latency | 61 530 µs                      | 1 580 µs               | −39×      |
| `noop`                         | throughput  | 18.2 /s                        | 896 /s                 | +49×      |
| `noop` (concurrency 16)        | throughput  | 278 /s                         | 1 805 /s               | +6.5×     |
| `noop-full` (seccomp + userns) | p50 latency | *(not meaningful — 50 ms floor)*| **1 230 µs**           | new floor |
| `noop-full` (concurrency 16)   | throughput  | *(~200/s from floor)*          | **1 662 /s**           | ~8×       |
| `stdout-heavy` (10 MiB)        | outcome     | **deadlock → SIGKILL @ 30 s**  | p50 **6 106 µs**, ~1.6–2.8 GB/s | **works → 2 GB/s** |
| `snapshot-repeat` (1 k × 16 KiB) | p50 latency | full hash per repeat: **26 848 µs** | mtime/size hint: **2 280 µs** | **−11.8×** |

`noop` = `/bin/true`, `SeccompLevel::Disabled`, `pid_namespace:true`.
`noop-full` = same plus `SeccompLevel::Standard` + user namespace
(where unprivileged). `stdout-heavy` pipes 10 MiB of null bytes through
the jail and drains via `jail.run()`.

Host: `6.17.8-orbstack` · 14 CPU · cgroup-v2 · userns available · overlayfs backing `/workspace`.

## What changed

### Spawn hot path

1. **`pidfd_open` + `AsyncFd` in `wait_for_pid`** ([run_internal.rs](crates/agentjail/src/run_internal.rs))
   Old code polled `waitpid(NOHANG)` every 50 ms; every `noop` sat for
   25 ms average / 50 ms worst case *after* the child was already a
   zombie. A pidfd becomes readable at the kernel's exit edge —
   tokio wakes synchronously. Graceful fallback to the 50 ms poll on
   kernels without `pidfd_open` (pre-5.3), probed once per process.

2. **`tokio::net::unix::pipe::Receiver` for stdout/stderr** ([pipe.rs](crates/agentjail/src/pipe.rs))
   Old code wrapped the pipe fd in `tokio::fs::File`, which routed
   every read through tokio's 512-slot blocking thread pool. New code
   registers the fd with tokio's reactor — O_NONBLOCK + epoll, no
   thread hop per read.

3. **`JailHandle::wait()` drains stdout/stderr concurrently** ([run.rs](crates/agentjail/src/run.rs))
   Old sequence read output *after* `wait_for_pid` returned; any child
   emitting more than one pipe buffer (~64 KiB) blocked on `write()`
   forever while the parent waited for an exit that couldn't happen.
   We SIGKILLed those jails at `timeout_secs`. Drain tasks now own the
   streams from the moment `wait()` is called.

4. **Pre-compiled seccomp BPF in `Jail::new`** ([seccomp.rs](crates/agentjail/src/seccomp.rs))
   `SeccompFilter::new(...).try_into()` used to run per spawn. The BPF
   program is identical across spawns of the same `Jail` — build once,
   apply the cached program in the child. Saves a BPF compile per
   seccomp-enabled spawn. Shows up at high concurrency and in
   `noop-full` throughput.

5. **`Arc<JailConfig>` / `Arc<NvidiaResources>` / `Arc<CompiledFilter>`** ([run.rs](crates/agentjail/src/run.rs))
   Old: `let config = self.config.clone()` per fork deep-cloned the
   env table and `Network::Allowlist(Vec<String>)`. New: `Arc::clone`
   is a refcount bump. Matters for long allowlists + large envs at
   high spawn rates.

6. **Batched cgroup `subtree_control` write** ([cgroup.rs](crates/agentjail/src/cgroup.rs))
   `+memory +cpu +pids +io` in a single write instead of four. Init
   path only, cached via `OnceLock`, but it's a real syscall cut.

7. **Removed `to_str().unwrap_or("/")` footgun** ([run.rs](crates/agentjail/src/run.rs))
   Non-UTF-8 output path used to silently clamp I/O bandwidth on the
   root device. Now refuses and logs.

8. **`Vec::with_capacity(64 KiB)` on `read_all`** ([pipe.rs](crates/agentjail/src/pipe.rs))
   Avoids the geometric grow-loop when the child emits moderate output.

### Snapshot + freeze

9. **`sha2/asm` feature** ([Cargo.toml](crates/agentjail/Cargo.toml))
   ARMv8 SHA extensions on aarch64 / SHA-NI on x86_64 Ice Lake+.
   ~5–10× for the incremental-snapshot hasher. One-line change.

10. **Cgroup freezer waits for quiescence** ([cgroup.rs](crates/agentjail/src/cgroup.rs), [snapshot.rs](crates/agentjail/src/snapshot.rs))
    `fs::write(cgroup.freeze, "1")` only *requests* freeze; tasks in
    uninterruptible syscalls keep running until they return. We now
    poll `cgroup.events` until the kernel reports `frozen 1`, with
    exponential backoff (100 µs → 1.6 ms) and a 50 ms deadline.
    Sub-ms in the common case, errors out rather than snapshotting a
    torn filesystem if a task genuinely can't freeze.

11. **Incremental snapshot mtime + size fast-path** ([snapshot.rs](crates/agentjail/src/snapshot.rs))
    `Snapshot::create_incremental_with_hint(…, prior)` takes the
    previous manifest. For each file, if `(relpath, size, mtime_ns)`
    matches *and* the prior blob still exists in the pool, we skip
    SHA-256 and the temp-file write entirely — one `stat` per file.
    Old manifests without `mtime_ns` fall back to full hashing. A 1 k
    file repeat-snapshot dropped from **26.8 ms** to **2.3 ms** (×11.8).
    At 100 k-file / 10 GB unchanged trees this is ~100 k stats (~1 s)
    instead of ~60 s of read+hash+tmp-write.

## Sub-millisecond spawn

`noop` p50 = **951 µs** with a real pid + mount + net + ipc namespace
jail around a real child process. Where the ~1 ms goes:

- `fork()` + setup + exec — kernel-bound, ~300 µs.
- Two `Pipe::new` + barrier pipe + cgroup skip — ~30 µs.
- Child: `unshare(NEWPID|NEWNS|NEWIPC)` + mount loop + chroot + exec
  `/bin/true` — ~500 µs dominant term.
- Parent: `pidfd_open` + `AsyncFd::readable().await` — ~50 µs from
  exit signal to wakeup.

This is not the floor. More work in [`PERF.md`](PERF.md).

## Regression status

Run inside Docker (`docker compose run --rm dev cargo test …`):

- `-p agentjail --lib` — **10/10**
- `-p agentjail --test integration --test snapshot_test --test fork_test` — **32/32**
- `-p agentjail --test audit_regression_test --test security_test --test seccomp_blocklist_test --test malicious_test` — **52/52**
- `-p agentjail --test inbound_reach_test` — **1/1**
- `-p agentjail-ctl --lib` — **25/25**
- `-p agentjail-bench` — **3/3** (unit tests on the harness)
- `-p agentjail-server --test wire` — 9/10. The one failure
  (`e2e_run_timeout`) **fails identically on pristine reference** —
  verified by stashing the agentjail hot-path files and re-running.
  Not caused by this work.

Total: **123/124**, zero new regressions.

## Reproduce

```bash
docker compose run --rm dev bash -c "
  cargo build -p agentjail-bench --release &&
  ./target/release/agentjail-bench noop            --iters 100 --warmup 20                  --json bench-results/noop-current.json           &&
  ./target/release/agentjail-bench noop            --iters 200 --warmup 10 --concurrency 16 --json bench-results/noop-c16-current.json       &&
  ./target/release/agentjail-bench noop-full       --iters 100 --warmup 20                  --json bench-results/noop-full-current.json      &&
  ./target/release/agentjail-bench noop-full       --iters 200 --warmup 10 --concurrency 16 --json bench-results/noop-full-c16-current.json  &&
  ./target/release/agentjail-bench stdout-heavy    --iters  20 --warmup  3                  --json bench-results/stdout-heavy-current.json   &&
  ./target/release/agentjail-bench snapshot-create --iters   5 --warmup  1 --tree-files 1000 --tree-size-kb 16 --json bench-results/snapshot-create-current.json &&
  ./target/release/agentjail-bench snapshot-repeat --iters   5 --warmup  1 --tree-files 1000 --tree-size-kb 16 --json bench-results/snapshot-repeat-current.json
"
```

Harness spec: [`crates/agentjail-bench/README.md`](crates/agentjail-bench/README.md).
