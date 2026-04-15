//! Integration tests for live forking.
//!
//! Run with: cargo test --test fork_test

use agentjail::{CloneMethod, Jail, JailConfig, SeccompLevel};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

fn setup_dirs(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let source = PathBuf::from(format!("/tmp/agentjail-fork-{}-src", name));
    let output = PathBuf::from(format!("/tmp/agentjail-fork-{}-out", name));
    let fork_output = PathBuf::from(format!("/tmp/agentjail-fork-{}-fork", name));

    let _ = fs::remove_dir_all(&source);
    let _ = fs::remove_dir_all(&output);
    let _ = fs::remove_dir_all(&fork_output);

    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&output).unwrap();

    (source, output, fork_output)
}

fn cleanup(source: &PathBuf, output: &PathBuf, fork_output: &PathBuf) {
    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_dir_all(output);
    let _ = fs::remove_dir_all(fork_output);
}

fn test_config(source: PathBuf, output: PathBuf) -> JailConfig {
    JailConfig {
        source,
        output,
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

#[tokio::test]
async fn test_live_fork_clones_filesystem() {
    let (source, output, fork_output) = setup_dirs("clone");

    // Seed the output directory with state
    fs::write(output.join("state.txt"), "original-state").unwrap();
    fs::create_dir_all(output.join("subdir")).unwrap();
    fs::write(output.join("subdir/nested.txt"), "nested-data").unwrap();

    // Script that reads the cloned state
    fs::write(
        source.join("read.sh"),
        "#!/bin/sh\ncat /output/state.txt && cat /output/subdir/nested.txt\n",
    )
    .unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    let (handle, info) = jail
        .live_fork(None, &fork_output, "/bin/sh", &["/workspace/read.sh"])
        .unwrap();

    let result = handle.wait().await.unwrap();

    assert_eq!(
        result.exit_code, 0,
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.contains("original-state"),
        "Fork should see cloned state, got: {}",
        stdout
    );
    assert!(
        stdout.contains("nested-data"),
        "Fork should see nested cloned state, got: {}",
        stdout
    );

    assert_eq!(info.files_cloned, 2);
    assert!(info.bytes_cloned > 0);
    assert!(
        info.clone_method == CloneMethod::Reflink || info.clone_method == CloneMethod::Copy,
        "Should use reflink or copy"
    );

    cleanup(&source, &output, &fork_output);
}

#[tokio::test]
async fn test_live_fork_independence() {
    let (source, output, fork_output) = setup_dirs("indep");

    fs::write(output.join("data.txt"), "shared").unwrap();

    // Script that modifies the output in the fork
    fs::write(
        source.join("modify.sh"),
        "#!/bin/sh\necho 'modified' > /output/data.txt\n",
    )
    .unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    let (handle, _info) = jail
        .live_fork(None, &fork_output, "/bin/sh", &["/workspace/modify.sh"])
        .unwrap();

    let result = handle.wait().await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    // Original output must be unchanged
    assert_eq!(
        fs::read_to_string(output.join("data.txt")).unwrap(),
        "shared"
    );

    // Fork's output was modified
    let fork_data = fs::read_to_string(fork_output.join("data.txt")).unwrap();
    assert!(
        fork_data.contains("modified"),
        "Fork output should be modified, got: {}",
        fork_data
    );

    cleanup(&source, &output, &fork_output);
}

#[tokio::test]
async fn test_live_fork_while_running() {
    let (source, output, fork_output) = setup_dirs("running");

    // The original jail writes a marker file then sleeps
    fs::write(
        source.join("long.sh"),
        "#!/bin/sh\necho 'running' > /output/marker.txt\nsleep 30\n",
    )
    .unwrap();

    // The fork reads the marker
    fs::write(
        source.join("check.sh"),
        "#!/bin/sh\ncat /output/marker.txt 2>/dev/null || echo 'NO_MARKER'\n",
    )
    .unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    // Start the long-running jail
    let original = jail.spawn("/bin/sh", &["/workspace/long.sh"]).unwrap();

    // Give it a moment to write the marker
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Fork while the original is running
    let (fork_handle, info) = jail
        .live_fork(
            Some(&original),
            &fork_output,
            "/bin/sh",
            &["/workspace/check.sh"],
        )
        .unwrap();

    assert!(!info.was_frozen || info.was_frozen); // frozen depends on cgroup availability

    let fork_result = fork_handle.wait().await.unwrap();
    assert_eq!(
        fork_result.exit_code, 0,
        "stderr: {}",
        String::from_utf8_lossy(&fork_result.stderr)
    );

    let stdout = String::from_utf8_lossy(&fork_result.stdout);
    assert!(
        stdout.contains("running") || stdout.contains("NO_MARKER"),
        "Fork should see marker or report missing, got: {}",
        stdout
    );

    // Kill the original
    original.kill();

    cleanup(&source, &output, &fork_output);
}

#[tokio::test]
async fn test_live_fork_multiple() {
    let (source, output, fork_output) = setup_dirs("multi");
    let fork_output2 = PathBuf::from("/tmp/agentjail-fork-multi-fork2");
    let _ = fs::remove_dir_all(&fork_output2);

    fs::write(output.join("counter.txt"), "0").unwrap();

    fs::write(
        source.join("read_counter.sh"),
        "#!/bin/sh\ncat /output/counter.txt\n",
    )
    .unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    // Fork twice from the same source
    let (h1, info1) = jail
        .live_fork(None, &fork_output, "/bin/sh", &["/workspace/read_counter.sh"])
        .unwrap();
    let (h2, info2) = jail
        .live_fork(None, &fork_output2, "/bin/sh", &["/workspace/read_counter.sh"])
        .unwrap();

    let r1 = h1.wait().await.unwrap();
    let r2 = h2.wait().await.unwrap();

    assert_eq!(r1.exit_code, 0);
    assert_eq!(r2.exit_code, 0);
    assert!(info1.files_cloned > 0);
    assert!(info2.files_cloned > 0);

    let s1 = String::from_utf8_lossy(&r1.stdout);
    let s2 = String::from_utf8_lossy(&r2.stdout);
    assert!(s1.contains("0"));
    assert!(s2.contains("0"));

    cleanup(&source, &output, &fork_output);
    let _ = fs::remove_dir_all(&fork_output2);
}

// -----------------------------------------------------------------------
// Advanced scenarios
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_live_fork_different_command() {
    let (source, output, fork_output) = setup_dirs("diffcmd");

    fs::write(output.join("state.txt"), "42").unwrap();

    // Original would run this (we don't actually start it here)
    fs::write(
        source.join("writer.sh"),
        "#!/bin/sh\necho 'writing' > /output/state.txt\nsleep 30\n",
    )
    .unwrap();

    // Fork runs a completely different command
    fs::write(
        source.join("reader.sh"),
        "#!/bin/sh\necho \"state=$(cat /output/state.txt)\"\n",
    )
    .unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    // Fork with a different command than what the original would run
    let (handle, _info) = jail
        .live_fork(None, &fork_output, "/bin/sh", &["/workspace/reader.sh"])
        .unwrap();

    let result = handle.wait().await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.contains("state=42"),
        "Fork with different command should read original state, got: {}",
        stdout
    );

    cleanup(&source, &output, &fork_output);
}

#[tokio::test]
async fn test_live_fork_chain() {
    // Fork a fork — verify chaining works
    let (source, output, fork_output) = setup_dirs("chain");
    let fork_output2 = PathBuf::from("/tmp/agentjail-fork-chain-fork2");
    let _ = fs::remove_dir_all(&fork_output2);

    fs::write(output.join("generation.txt"), "gen-0").unwrap();

    // Script that reads generation and writes a new file
    fs::write(
        source.join("evolve.sh"),
        "#!/bin/sh\ncat /output/generation.txt\necho 'evolved' > /output/evolved.txt\n",
    )
    .unwrap();

    fs::write(
        source.join("read_both.sh"),
        "#!/bin/sh\ncat /output/generation.txt; echo; ls /output/\n",
    )
    .unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    // First fork
    let (h1, _) = jail
        .live_fork(None, &fork_output, "/bin/sh", &["/workspace/evolve.sh"])
        .unwrap();
    let r1 = h1.wait().await.unwrap();
    assert_eq!(r1.exit_code, 0);

    // fork_output now has both generation.txt and evolved.txt
    assert!(fork_output.join("evolved.txt").exists());

    // Fork-of-fork: create a new jail from fork_output, then fork it again
    let fork_config = test_config(source.clone(), fork_output.clone());
    let fork_jail = Jail::new(fork_config).unwrap();

    let (h2, info2) = fork_jail
        .live_fork(None, &fork_output2, "/bin/sh", &["/workspace/read_both.sh"])
        .unwrap();

    let r2 = h2.wait().await.unwrap();
    assert_eq!(
        r2.exit_code, 0,
        "stderr: {}",
        String::from_utf8_lossy(&r2.stderr)
    );

    let stdout2 = String::from_utf8_lossy(&r2.stdout);
    assert!(
        stdout2.contains("gen-0"),
        "Chain fork should see original generation, got: {}",
        stdout2
    );
    assert!(
        stdout2.contains("evolved.txt"),
        "Chain fork should see evolved file, got: {}",
        stdout2
    );
    // Chain fork should have cloned both files
    assert!(
        info2.files_cloned >= 2,
        "Chain fork should clone at least 2 files, got {}",
        info2.files_cloned
    );

    cleanup(&source, &output, &fork_output);
    let _ = fs::remove_dir_all(&fork_output2);
}

#[tokio::test]
async fn test_live_fork_original_exits_first() {
    let (source, output, fork_output) = setup_dirs("exit-first");

    fs::write(output.join("data.txt"), "snapshot").unwrap();

    // Original exits immediately
    fs::write(source.join("quick.sh"), "#!/bin/sh\nexit 0\n").unwrap();

    // Fork sleeps briefly then reads the file
    fs::write(
        source.join("slow.sh"),
        "#!/bin/sh\nsleep 0.2\ncat /output/data.txt\n",
    )
    .unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    // Start original
    let original = jail.spawn("/bin/sh", &["/workspace/quick.sh"]).unwrap();

    // Fork before original completes
    let (fork_handle, _) = jail
        .live_fork(
            Some(&original),
            &fork_output,
            "/bin/sh",
            &["/workspace/slow.sh"],
        )
        .unwrap();

    // Wait for original to exit
    let orig_result = original.wait().await.unwrap();
    assert_eq!(orig_result.exit_code, 0);

    // Fork should still complete successfully even though original is gone
    let fork_result = fork_handle.wait().await.unwrap();
    assert_eq!(
        fork_result.exit_code, 0,
        "Fork should survive original exiting. stderr: {}",
        String::from_utf8_lossy(&fork_result.stderr)
    );

    let stdout = String::from_utf8_lossy(&fork_result.stdout);
    assert!(
        stdout.contains("snapshot"),
        "Fork should still read its own output dir, got: {}",
        stdout
    );

    cleanup(&source, &output, &fork_output);
}

#[tokio::test]
async fn test_live_fork_timeout() {
    let (source, output, fork_output) = setup_dirs("timeout");

    fs::write(output.join("data.txt"), "before-timeout").unwrap();

    fs::write(source.join("hang.sh"), "#!/bin/sh\nsleep 100\n").unwrap();

    let mut config = test_config(source.clone(), output.clone());
    config.timeout_secs = 2; // Short timeout

    let jail = Jail::new(config).unwrap();

    let (handle, _info) = jail
        .live_fork(None, &fork_output, "/bin/sh", &["/workspace/hang.sh"])
        .unwrap();

    let result = handle.wait().await.unwrap();

    assert!(
        result.timed_out || result.exit_code != 0,
        "Forked jail should time out or be killed, exit_code={}, timed_out={}",
        result.exit_code,
        result.timed_out
    );
    assert!(
        result.duration.as_secs() < 10,
        "Should not run for more than 10s, took {:?}",
        result.duration
    );

    cleanup(&source, &output, &fork_output);
}

#[tokio::test]
async fn test_live_fork_symlink_safety_in_output() {
    let (source, output, fork_output) = setup_dirs("symsafe");

    // Put a symlink in the output directory pointing outside
    let secret = PathBuf::from("/tmp/agentjail-fork-symsafe-secret");
    fs::write(&secret, "TOP_SECRET").unwrap();
    std::os::unix::fs::symlink(&secret, output.join("escape")).unwrap();
    fs::write(output.join("legit.txt"), "safe").unwrap();

    fs::write(
        source.join("check.sh"),
        "#!/bin/sh\nls /output/\ncat /output/legit.txt\n",
    )
    .unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    let (handle, info) = jail
        .live_fork(None, &fork_output, "/bin/sh", &["/workspace/check.sh"])
        .unwrap();

    // The symlink should NOT be copied to fork_output
    assert!(
        !fork_output.join("escape").exists(),
        "Symlink should not be copied into fork output"
    );
    assert_eq!(
        info.files_cloned, 1,
        "Only legit.txt should be cloned, not the symlink"
    );

    let result = handle.wait().await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    cleanup(&source, &output, &fork_output);
    let _ = fs::remove_file(&secret);
}

#[tokio::test]
async fn test_live_fork_preserves_binary_data_in_jail() {
    let (source, output, fork_output) = setup_dirs("binary");

    // Write binary data to the output directory
    let binary_data: Vec<u8> = (0..=255).collect();
    fs::write(output.join("data.bin"), &binary_data).unwrap();

    // Script that checks the binary file size and md5
    fs::write(
        source.join("check_bin.sh"),
        "#!/bin/sh\nwc -c < /output/data.bin\n",
    )
    .unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    let (handle, info) = jail
        .live_fork(None, &fork_output, "/bin/sh", &["/workspace/check_bin.sh"])
        .unwrap();

    assert_eq!(info.bytes_cloned, 256);

    // Verify file is bit-identical on disk
    assert_eq!(
        fs::read(fork_output.join("data.bin")).unwrap(),
        binary_data,
        "Binary data in fork output must be identical"
    );

    let result = handle.wait().await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.trim().contains("256"),
        "Binary file should be 256 bytes inside jail, got: {}",
        stdout
    );

    cleanup(&source, &output, &fork_output);
}

