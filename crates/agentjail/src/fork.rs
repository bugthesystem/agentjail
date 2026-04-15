//! Live forking: clone a running jail's filesystem state using COW semantics.
//!
//! Uses the Linux `FICLONE` ioctl for instant copy-on-write clones on
//! supported filesystems (btrfs, xfs with reflink). Falls back to regular
//! file copy when COW isn't available.
//!
//! # How it works
//!
//! 1. The running jail's cgroup is briefly frozen (sub-millisecond) so the
//!    filesystem is in a consistent state.
//! 2. The output directory is cloned using `FICLONE` (reflink) per file.
//!    On COW-capable filesystems this is instant — no data is copied until
//!    either side writes, at which point only the changed blocks diverge.
//! 3. The original jail is immediately thawed.
//! 4. A new jail is spawned with the cloned output directory.
//!
//! The result is a full, independent copy of the jail's writable state
//! obtained in milliseconds without meaningfully pausing the original.
//!
//! # Example
//!
//! ```ignore
//! let jail = Jail::new(config)?;
//! let handle = jail.spawn("python", &["train.py"])?;
//!
//! // Fork the running jail — filesystem state is duplicated instantly.
//! let (fork_handle, info) = jail.live_fork(
//!     Some(&handle),          // freeze for consistent snapshot
//!     "/tmp/fork-output",
//!     "python", &["train.py"],
//! )?;
//!
//! println!("Forked in {:?} ({:?})", info.clone_duration, info.clone_method);
//! // Both handle and fork_handle now run independently.
//! ```

use crate::error::{JailError, Result};
use std::fs;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::{Duration, Instant};

/// Method used for filesystem cloning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloneMethod {
    /// Reflink COW clone via `FICLONE` ioctl. Instant — no extra disk
    /// space until writes diverge.
    Reflink,
    /// Regular file copy. Full disk usage immediately.
    Copy,
    /// Some files were reflinked, others fell back to regular copy.
    Mixed,
}

/// Metadata about a completed fork operation.
#[derive(Debug, Clone)]
pub struct ForkInfo {
    /// Wall-clock time for the filesystem clone.
    pub clone_duration: Duration,
    /// Clone strategy that was actually used.
    pub clone_method: CloneMethod,
    /// Total regular files cloned.
    pub files_cloned: u64,
    /// Files that used COW (reflink).
    pub files_cow: u64,
    /// Total logical bytes cloned.
    pub bytes_cloned: u64,
    /// Whether the source jail was frozen during the clone.
    pub was_frozen: bool,
}

// ---------------------------------------------------------------------------
// FICLONE ioctl
// ---------------------------------------------------------------------------

/// `FICLONE` ioctl number — `_IOW(0x94, 9, int)`.
///
/// Creates a reflink (shared data blocks) between two files.
/// Encoding: `(1 << 30) | (4 << 16) | (0x94 << 8) | 9 = 0x4004_9409`.
/// Identical on x86-64 and aarch64.
const FICLONE: libc::c_ulong = 0x4004_9409;

// ---------------------------------------------------------------------------
// Public (crate) API
// ---------------------------------------------------------------------------

/// Clone a directory tree using copy-on-write when possible.
///
/// For each regular file the function first tries `FICLONE`. If the ioctl
/// fails (unsupported filesystem, cross-device, etc.) it falls back to
/// `std::fs::copy`. Symlinks are skipped to prevent traversal attacks.
pub(crate) fn cow_clone(src: &Path, dst: &Path) -> Result<ForkInfo> {
    let start = Instant::now();
    let mut stats = CloneStats::default();

    if !src.exists() {
        return Err(JailError::PathNotFound(src.to_path_buf()));
    }

    fs::create_dir_all(dst).map_err(JailError::Cgroup)?;
    clone_dir_recursive(src, dst, &mut stats)?;

    let clone_method = match (stats.cow_files, stats.total_files) {
        (_, 0) => CloneMethod::Copy,
        (cow, total) if cow == total => CloneMethod::Reflink,
        (0, _) => CloneMethod::Copy,
        _ => CloneMethod::Mixed,
    };

    Ok(ForkInfo {
        clone_duration: start.elapsed(),
        clone_method,
        files_cloned: stats.total_files,
        files_cow: stats.cow_files,
        bytes_cloned: stats.total_bytes,
        was_frozen: false, // caller sets this after the fact
    })
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

#[derive(Default)]
struct CloneStats {
    total_files: u64,
    cow_files: u64,
    total_bytes: u64,
}

/// Recursively clone a directory, trying `FICLONE` for every regular file.
fn clone_dir_recursive(src: &Path, dst: &Path, stats: &mut CloneStats) -> Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst).map_err(JailError::Cgroup)?;
    }

    for entry in fs::read_dir(src).map_err(JailError::Cgroup)? {
        let entry = entry.map_err(JailError::Cgroup)?;
        let ft = entry.file_type().map_err(JailError::Cgroup)?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        // Skip symlinks — following them could escape the snapshot scope.
        if ft.is_symlink() {
            continue;
        }

        if ft.is_dir() {
            clone_dir_recursive(&src_path, &dst_path, stats)?;
        } else {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            stats.total_files += 1;
            stats.total_bytes += size;

            if try_ficlone(&src_path, &dst_path).is_ok() {
                stats.cow_files += 1;
            } else {
                // Fallback: full copy (preserves permissions).
                fs::copy(&src_path, &dst_path).map_err(JailError::Cgroup)?;
            }
        }
    }

    Ok(())
}

