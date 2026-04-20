//! Jail filesystem snapshotting for faster repeated builds.
//!
//! Two storage modes:
//!
//! 1. **Full copy** (the original) — [`Snapshot::create`] walks the output
//!    dir and writes a plain duplicate under `snapshot_dir`. Simple,
//!    zero coupling to other snapshots.
//! 2. **Content-addressed incremental** —
//!    [`Snapshot::create_incremental`] writes each unique file body into
//!    a shared *object pool* keyed by SHA-256, plus a `manifest.json`
//!    under `snapshot_dir`. Later snapshots of the same bytes dedupe to
//!    zero extra disk. [`Snapshot::restore_incremental`] hardlinks (or
//!    falls back to copy) each blob back into a target dir.
//!
//! Both modes are complementary — the control plane picks one at write
//! time based on `AGENTJAIL_SNAPSHOT_POOL_DIR`.

use crate::error::{JailError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Freeze every process in a cgroup so its filesystem writes pause while
/// a snapshot is captured. Sub-millisecond under normal load. No-op path
/// for callers who already know they have no cgroup — just pass the
/// appropriate [`freeze_cgroup`] / [`thaw_cgroup`] calls yourself.
pub fn freeze_cgroup(cgroup_path: &Path) -> Result<()> {
    fs::write(cgroup_path.join("cgroup.freeze"), "1").map_err(JailError::Cgroup)
}

/// Unfreeze every process in a cgroup. Idempotent — thawing an already
/// thawed cgroup is safe.
pub fn thaw_cgroup(cgroup_path: &Path) -> Result<()> {
    fs::write(cgroup_path.join("cgroup.freeze"), "0").map_err(JailError::Cgroup)
}

/// Capture a [`Snapshot`] while optionally freezing the source cgroup so
/// the filesystem is quiescent for the duration of the copy.
///
/// - If `cgroup_path` is `Some`, the cgroup is frozen before the copy and
///   thawed afterwards — even if the copy errors.
/// - If `cgroup_path` is `None`, this behaves identically to
///   [`Snapshot::create`].
///
/// The freeze path is best-effort: if freeze fails (e.g. cgroup-v1 system,
/// missing `cgroup.freeze` file), we log a warning and proceed with a
/// plain copy. Snapshotting a hot filesystem is tolerated by the caller;
/// this helper exists to *improve* consistency, not to require it.
pub fn snapshot_frozen(
    cgroup_path: Option<&Path>,
    output_dir: &Path,
    snapshot_dir: &Path,
) -> Result<Snapshot> {
    let frozen = match cgroup_path {
        Some(p) => match freeze_cgroup(p) {
            Ok(()) => Some(p),
            Err(e) => {
                eprintln!(
                    "warning: freeze {} failed ({e}) — snapshotting hot filesystem",
                    p.display()
                );
                None
            }
        },
        None => None,
    };
    let out = Snapshot::create(output_dir, snapshot_dir);
    if let Some(p) = frozen {
        if let Err(e) = thaw_cgroup(p) {
            eprintln!(
                "ERROR: thaw {} failed ({e}) — processes remain frozen!",
                p.display()
            );
        }
    }
    out
}

/// A snapshot of a jail's output directory.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Path to the snapshot directory.
    path: PathBuf,
    /// Original output directory this was taken from.
    source: PathBuf,
}

impl Snapshot {
    /// Create a snapshot from an output directory.
    pub fn create(output_dir: &Path, snapshot_dir: &Path) -> Result<Self> {
        if !output_dir.exists() {
            return Err(JailError::PathNotFound(output_dir.to_path_buf()));
        }

        // Create snapshot directory
        fs::create_dir_all(snapshot_dir).map_err(JailError::Snapshot)?;

        // Copy contents
        copy_dir_recursive(output_dir, snapshot_dir)?;

        Ok(Self {
            path: snapshot_dir.to_path_buf(),
            source: output_dir.to_path_buf(),
        })
    }

    /// Restore snapshot to the original output directory.
    pub fn restore(&self) -> Result<()> {
        self.restore_to(&self.source)
    }

