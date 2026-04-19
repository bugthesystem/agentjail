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
use crate::snapshot::copy_dir_with;
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

/// `FICLONE` ioctl number — `_IOW(0x94, 9, int)`.
///
/// Creates a reflink (shared data blocks) between two files.
/// Encoding: `(1 << 30) | (4 << 16) | (0x94 << 8) | 9 = 0x4004_9409`.
/// Identical on x86-64 and aarch64.
const FICLONE: libc::c_ulong = 0x4004_9409;

/// Clone a directory tree using copy-on-write when possible.
///
/// For each regular file the function first tries `FICLONE`. If the ioctl
/// fails (unsupported filesystem, cross-device, etc.) it falls back to
/// `std::fs::copy`. Symlinks are skipped to prevent traversal attacks.
///
/// Reuses [`copy_dir_with`] from the snapshot module for the directory
/// walk — only the per-file copy strategy differs.
pub(crate) fn cow_clone(src: &Path, dst: &Path) -> Result<ForkInfo> {
    let start = Instant::now();

    if !src.exists() {
        return Err(JailError::PathNotFound(src.to_path_buf()));
    }

    fs::create_dir_all(dst).map_err(JailError::Io)?;

    let mut total_files: u64 = 0;
    let mut cow_files: u64 = 0;
    let mut total_bytes: u64 = 0;

    copy_dir_with(src, dst, &mut |src_path, dst_path| {
        let size = fs::metadata(src_path).map(|m| m.len()).unwrap_or(0);
        total_files += 1;
        total_bytes += size;

        if try_ficlone(src_path, dst_path).is_ok() {
            cow_files += 1;
        } else {
            fs::copy(src_path, dst_path).map_err(JailError::Io)?;
            // Preserve permissions (fs::copy preserves on most filesystems
            // but we explicitly set to be safe on overlayfs).
            if let Ok(meta) = fs::metadata(src_path) {
                let _ = fs::set_permissions(dst_path, meta.permissions());
            }
        }
        Ok(())
    })?;

    let clone_method = match (cow_files, total_files) {
        (_, 0) => CloneMethod::Copy,
        (cow, total) if cow == total => CloneMethod::Reflink,
        (0, _) => CloneMethod::Copy,
        _ => CloneMethod::Mixed,
    };

    Ok(ForkInfo {
        clone_duration: start.elapsed(),
        clone_method,
        files_cloned: total_files,
        files_cow: cow_files,
        bytes_cloned: total_bytes,
        was_frozen: false, // caller sets this after the fact
    })
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

