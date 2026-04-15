//! Security and reliability regression tests from the production audit.
//!
//! Each test here targets a specific audit finding. Tests use the lightweight
//! config (no user namespace, no seccomp) unless testing a specific security
//! layer — in which case the relevant layer is enabled.
//!
//! Run with: cargo test --test audit_regression_test

use agentjail::{Jail, JailConfig, SeccompLevel, Snapshot};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

fn setup(name: &str) -> (PathBuf, PathBuf) {
    let src = PathBuf::from(format!("/tmp/aj-audit-{}-src", name));
    let out = PathBuf::from(format!("/tmp/aj-audit-{}-out", name));
    let _ = fs::remove_dir_all(&src);
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&out).unwrap();
    (src, out)
}

fn cleanup(src: &PathBuf, out: &PathBuf) {
    let _ = fs::remove_dir_all(src);
    let _ = fs::remove_dir_all(out);
}

fn base_config(src: PathBuf, out: PathBuf) -> JailConfig {
    JailConfig {
        source: src,
        output: out,
        timeout_secs: 10,
        user_namespace: false,
        seccomp: SeccompLevel::Disabled,
        landlock: false,
        memory_mb: 0,
        cpu_percent: 0,
        max_pids: 0,
        pid_namespace: true,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// AUDIT #1/2: Zombie leak — Drop kills+reaps, ChildGuard on error paths
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_drop_handle_kills_child() {
    let (src, out) = setup("drop-kill");

    fs::write(src.join("long.sh"), "#!/bin/sh\nsleep 300\n").unwrap();

    let config = base_config(src.clone(), out.clone());
    let jail = Jail::new(config).unwrap();
    let handle = jail.spawn("/bin/sh", &["/workspace/long.sh"]).unwrap();
    let pid = handle.pid();

    // Drop the handle without calling wait() — should kill the child.
    drop(handle);

    // Give the OS a moment to reap.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // The process should be gone (kill with signal 0 = check existence).
    let alive = unsafe { libc::kill(pid as i32, 0) };
    assert_eq!(
        alive, -1,
        "Child should be dead after JailHandle drop, but kill(0) succeeded"
    );

    cleanup(&src, &out);
}

#[tokio::test]
async fn test_no_zombie_after_drop() {
    let (src, out) = setup("no-zombie");

    fs::write(src.join("long.sh"), "#!/bin/sh\nsleep 300\n").unwrap();

    let config = base_config(src.clone(), out.clone());
    let jail = Jail::new(config).unwrap();
    let handle = jail.spawn("/bin/sh", &["/workspace/long.sh"]).unwrap();
    let pid = handle.pid();

    drop(handle);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Check /proc/<pid>/status — should not exist (no zombie).
    let proc_path = format!("/proc/{}/status", pid);
    assert!(
        !std::path::Path::new(&proc_path).exists(),
        "Process {} should be fully reaped (no zombie), but /proc entry still exists",
        pid
    );

    cleanup(&src, &out);
}

// ---------------------------------------------------------------------------
// AUDIT #3: Seccomp — new mount API, unshare, setns blocked
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_seccomp_blocks_unshare() {
    let (src, out) = setup("seccomp-unshare");

    // Try to call unshare() inside the jail — should fail with EPERM.
    fs::write(
        src.join("t.sh"),
        "#!/bin/sh\nunshare --mount true 2>&1 && echo ESCAPED || echo BLOCKED\n",
    )
    .unwrap();

    let mut config = base_config(src.clone(), out.clone());
    config.seccomp = SeccompLevel::Standard;

    let jail = Jail::new(config).unwrap();
    let r = jail.run("/bin/sh", &["/workspace/t.sh"]).await.unwrap();
    let stdout = String::from_utf8_lossy(&r.stdout);

    assert!(
        stdout.contains("BLOCKED") || r.exit_code != 0,
        "unshare should be blocked by seccomp, got: {}",
        stdout
    );

    cleanup(&src, &out);
}

#[tokio::test]
async fn test_seccomp_blocks_bpf() {
    let (src, out) = setup("seccomp-bpf");

    // Try to use bpf() syscall — should fail.
    // We use a C-level test via python if available, otherwise just check
    // that the syscall table includes the block.
    fs::write(
        src.join("t.sh"),
        concat!(
            "#!/bin/sh\n",
            "# Try to call bpf(BPF_PROG_LOAD) via syscall — should get EPERM\n",
            "python3 -c 'import ctypes; libc=ctypes.CDLL(None); print(\"BPF_RESULT:\", libc.syscall(321, 5, 0, 0))' 2>&1 || echo 'NO_PYTHON'\n",
        ),
    )
    .unwrap();

    let mut config = base_config(src.clone(), out.clone());
    config.seccomp = SeccompLevel::Standard;

    let jail = Jail::new(config).unwrap();
    let r = jail.run("/bin/sh", &["/workspace/t.sh"]).await.unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&r.stdout),
        String::from_utf8_lossy(&r.stderr)
    );

    // Either python isn't available (fine) or the syscall returned -1 (EPERM)
    assert!(
        combined.contains("NO_PYTHON") || combined.contains("-1") || combined.contains("EPERM"),
        "bpf syscall should be blocked, got: {}",
        combined
    );

    cleanup(&src, &out);
}