    /// Restore snapshot to a specific directory.
    pub fn restore_to(&self, target: &Path) -> Result<()> {
        if !self.path.exists() {
            return Err(JailError::PathNotFound(self.path.clone()));
        }

        // Clear target directory
        if target.exists() {
            clear_dir(target)?;
        } else {
            fs::create_dir_all(target).map_err(JailError::Snapshot)?;
        }

        // Copy snapshot contents
        copy_dir_recursive(&self.path, target)?;

        Ok(())
    }

    /// Load an existing snapshot from disk.
    pub fn load(snapshot_dir: &Path, original_source: &Path) -> Result<Self> {
        if !snapshot_dir.exists() {
            return Err(JailError::PathNotFound(snapshot_dir.to_path_buf()));
        }

        Ok(Self {
            path: snapshot_dir.to_path_buf(),
            source: original_source.to_path_buf(),
        })
    }

    /// Delete the snapshot.
    pub fn delete(self) -> Result<()> {
        if self.path.exists() {
            fs::remove_dir_all(&self.path).map_err(JailError::Snapshot)?;
        }
        Ok(())
    }

    /// Get snapshot path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get snapshot size in bytes.
    #[must_use]
    pub fn size_bytes(&self) -> u64 {
        dir_size(&self.path).unwrap_or(0)
    }
}

/// Copy directory recursively. Symlinks are skipped to prevent traversal attacks.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    copy_dir_with(src, dst, &mut |s, d| {
        fs::copy(s, d).map_err(JailError::Snapshot)?;
        Ok(())
    })
}

