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
        fs::create_dir_all(snapshot_dir).map_err(JailError::Cgroup)?;

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
            fs::create_dir_all(target).map_err(JailError::Cgroup)?;
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
            fs::remove_dir_all(&self.path).map_err(JailError::Cgroup)?;
        }
        Ok(())
    }

    /// Get snapshot path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get snapshot size in bytes.
    pub fn size_bytes(&self) -> u64 {
        dir_size(&self.path).unwrap_or(0)
    }
}

/// Copy directory recursively.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst).map_err(JailError::Cgroup)?;
    }

    for entry in fs::read_dir(src).map_err(JailError::Cgroup)? {
        let entry = entry.map_err(JailError::Cgroup)?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path).map_err(JailError::Cgroup)?;
        }
    }

    Ok(())
}

/// Clear directory contents without removing the directory itself.
fn clear_dir(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(JailError::Cgroup)? {
        let entry = entry.map_err(JailError::Cgroup)?;
        let path = entry.path();

        if path.is_dir() {
            fs::remove_dir_all(&path).map_err(JailError::Cgroup)?;
        } else {
            fs::remove_file(&path).map_err(JailError::Cgroup)?;
        }
    }
    Ok(())
}

/// Calculate directory size recursively.
fn dir_size(path: &Path) -> std::io::Result<u64> {
    let mut size = 0;

    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                size += dir_size(&path)?;
            } else {
                size += entry.metadata()?.len();
            }
        }
    }

    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
}