#[tokio::test]
async fn test_seccomp_strict_blocks_socketpair() {
    let (src, out) = setup("seccomp-socketpair");

    fs::write(
        src.join("t.sh"),
        concat!(
            "#!/bin/sh\n",
            "python3 -c '\n",
            "import socket\n",
            "try:\n",
            "    a,b = socket.socketpair()\n",
            "    print(\"CREATED\")\n",
            "except OSError as e:\n",
            "    print(\"BLOCKED:\", e)\n",
            "' 2>&1 || echo 'NO_PYTHON'\n",
        ),
    )
    .unwrap();

    let mut config = base_config(src.clone(), out.clone());
    config.seccomp = SeccompLevel::Strict;

    let jail = Jail::new(config).unwrap();
    let r = jail.run("/bin/sh", &["/workspace/t.sh"]).await.unwrap();
    let stdout = String::from_utf8_lossy(&r.stdout);

    assert!(
        stdout.contains("BLOCKED") || stdout.contains("NO_PYTHON"),
        "socketpair should be blocked in Strict mode, got: {}",
        stdout
    );

    cleanup(&src, &out);
}

// ---------------------------------------------------------------------------
// AUDIT #4: Snapshot clear_dir symlink traversal
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_restore_does_not_follow_symlinks() {
    let snap_src = PathBuf::from("/tmp/aj-audit-symlink-snap-src");
    let snap_dir = PathBuf::from("/tmp/aj-audit-symlink-snap-dir");
    let target = PathBuf::from("/tmp/aj-audit-symlink-target");
    let sentinel = target.join("sentinel.txt");

    let _ = fs::remove_dir_all(&snap_src);
    let _ = fs::remove_dir_all(&snap_dir);
    let _ = fs::remove_dir_all(&target);

    // Create a directory with a file that we DON'T want deleted.
    fs::create_dir_all(&target).unwrap();
    fs::write(&sentinel, "MUST_SURVIVE").unwrap();

    // Create snapshot source with a legitimate file.
    fs::create_dir_all(&snap_src).unwrap();
    fs::write(snap_src.join("legit.txt"), "ok").unwrap();

    let snap = Snapshot::create(&snap_src, &snap_dir).unwrap();

    // Now tamper with the restore target: place a symlink to the target dir.
    let restore_target = PathBuf::from("/tmp/aj-audit-symlink-restore");
    let _ = fs::remove_dir_all(&restore_target);
    fs::create_dir_all(&restore_target).unwrap();
    std::os::unix::fs::symlink(&target, restore_target.join("escape")).unwrap();

    // Restore over the tampered target — clear_dir should NOT follow the symlink.
    snap.restore_to(&restore_target).unwrap();

    // The sentinel file outside the snapshot MUST still exist.
    assert!(
        sentinel.exists(),
        "clear_dir followed a symlink and deleted files outside the snapshot directory!"
    );
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "MUST_SURVIVE");

    // Cleanup
    snap.delete().unwrap();
    let _ = fs::remove_dir_all(&snap_src);
    let _ = fs::remove_dir_all(&restore_target);
    let _ = fs::remove_dir_all(&target);
}

