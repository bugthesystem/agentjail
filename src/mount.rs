//! Filesystem mounts for jail isolation.

use crate::config::Access;
use crate::error::{JailError, Result};
use rustix::mount::{MountFlags, MountPropagationFlags, mount, mount_remount};
use std::fs;
use std::path::Path;

/// Bind mount a source path to a destination inside the jail.
pub fn bind_mount(src: &Path, dst: &Path, access: Access) -> Result<()> {
    if !src.exists() {
        return Ok(()); // Skip non-existent paths
    }

    if src.is_dir() {
        fs::create_dir_all(dst).map_err(JailError::Cgroup)?;
    } else {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(JailError::Cgroup)?;
        }
        fs::File::create(dst).map_err(JailError::Cgroup)?;
    }

    mount(src, dst, "", MountFlags::BIND, "").map_err(JailError::Mount)?;

    let mut flags = MountFlags::BIND | MountFlags::NOSUID | MountFlags::NODEV;
    if access == Access::ReadOnly {
        flags |= MountFlags::RDONLY;
    }

    mount_remount(dst, flags, "").map_err(JailError::Mount)?;

    Ok(())
}

/// Mount a tmpfs at the given path.
pub fn mount_tmpfs(dst: &Path, size_mb: u64) -> Result<()> {
    fs::create_dir_all(dst).map_err(JailError::Cgroup)?;

    let options = format!("size={}m,mode=1777", size_mb);
    let flags = MountFlags::NOSUID | MountFlags::NODEV;

    mount("tmpfs", dst, "tmpfs", flags, &options).map_err(JailError::Mount)?;

    Ok(())
}

/// Mount /proc inside the jail.
pub fn mount_proc(dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).map_err(JailError::Cgroup)?;

    let flags = MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC;
    mount("proc", dst, "proc", flags, "").map_err(JailError::Mount)?;

    Ok(())
}

/// Make the root mount private to prevent mount propagation.
pub fn make_root_private() -> Result<()> {
    rustix::mount::mount_change(
        "/",
        MountPropagationFlags::REC | MountPropagationFlags::PRIVATE,
    )
    .map_err(JailError::Mount)?;

    Ok(())
}

/// Setup minimal root filesystem for the jail.
///
/// Creates:
/// - /workspace (source, read-only)
/// - /output (artifacts, read-write)
/// - /bin, /lib, /lib64, /usr (system binaries, read-only)
/// - /tmp (tmpfs)
/// - /proc
/// - /dev (minimal)
pub fn setup_root(new_root: &Path, source: &Path, output: &Path) -> Result<()> {
    fs::create_dir_all(new_root).map_err(JailError::Cgroup)?;

    // User directories
    let workspace = new_root.join("workspace");
    let output_dir = new_root.join("output");

    bind_mount(source, &workspace, Access::ReadOnly)?;
    bind_mount(output, &output_dir, Access::ReadWrite)?;

    // System directories (read-only)
    let system_dirs = ["/bin", "/lib", "/lib64", "/usr", "/etc"];
    for dir in &system_dirs {
        let src = Path::new(dir);
        let dst = new_root.join(dir.trim_start_matches('/'));
        bind_mount(src, &dst, Access::ReadOnly)?;
    }

    // Temp and proc
    mount_tmpfs(&new_root.join("tmp"), 100)?;
    mount_proc(&new_root.join("proc"))?;

    // Minimal /dev
    let dev = new_root.join("dev");
    fs::create_dir_all(&dev).map_err(JailError::Cgroup)?;

    // Create essential device nodes via bind mount
    let dev_nodes = ["/dev/null", "/dev/zero", "/dev/urandom", "/dev/random"];
    for node in &dev_nodes {
        let src = Path::new(node);
        let dst = new_root.join(node.trim_start_matches('/'));
        bind_mount(src, &dst, Access::ReadWrite)?;
    }

    Ok(())
}
