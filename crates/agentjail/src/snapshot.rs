//! Jail filesystem snapshotting for faster repeated builds.
//!
//! Saves and restores the output directory state, similar to Docker layer caching.

use crate::error::{JailError, Result};
use std::fs;
use std::path::{Path, PathBuf};

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