/// Attempt a `FICLONE` ioctl to create a reflink copy of a single file.
///
/// On success the destination shares the source's data blocks (COW).
/// On failure the empty destination file is removed so the caller can
/// fall back to a regular copy.
fn try_ficlone(src: &Path, dst: &Path) -> std::io::Result<()> {
    let src_file = fs::File::open(src)?;
    let dst_file = fs::File::create(dst)?;

    // SAFETY: Both fds are valid, opened above. FICLONE is a safe ioctl
    // that creates a reflink — it cannot corrupt memory.
    let ret = unsafe { libc::ioctl(dst_file.as_raw_fd(), FICLONE, src_file.as_raw_fd()) };

    if ret == 0 {
        // Preserve source permissions (FICLONE only clones data blocks).
        if let Ok(meta) = fs::metadata(src) {
            let _ = fs::set_permissions(dst, meta.permissions());
        }
        Ok(())
    } else {
        // Remove the empty file so the caller can retry with fs::copy.
        let _ = fs::remove_file(dst);
        Err(std::io::Error::last_os_error())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_cow_clone_basic() {
        let src = PathBuf::from("/tmp/agentjail-cow-test-src");
        let dst = PathBuf::from("/tmp/agentjail-cow-test-dst");

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);

        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("file.txt"), "hello").unwrap();
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("sub/nested.txt"), "world").unwrap();

        let info = cow_clone(&src, &dst).unwrap();

        assert_eq!(info.files_cloned, 2);
        assert!(info.bytes_cloned > 0);
        assert_eq!(fs::read_to_string(dst.join("file.txt")).unwrap(), "hello");
        assert_eq!(
            fs::read_to_string(dst.join("sub/nested.txt")).unwrap(),
            "world"
        );

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn test_cow_clone_skips_symlinks() {
        let src = PathBuf::from("/tmp/agentjail-cow-symlink-src");
        let dst = PathBuf::from("/tmp/agentjail-cow-symlink-dst");
        let secret = PathBuf::from("/tmp/agentjail-cow-symlink-secret");

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
        let _ = fs::remove_file(&secret);

        fs::create_dir_all(&src).unwrap();
        fs::write(&secret, "SECRET").unwrap();
        fs::write(src.join("legit.txt"), "ok").unwrap();
        std::os::unix::fs::symlink(&secret, src.join("link")).unwrap();

        let info = cow_clone(&src, &dst).unwrap();

        assert_eq!(info.files_cloned, 1);
        assert!(dst.join("legit.txt").exists());
        assert!(!dst.join("link").exists());

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
        let _ = fs::remove_file(&secret);
    }

    #[test]
    fn test_cow_clone_empty_dir() {
        let src = PathBuf::from("/tmp/agentjail-cow-empty-src");
        let dst = PathBuf::from("/tmp/agentjail-cow-empty-dst");

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);

        fs::create_dir_all(&src).unwrap();

        let info = cow_clone(&src, &dst).unwrap();

        assert_eq!(info.files_cloned, 0);
        assert!(dst.exists());

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn test_cow_clone_missing_source() {
        let src = PathBuf::from("/tmp/agentjail-cow-nosrc");
        let dst = PathBuf::from("/tmp/agentjail-cow-nodst");
        let _ = fs::remove_dir_all(&src);

        assert!(cow_clone(&src, &dst).is_err());
    }
}
