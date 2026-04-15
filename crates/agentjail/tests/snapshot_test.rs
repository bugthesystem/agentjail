//! Tests for jail filesystem snapshotting.
//!
//! Run with: cargo test --test snapshot_test

use agentjail::Snapshot;
use std::fs;
use std::path::PathBuf;

#[test]
fn test_snapshot_create_restore() {
    let output = PathBuf::from("/tmp/snapshot-test-output");
    let snapshot = PathBuf::from("/tmp/snapshot-test-snap");

    // Setup
    let _ = fs::remove_dir_all(&output);
    let _ = fs::remove_dir_all(&snapshot);
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("file.txt"), "hello").unwrap();
    fs::create_dir_all(output.join("subdir")).unwrap();
    fs::write(output.join("subdir/nested.txt"), "world").unwrap();

    // Create snapshot
    let snap = Snapshot::create(&output, &snapshot).unwrap();
    assert!(snap.size_bytes() > 0);

    // Modify output
    fs::write(output.join("file.txt"), "modified").unwrap();
    fs::remove_file(output.join("subdir/nested.txt")).unwrap();

    // Restore
    snap.restore().unwrap();

    // Verify
    assert_eq!(fs::read_to_string(output.join("file.txt")).unwrap(), "hello");
    assert_eq!(
        fs::read_to_string(output.join("subdir/nested.txt")).unwrap(),
        "world"
    );

    // Cleanup
    snap.delete().unwrap();
    let _ = fs::remove_dir_all(&output);
}

#[test]
fn test_snapshot_skips_symlinks() {
    let src = PathBuf::from("/tmp/snapshot-symlink-src");
    let dst = PathBuf::from("/tmp/snapshot-symlink-dst");
    let secret = PathBuf::from("/tmp/snapshot-symlink-secret");

    let _ = fs::remove_dir_all(&src);
    let _ = fs::remove_dir_all(&dst);
    let _ = fs::remove_dir_all(&secret);

    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dst).unwrap();

    // Create a secret file outside the snapshot dir
    fs::write(&secret, "TOP_SECRET_DATA").unwrap();

    // Create a regular file and a symlink pointing to the secret
    fs::write(src.join("legit.txt"), "normal").unwrap();
    std::os::unix::fs::symlink(&secret, src.join("sneaky_link")).unwrap();

    // Copy should skip the symlink
    let snap = Snapshot::create(&src, &dst).unwrap();

    // legit.txt must be copied
    assert_eq!(fs::read_to_string(dst.join("legit.txt")).unwrap(), "normal");
    // symlink must NOT be copied (not followed, not recreated)
    assert!(
        !dst.join("sneaky_link").exists(),
        "Symlink should not be copied into snapshot"
    );

    // Cleanup
    snap.delete().unwrap();
    let _ = fs::remove_dir_all(&src);
    let _ = fs::remove_file(&secret);
}
