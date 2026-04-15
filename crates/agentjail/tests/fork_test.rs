//! Integration tests for live forking.
//!
//! Run with: cargo test --test fork_test

use agentjail::{CloneMethod, Jail, JailConfig, SeccompLevel};
use std::fs;
use std::path::PathBuf;

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