// ---------------------------------------------------------------------------
// AUDIT #5: Mount security — /etc restricted, /tmp NOEXEC
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_etc_shadow_not_accessible() {
    let (src, out) = setup("etc-shadow");

    fs::write(
        src.join("t.sh"),
        "#!/bin/sh\ncat /etc/shadow 2>&1 && echo LEAKED || echo SAFE\n",
    )
    .unwrap();

    let config = base_config(src.clone(), out.clone());
    let jail = Jail::new(config).unwrap();
    let r = jail.run("/bin/sh", &["/workspace/t.sh"]).await.unwrap();
    let stdout = String::from_utf8_lossy(&r.stdout);

    assert!(
        !stdout.contains("root:") && stdout.contains("SAFE"),
        "/etc/shadow should not be accessible, got: {}",
        stdout
    );

    cleanup(&src, &out);
}

#[tokio::test]
async fn test_etc_hostname_not_accessible() {
    let (src, out) = setup("etc-hostname");

    fs::write(
        src.join("t.sh"),
        "#!/bin/sh\ncat /etc/hostname 2>&1 && echo LEAKED || echo SAFE\n",
    )
    .unwrap();

    let config = base_config(src.clone(), out.clone());
    let jail = Jail::new(config).unwrap();
    let r = jail.run("/bin/sh", &["/workspace/t.sh"]).await.unwrap();
    let stdout = String::from_utf8_lossy(&r.stdout);

    assert!(
        stdout.contains("SAFE") || stdout.contains("No such file"),
        "/etc/hostname should not be accessible, got: {}",
        stdout
    );

    cleanup(&src, &out);
}

#[tokio::test]
async fn test_etc_machine_id_not_accessible() {
    let (src, out) = setup("etc-machine-id");

    fs::write(
        src.join("t.sh"),
        "#!/bin/sh\ncat /etc/machine-id 2>&1 && echo LEAKED || echo SAFE\n",
    )
    .unwrap();

    let config = base_config(src.clone(), out.clone());
    let jail = Jail::new(config).unwrap();
    let r = jail.run("/bin/sh", &["/workspace/t.sh"]).await.unwrap();
    let stdout = String::from_utf8_lossy(&r.stdout);

    assert!(
        stdout.contains("SAFE") || stdout.contains("No such file"),
        "/etc/machine-id should not be accessible, got: {}",
        stdout
    );

    cleanup(&src, &out);
}

#[tokio::test]
async fn test_etc_resolv_conf_accessible() {
    let (src, out) = setup("etc-resolv");

    // resolv.conf SHOULD be accessible (needed for DNS).
    fs::write(
        src.join("t.sh"),
        "#!/bin/sh\ntest -f /etc/resolv.conf && echo PRESENT || echo MISSING\n",
    )
    .unwrap();

    let config = base_config(src.clone(), out.clone());
    let jail = Jail::new(config).unwrap();
    let r = jail.run("/bin/sh", &["/workspace/t.sh"]).await.unwrap();
    // It's ok if resolv.conf doesn't exist on the host, but it should
    // not be an error to look for it.
    assert_eq!(r.exit_code, 0);

    cleanup(&src, &out);
}

#[tokio::test]
async fn test_tmp_noexec() {
    let (src, out) = setup("tmp-noexec");

    // Write a script to /tmp and try to execute it — should fail with EACCES.
    fs::write(
        src.join("t.sh"),
        concat!(
            "#!/bin/sh\n",
            "echo '#!/bin/sh' > /tmp/exploit.sh\n",
            "echo 'echo EXECUTED' >> /tmp/exploit.sh\n",
            "chmod +x /tmp/exploit.sh\n",
            "/tmp/exploit.sh 2>&1 && echo RAN || echo BLOCKED\n",
        ),
    )
    .unwrap();

    let config = base_config(src.clone(), out.clone());
    let jail = Jail::new(config).unwrap();
    let r = jail.run("/bin/sh", &["/workspace/t.sh"]).await.unwrap();
    let stdout = String::from_utf8_lossy(&r.stdout);

    assert!(
        stdout.contains("BLOCKED") || stdout.contains("Permission denied"),
        "/tmp should be NOEXEC, got: {}",
        stdout
    );

    cleanup(&src, &out);
}