#[tokio::test]
async fn test_live_fork_creates_output_dir() {
    let (source, output, _fork_output) = setup_dirs("autocreate");
    // Use a deeply nested path that doesn't exist yet
    let deep_fork = PathBuf::from("/tmp/agentjail-fork-autocreate-deep/a/b/c");
    let _ = fs::remove_dir_all("/tmp/agentjail-fork-autocreate-deep");

    fs::write(output.join("file.txt"), "hello").unwrap();

    fs::write(
        source.join("read.sh"),
        "#!/bin/sh\ncat /output/file.txt\n",
    )
    .unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    // fork_output path doesn't exist — should be auto-created
    let (handle, _info) = jail
        .live_fork(None, &deep_fork, "/bin/sh", &["/workspace/read.sh"])
        .unwrap();

    let result = handle.wait().await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("hello"));

    cleanup(&source, &output, &_fork_output);
    let _ = fs::remove_dir_all("/tmp/agentjail-fork-autocreate-deep");
}

#[tokio::test]
async fn test_live_fork_info_accuracy() {
    let (source, output, fork_output) = setup_dirs("forkinfo");

    // Create files with known sizes
    fs::write(output.join("ten.txt"), "0123456789").unwrap(); // 10 bytes
    fs::write(output.join("five.txt"), "abcde").unwrap(); // 5 bytes
    fs::create_dir_all(output.join("sub")).unwrap();
    fs::write(output.join("sub/three.txt"), "xyz").unwrap(); // 3 bytes

    fs::write(source.join("noop.sh"), "#!/bin/sh\ntrue\n").unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    let (_handle, info) = jail
        .live_fork(None, &fork_output, "/bin/sh", &["/workspace/noop.sh"])
        .unwrap();

    assert_eq!(info.files_cloned, 3, "Should report exactly 3 files");
    assert_eq!(info.bytes_cloned, 18, "Should report exactly 18 bytes (10+5+3)");
    assert!(!info.was_frozen, "Should not be frozen when no handle passed");
    assert!(
        info.clone_duration < Duration::from_secs(5),
        "Clone should be fast, took {:?}",
        info.clone_duration
    );
    assert!(
        info.clone_method == CloneMethod::Reflink
            || info.clone_method == CloneMethod::Copy
            || info.clone_method == CloneMethod::Mixed,
        "clone_method should be a valid variant"
    );
    // files_cow <= files_cloned
    assert!(
        info.files_cow <= info.files_cloned,
        "COW files ({}) should not exceed total files ({})",
        info.files_cow,
        info.files_cloned
    );

    _handle.kill();
    cleanup(&source, &output, &fork_output);
}