/// Walk a directory tree, skipping symlinks, and call `copy_file` for each
/// regular file. Shared by [`Snapshot`] (plain copy) and live forking
/// (COW copy via FICLONE).
pub(crate) fn copy_dir_with<F>(src: &Path, dst: &Path, copy_file: &mut F) -> Result<()>
where
    F: FnMut(&Path, &Path) -> Result<()>,
{
    if !dst.exists() {
        fs::create_dir_all(dst).map_err(JailError::Snapshot)?;
    }

    for entry in fs::read_dir(src).map_err(JailError::Snapshot)? {
        let entry = entry.map_err(JailError::Snapshot)?;
        let ft = entry.file_type().map_err(JailError::Snapshot)?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if ft.is_symlink() {
            // Skip symlinks — following them could escape the snapshot scope.
            continue;
        } else if ft.is_dir() {
            copy_dir_with(&src_path, &dst_path, copy_file)?;
        } else {
            copy_file(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

/// Clear directory contents without removing the directory itself.
/// Symlinks are removed (not followed) to prevent directory traversal attacks.
fn clear_dir(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(JailError::Snapshot)? {
        let entry = entry.map_err(JailError::Snapshot)?;
        let ft = entry.file_type().map_err(JailError::Snapshot)?;
        let path = entry.path();

        if ft.is_symlink() {
            // Remove the symlink itself — never follow it.
            fs::remove_file(&path).map_err(JailError::Snapshot)?;
        } else if ft.is_dir() {
            fs::remove_dir_all(&path).map_err(JailError::Snapshot)?;
        } else {
            fs::remove_file(&path).map_err(JailError::Snapshot)?;
        }
    }
    Ok(())
}

// ---------------- content-addressed incremental ----------------

/// One entry in an incremental manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Path relative to the snapshot's output root, using `/` separators.
    pub path: String,
    /// Unix mode bits. We preserve the executable bit and general perms
    /// on restore; symlinks and non-regular files are skipped at capture.
    pub mode: u32,
    /// Hex-encoded SHA-256 of the file's bytes.
    pub sha256: String,
    /// File size in bytes (cached so a restore can allocate).
    pub size: u64,
}

/// Manifest format written under `snapshot_dir/manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Format version. Bump when [`ManifestEntry`] changes.
    pub version: u32,
    /// Files, sorted by path for deterministic output.
    pub entries: Vec<ManifestEntry>,
}

impl Manifest {
    /// Total logical size of all referenced blobs.
    #[must_use]
    pub fn size_bytes(&self) -> u64 {
        self.entries.iter().map(|e| e.size).sum()
    }

    /// Hex-encoded blob hashes referenced by this manifest. Useful for GC.
    pub fn referenced_blobs(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.sha256.as_str())
    }
}

const MANIFEST_NAME: &str = "manifest.json";

impl Snapshot {
    /// Create an incremental snapshot: each regular file's bytes go into
    /// a content-addressed `objects_pool` (by sha256), and a manifest of
    /// `(path, sha, mode, size)` lives in `snapshot_dir`. Files whose
    /// hash is already in the pool are free.
    ///
    /// The pool layout is `{pool}/{hash[0..2]}/{hash}`. Pool writes are
    /// idempotent; stale writes from an interrupted previous snapshot
    /// are safely overwritten.
    pub fn create_incremental(
        output_dir: &Path,
        snapshot_dir: &Path,
        objects_pool: &Path,
    ) -> Result<Self> {
        if !output_dir.exists() {
            return Err(JailError::PathNotFound(output_dir.to_path_buf()));
        }
        fs::create_dir_all(snapshot_dir).map_err(JailError::Snapshot)?;
        fs::create_dir_all(objects_pool).map_err(JailError::Snapshot)?;

        let mut entries: Vec<ManifestEntry> = Vec::new();
        walk_and_hash(output_dir, output_dir, objects_pool, &mut entries)?;
        entries.sort_by(|a, b| a.path.cmp(&b.path));

        let manifest = Manifest {
            version: 1,
            entries,
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|e| JailError::Snapshot(std::io::Error::other(e)))?;
        fs::write(snapshot_dir.join(MANIFEST_NAME), manifest_bytes)
            .map_err(JailError::Snapshot)?;

        Ok(Self {
            path: snapshot_dir.to_path_buf(),
            source: output_dir.to_path_buf(),
        })
    }

    /// Restore a content-addressed snapshot into `target_dir`, using
    /// hardlinks from the object pool when possible (falls back to copy
    /// across filesystems or when the link limit is reached).
    pub fn restore_incremental(
        snapshot_dir: &Path,
        objects_pool: &Path,
        target_dir: &Path,
    ) -> Result<()> {
        let manifest_bytes = fs::read(snapshot_dir.join(MANIFEST_NAME))
            .map_err(JailError::Snapshot)?;
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| JailError::Snapshot(std::io::Error::other(e)))?;

        if target_dir.exists() {
            clear_dir(target_dir)?;
        } else {
            fs::create_dir_all(target_dir).map_err(JailError::Snapshot)?;
        }

        for entry in manifest.entries {
            let blob = blob_path(objects_pool, &entry.sha256);
            if !blob.exists() {
                return Err(JailError::Snapshot(std::io::Error::other(format!(
                    "blob {} missing from pool",
                    entry.sha256,
                ))));
            }
            let dst = target_dir.join(&entry.path);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(JailError::Snapshot)?;
            }
            if fs::hard_link(&blob, &dst).is_err() {
                // Hard links fail across filesystems or past the per-inode
                // limit; fall back to copy and replay mode bits.
                fs::copy(&blob, &dst).map_err(JailError::Snapshot)?;
            }
            set_unix_mode(&dst, entry.mode);
        }
        Ok(())
    }
}

/// Load and return a manifest from a snapshot dir created with
/// [`Snapshot::create_incremental`]. Errors when the file is missing —
/// useful for tools distinguishing full vs incremental snapshots.
pub fn load_manifest(snapshot_dir: &Path) -> Result<Manifest> {
    let bytes = fs::read(snapshot_dir.join(MANIFEST_NAME))
        .map_err(JailError::Snapshot)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| JailError::Snapshot(std::io::Error::other(e)))
}

fn blob_path(pool: &Path, sha: &str) -> PathBuf {
    let prefix = if sha.len() >= 2 { &sha[..2] } else { sha };
    pool.join(prefix).join(sha)
}

fn walk_and_hash(
    root: &Path,
    current: &Path,
    pool: &Path,
    out: &mut Vec<ManifestEntry>,
) -> Result<()> {
    for entry in fs::read_dir(current).map_err(JailError::Snapshot)? {
        let entry = entry.map_err(JailError::Snapshot)?;
        let ft = entry.file_type().map_err(JailError::Snapshot)?;
        let src = entry.path();

        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            walk_and_hash(root, &src, pool, out)?;
            continue;
        }
        if !ft.is_file() {
            continue;
        }

        // Normalise the relative path to forward-slashes so manifests
        // round-trip cross-platform.
        let rel = src.strip_prefix(root).unwrap_or(&src);
        let rel_str = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");

        let (sha, size) = hash_into_pool(&src, pool)?;
        out.push(ManifestEntry {
            path: rel_str,
            mode: get_unix_mode(&src),
            sha256: sha,
            size,
        });
    }
    Ok(())
}