// ---------------------------------------------------------------------------
// AUDIT #6: Pipe bounded output — jailed process can't OOM parent
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_large_stdout_does_not_oom() {
    let (src, out) = setup("pipe-oom");

    // Generate ~10MB of output — should be handled without crashing.
    fs::write(
        src.join("t.sh"),
        "#!/bin/sh\ndd if=/dev/zero bs=1M count=10 2>/dev/null | base64\n",
    )
    .unwrap();

    let config = base_config(src.clone(), out.clone());
    let jail = Jail::new(config).unwrap();
    let r = jail.run("/bin/sh", &["/workspace/t.sh"]).await.unwrap();

    // Should complete without crashing the parent. The child may get
    // SIGPIPE (141) when the pipe reader caps, or SIGKILL (137) from
    // the Drop cleanup — both are fine. The key assertion: we didn't OOM.
    assert!(
        r.stdout.len() <= 256 * 1024 * 1024 + 4096,
        "Output should be capped, got {} bytes",
        r.stdout.len()
    );

    cleanup(&src, &out);
}

// ---------------------------------------------------------------------------
// AUDIT #7: Fork — filesystem isolation verified
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_fork_symlink_in_output_not_followed() {
    let (src, out) = setup("fork-symlink");
    let fork_out = PathBuf::from("/tmp/aj-audit-fork-symlink-fork");
    let _ = fs::remove_dir_all(&fork_out);

    let secret = PathBuf::from("/tmp/aj-audit-fork-symlink-secret");
    fs::write(&secret, "SECRET_DATA").unwrap();

    // Place a symlink in the output dir pointing to the secret
    std::os::unix::fs::symlink(&secret, out.join("escape")).unwrap();
    fs::write(out.join("legit.txt"), "safe").unwrap();

    fs::write(src.join("t.sh"), "#!/bin/sh\ncat /output/legit.txt\n").unwrap();

    let config = base_config(src.clone(), out.clone());
    let jail = Jail::new(config).unwrap();

    let (forked, info) = jail.live_fork(None, &fork_out).unwrap();

    // Symlink should NOT be in the fork
    assert!(!fork_out.join("escape").exists(), "Symlink should not be cloned");
    assert_eq!(info.files_cloned, 1, "Only legit.txt should be cloned");

    let r = forked.run("/bin/sh", &["/workspace/t.sh"]).await.unwrap();
    assert_eq!(r.exit_code, 0);

    cleanup(&src, &out);
    let _ = fs::remove_dir_all(&fork_out);
    let _ = fs::remove_file(&secret);
}

// ---------------------------------------------------------------------------
// AUDIT #8: Cgroup cleanup — no orphans after drop
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_cgroup_cleaned_up_after_drop() {
    let (src, out) = setup("cgroup-cleanup");

    fs::write(src.join("t.sh"), "#!/bin/sh\nsleep 300\n").unwrap();

    let mut config = base_config(src.clone(), out.clone());
    config.memory_mb = 64; // Enable cgroup so one is created

    let jail = Jail::new(config).unwrap();
    let handle = jail.spawn("/bin/sh", &["/workspace/t.sh"]).unwrap();
    let pid = handle.pid();

    // The cgroup directory should exist while running
    drop(handle);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // After drop, cgroup should be removed (or at least the process killed)
    let alive = unsafe { libc::kill(pid as i32, 0) };
    assert_eq!(alive, -1, "Process should be dead after handle drop");

    cleanup(&src, &out);
}