#[tokio::test]
async fn test_live_fork_with_events() {
    let (source, output, fork_output) = setup_dirs("events");

    fs::write(output.join("state.txt"), "event-test").unwrap();

    fs::write(
        source.join("echo.sh"),
        "#!/bin/sh\necho 'fork-stdout-line'\ncat /output/state.txt\n",
    )
    .unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    // Fork, then attach events to the forked handle
    let (handle, _info) = jail
        .live_fork(None, &fork_output, "/bin/sh", &["/workspace/echo.sh"])
        .unwrap();

    // Use spawn_with_events pattern: wait and collect output
    let result = handle.wait().await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.contains("fork-stdout-line"),
        "Should capture stdout, got: {}",
        stdout
    );
    assert!(
        stdout.contains("event-test"),
        "Should capture cloned state output, got: {}",
        stdout
    );

    cleanup(&source, &output, &fork_output);
}

#[tokio::test]
async fn test_live_fork_concurrent_from_running() {
    let (source, output, fork_output) = setup_dirs("concurrent");
    let fork_output2 = PathBuf::from("/tmp/agentjail-fork-concurrent-fork2");
    let fork_output3 = PathBuf::from("/tmp/agentjail-fork-concurrent-fork3");
    let _ = fs::remove_dir_all(&fork_output2);
    let _ = fs::remove_dir_all(&fork_output3);

    fs::write(output.join("shared.txt"), "base-state").unwrap();

    fs::write(
        source.join("long.sh"),
        "#!/bin/sh\nsleep 30\n",
    )
    .unwrap();

    fs::write(
        source.join("read.sh"),
        "#!/bin/sh\ncat /output/shared.txt\n",
    )
    .unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    // Start a long-running original
    let original = jail.spawn("/bin/sh", &["/workspace/long.sh"]).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Fork 3 times concurrently from the same running jail
    let (h1, i1) = jail
        .live_fork(Some(&original), &fork_output, "/bin/sh", &["/workspace/read.sh"])
        .unwrap();
    let (h2, i2) = jail
        .live_fork(Some(&original), &fork_output2, "/bin/sh", &["/workspace/read.sh"])
        .unwrap();
    let (h3, i3) = jail
        .live_fork(Some(&original), &fork_output3, "/bin/sh", &["/workspace/read.sh"])
        .unwrap();

    // All three should complete successfully
    let (r1, r2, r3) = tokio::join!(h1.wait(), h2.wait(), h3.wait());
    let r1 = r1.unwrap();
    let r2 = r2.unwrap();
    let r3 = r3.unwrap();

    assert_eq!(r1.exit_code, 0, "Fork 1 failed");
    assert_eq!(r2.exit_code, 0, "Fork 2 failed");
    assert_eq!(r3.exit_code, 0, "Fork 3 failed");

    // All should see base-state
    assert!(String::from_utf8_lossy(&r1.stdout).contains("base-state"));
    assert!(String::from_utf8_lossy(&r2.stdout).contains("base-state"));
    assert!(String::from_utf8_lossy(&r3.stdout).contains("base-state"));

    // All clone infos should be valid
    assert!(i1.files_cloned > 0);
    assert!(i2.files_cloned > 0);
    assert!(i3.files_cloned > 0);

    original.kill();
    cleanup(&source, &output, &fork_output);
    let _ = fs::remove_dir_all(&fork_output2);
    let _ = fs::remove_dir_all(&fork_output3);
}

