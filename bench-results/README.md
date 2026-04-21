# bench-results/

Baseline and after-state JSON artifacts from [`agentjail-bench`](../crates/agentjail-bench/).
One pair (`*-baseline.json` + `*-after.json`) per scenario, captured on the
same host under the same kernel and CPU budget so the numbers are
directly comparable.

Run inside Docker (`docker compose run --rm dev …`) so the host the
benchmarks see is a known Linux configuration — `/proc/self/mountinfo`,
cgroup-v2 delegation, and user-namespace permissions all show up in each
JSON's `env` block.

## Baseline

State of `main` at [c04f6fa](https://github.com/bugthesystem/agentjail/commit/c04f6fa),
kernel captured per-file under `env.kernel`.

## After

Phase 1 wins applied:
1. `pidfd_open` + `AsyncFd` for `wait_for_pid` — eliminates the 50 ms
   `tokio::time::sleep` floor.
2. `tokio::net::unix::pipe::Receiver` for stdout/stderr — epoll-driven
   reads instead of tokio's blocking thread pool.
3. `JailHandle::wait()` drains stdout/stderr in background tasks
   concurrently with the process wait — fixes a pre-existing deadlock
   on output > one pipe buffer (~64 KiB) and lets the pipe Receiver
   actually show up in latency numbers.

## Reproducing

```
docker compose run --rm dev bash -c "
  cargo build -p agentjail-bench --release &&
  ./target/release/agentjail-bench noop         --iters 100 --warmup 20 --json bench-results/noop-after.json &&
  ./target/release/agentjail-bench noop         --iters 200 --warmup 10 --concurrency 16 --json bench-results/noop-c16-after.json &&
  ./target/release/agentjail-bench stdout-heavy --iters 20  --warmup 3  --json bench-results/stdout-heavy-after.json
"
```

Full spec in [`../crates/agentjail-bench/README.md`](../crates/agentjail-bench/README.md).