/// Stream a file through SHA-256 into a temp file in the pool, then
/// atomically rename to the final blob path if it doesn't already exist.
fn hash_into_pool(src: &Path, pool: &Path) -> Result<(String, u64)> {
    let mut f = fs::File::open(src).map_err(JailError::Snapshot)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut size: u64 = 0;
    let tmp = pool.join(format!(".tmp-{}", uniq()));
    let mut tmp_writer = fs::File::create(&tmp).map_err(JailError::Snapshot)?;

    loop {
        let n = f.read(&mut buf).map_err(JailError::Snapshot)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        tmp_writer.write_all(&buf[..n]).map_err(JailError::Snapshot)?;
        size += n as u64;
    }
    tmp_writer.flush().map_err(JailError::Snapshot)?;
    drop(tmp_writer);

    let sha = hex::encode(hasher.finalize());
    let final_path = blob_path(pool, &sha);
    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent).map_err(JailError::Snapshot)?;
    }
    if final_path.exists() {
        // Already in pool — drop the temp and skip.
        let _ = fs::remove_file(&tmp);
    } else {
        // Rename within a filesystem is atomic. A concurrent writer for
        // the same hash may race us; last-write-wins is fine because the
        // contents are identical by construction.
        if let Err(e) = fs::rename(&tmp, &final_path) {
            let _ = fs::remove_file(&tmp);
            return Err(JailError::Snapshot(e));
        }
    }
    Ok((sha, size))
}

fn uniq() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}", std::process::id(), c)
}

#[cfg(unix)]
fn get_unix_mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p)
        .map(|m| m.permissions().mode())
        .unwrap_or(0o644)
}

#[cfg(not(unix))]
fn get_unix_mode(_p: &Path) -> u32 {
    0o644
}

#[cfg(unix)]
fn set_unix_mode(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(p, fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_unix_mode(_p: &Path, _mode: u32) {}

/// Garbage-collect unreferenced blobs from `objects_pool`.
///
/// `referenced` is the union of every manifest's `referenced_blobs()`.
/// Anything on disk under the pool that isn't in that set is removed.
/// Returns `(blobs_deleted, bytes_freed)`.
pub fn gc_objects_pool(
    objects_pool: &Path,
    referenced: &std::collections::HashSet<String>,
) -> Result<(usize, u64)> {
    let mut deleted = 0usize;
    let mut freed = 0u64;
    if !objects_pool.exists() {
        return Ok((0, 0));
    }
    for shard in fs::read_dir(objects_pool).map_err(JailError::Snapshot)? {
        let shard = shard.map_err(JailError::Snapshot)?;
        let shard_path = shard.path();
        if !shard_path.is_dir() {
            continue;
        }
        for blob in fs::read_dir(&shard_path).map_err(JailError::Snapshot)? {
            let blob = blob.map_err(JailError::Snapshot)?;
            let name = blob.file_name().to_string_lossy().into_owned();
            if name.starts_with(".tmp-") {
                // Orphaned tempfile from a crashed writer.
                let sz = blob.metadata().map(|m| m.len()).unwrap_or(0);
                let _ = fs::remove_file(blob.path());
                deleted += 1;
                freed += sz;
                continue;
            }
            if !referenced.contains(&name) {
                let sz = blob.metadata().map(|m| m.len()).unwrap_or(0);
                let _ = fs::remove_file(blob.path());
                deleted += 1;
                freed += sz;
            }
        }
        // Best-effort empty-shard cleanup.
        let _ = fs::remove_dir(&shard_path);
    }
    Ok((deleted, freed))
}

/// Calculate directory size recursively (does not follow symlinks).
fn dir_size(path: &Path) -> std::io::Result<u64> {
    let mut size = 0;

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let ft = entry.file_type()?;

        if ft.is_symlink() {
            continue;
        } else if ft.is_dir() {
            size += dir_size(&entry.path())?;
        } else {
            size += entry.metadata()?.len();
        }
    }

    Ok(size)
}