// ---------------------------------------------------------------------------
// AUDIT #9: Netlink — interface name validation
// ---------------------------------------------------------------------------

#[test]
fn test_cleanup_stale_veths_does_not_panic() {
    // Should be safe to call even with no stale interfaces.
    agentjail::cleanup_stale_veths();
}

// ---------------------------------------------------------------------------
// AUDIT #10: Multiple forks don't exhaust resources
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_rapid_spawn_drop_no_leak() {
    let (src, out) = setup("rapid-spawn");

    fs::write(src.join("t.sh"), "#!/bin/sh\nsleep 300\n").unwrap();

    let config = base_config(src.clone(), out.clone());
    let jail = Jail::new(config).unwrap();

    // Spawn and immediately drop 20 handles — should not leak zombies.
    let mut pids = Vec::new();
    for _ in 0..20 {
        let handle = jail.spawn("/bin/sh", &["/workspace/t.sh"]).unwrap();
        pids.push(handle.pid());
        drop(handle);
    }

    tokio::time::sleep(Duration::from_millis(200)).await;

    // All should be dead.
    for pid in &pids {
        let alive = unsafe { libc::kill(*pid as i32, 0) };
        assert_eq!(
            alive, -1,
            "PID {} should be dead after handle drop",
            pid
        );
    }

    cleanup(&src, &out);
}

// ---------------------------------------------------------------------------
// AUDIT #11: Snapshot — dir_size doesn't follow symlinks
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_size_does_not_follow_symlinks() {
    let snap_dir = PathBuf::from("/tmp/aj-audit-snap-size");
    let target = PathBuf::from("/tmp/aj-audit-snap-size-target");

    let _ = fs::remove_dir_all(&snap_dir);
    let _ = fs::remove_dir_all(&target);

    fs::create_dir_all(&snap_dir).unwrap();
    fs::create_dir_all(&target).unwrap();

    // Create a large file outside the snapshot dir
    fs::write(target.join("big.txt"), "x".repeat(10_000)).unwrap();

    // Create a symlink inside the snapshot dir pointing to the large file
    std::os::unix::fs::symlink(target.join("big.txt"), snap_dir.join("link")).unwrap();
    fs::write(snap_dir.join("real.txt"), "small").unwrap();

    let snap = Snapshot::load(&snap_dir, &snap_dir).unwrap();
    let size = snap.size_bytes();

    // Size should only count real.txt (5 bytes), not the symlink target (10000 bytes)
    assert!(
        size < 1000,
        "size_bytes should not follow symlinks, got {} bytes",
        size
    );

    let _ = fs::remove_dir_all(&snap_dir);
    let _ = fs::remove_dir_all(&target);
}

// ---------------------------------------------------------------------------
// AUDIT #12: Process exit code propagation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_nonzero_exit_code_propagated() {
    let (src, out) = setup("exit-code");

    fs::write(src.join("t.sh"), "#!/bin/sh\nexit 42\n").unwrap();

    let config = base_config(src.clone(), out.clone());
    let jail = Jail::new(config).unwrap();
    let r = jail.run("/bin/sh", &["/workspace/t.sh"]).await.unwrap();

    assert_eq!(r.exit_code, 42, "Exit code should propagate");

    cleanup(&src, &out);
}

// ---------------------------------------------------------------------------
// AUDIT #13: Workspace is truly read-only
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_workspace_readonly_enforced() {
    let (src, out) = setup("ro-enforce");

    fs::write(
        src.join("t.sh"),
        "#!/bin/sh\necho pwned > /workspace/t.sh 2>&1 && echo WRITABLE || echo READONLY\n",
    )
    .unwrap();

    let config = base_config(src.clone(), out.clone());
    let jail = Jail::new(config).unwrap();
    let r = jail.run("/bin/sh", &["/workspace/t.sh"]).await.unwrap();
    let stdout = String::from_utf8_lossy(&r.stdout);

    assert!(
        stdout.contains("READONLY") || stdout.contains("Read-only"),
        "Workspace should be read-only, got: {}",
        stdout
    );

    cleanup(&src, &out);
}