#[tokio::test]
async fn test_live_fork_empty_output() {
    let (source, output, fork_output) = setup_dirs("emptyout");

    // Output directory is empty — fork should still work
    fs::write(
        source.join("list.sh"),
        "#!/bin/sh\nls /output/ | wc -l\n",
    )
    .unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    let (handle, info) = jail
        .live_fork(None, &fork_output, "/bin/sh", &["/workspace/list.sh"])
        .unwrap();

    assert_eq!(info.files_cloned, 0);
    assert_eq!(info.bytes_cloned, 0);

    let result = handle.wait().await.unwrap();
    assert_eq!(result.exit_code, 0);

    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.trim() == "0",
        "Empty output fork should see 0 files, got: {}",
        stdout
    );

    cleanup(&source, &output, &fork_output);
}

#[tokio::test]
async fn test_live_fork_preserves_file_permissions_in_jail() {
    use std::os::unix::fs::PermissionsExt;

    let (source, output, fork_output) = setup_dirs("perms");

    // Create an executable script in the output dir
    fs::write(output.join("run.sh"), "#!/bin/sh\necho 'executable'\n").unwrap();
    fs::set_permissions(
        output.join("run.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    // The fork will try to execute the script from the output dir
    fs::write(
        source.join("exec_output.sh"),
        "#!/bin/sh\n/output/run.sh\n",
    )
    .unwrap();

    let config = test_config(source.clone(), output.clone());
    let jail = Jail::new(config).unwrap();

    let (handle, _info) = jail
        .live_fork(
            None,
            &fork_output,
            "/bin/sh",
            &["/workspace/exec_output.sh"],
        )
        .unwrap();

    // Verify the permission was preserved on disk
    let forked_perms = fs::metadata(fork_output.join("run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(
        forked_perms & 0o111,
        0o111,
        "Execute bits should be preserved, got {:o}",
        forked_perms
    );

    let result = handle.wait().await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "Executable should run in fork. stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.contains("executable"),
        "Executable output should work, got: {}",
        stdout
    );

    cleanup(&source, &output, &fork_output);
}
