//! Filesystem mounts for jail isolation.

use crate::config::Access;
use crate::error::{JailError, Result};
use rustix::mount::{MountFlags, MountPropagationFlags, mount, mount_remount};
use std::fs;
use std::path::Path;

/// Bind mount a source path to a destination inside the jail.
pub fn bind_mount(src: &Path, dst: &Path, access: Access) -> Result<()> {
    do_bind_mount(src, dst, access, true)
}

/// Bind mount a device node. Same as bind_mount but omits NODEV
/// so the device remains accessible.
pub fn bind_mount_dev(src: &Path, dst: &Path, access: Access) -> Result<()> {
    do_bind_mount(src, dst, access, false)
}

fn do_bind_mount(src: &Path, dst: &Path, access: Access, nodev: bool) -> Result<()> {
    if !src.exists() {
        return Ok(()); // Skip non-existent paths
    }

    if src.is_dir() {
        fs::create_dir_all(dst).map_err(JailError::Io)?;
    } else {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(JailError::Io)?;
        }
        fs::File::create(dst).map_err(JailError::Io)?;
    }

    mount(src, dst, "", MountFlags::BIND, "").map_err(JailError::Mount)?;

    let mut flags = MountFlags::BIND | MountFlags::NOSUID;
    if nodev {
        flags |= MountFlags::NODEV;
    }
    if access == Access::ReadOnly {
        flags |= MountFlags::RDONLY;
    }

    mount_remount(dst, flags, "").map_err(JailError::Mount)?;

    Ok(())
}

/// Mount a tmpfs at the given path.
pub fn mount_tmpfs(dst: &Path, size_mb: u64) -> Result<()> {
    fs::create_dir_all(dst).map_err(JailError::Io)?;

    let options = format!("size={size_mb}m,mode=1777");
    let flags = MountFlags::NOSUID | MountFlags::NODEV;

    mount("tmpfs", dst, "tmpfs", flags, &options).map_err(JailError::Mount)?;

    Ok(())
}

/// Mount a tmpfs with NOEXEC (for /tmp — prevents write+execute bypass).
pub fn mount_tmpfs_noexec(dst: &Path, size_mb: u64) -> Result<()> {
    fs::create_dir_all(dst).map_err(JailError::Io)?;

    let options = format!("size={size_mb}m,mode=1777");
    let flags = MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC;

    mount("tmpfs", dst, "tmpfs", flags, &options).map_err(JailError::Mount)?;

    Ok(())
}

/// Mount /proc inside the jail.
pub fn mount_proc(dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).map_err(JailError::Io)?;

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
    fs::create_dir_all(new_root).map_err(JailError::Io)?;

    // User directories
    let workspace = new_root.join("workspace");
    let output_dir = new_root.join("output");

    bind_mount(source, &workspace, Access::ReadOnly)?;
    bind_mount(output, &output_dir, Access::ReadWrite)?;

    // System directories (read-only). Warn on missing so operators notice
    // broken jails instead of getting cryptic "command not found" errors.
    let system_dirs = ["/bin", "/lib", "/lib64", "/usr"];
    for dir in &system_dirs {
        let src = Path::new(dir);
        if !src.exists() {
            eprintln!("warning: system directory {dir} does not exist, skipping mount");
            continue;
        }
        let dst = new_root.join(dir.trim_start_matches('/'));
        bind_mount(src, &dst, Access::ReadOnly)?;
    }

    // Mount a minimal /etc — only files needed for dynamic linking and DNS.
    // Full /etc would leak host secrets (shadow, ssh keys, machine-id).
    let etc_dst = new_root.join("etc");
    mount_tmpfs(&etc_dst, 1)?;
    let safe_etc_files = [
        "ld.so.cache",
        "ld.so.conf",
        "resolv.conf",
        "nsswitch.conf",
        "passwd",
        "group",
        "ssl",
        "alternatives",
    ];
    for name in &safe_etc_files {
        let src = Path::new("/etc").join(name);
        let dst = etc_dst.join(name);
        bind_mount(&src, &dst, Access::ReadOnly)?;
    }

    // Temp and proc — NOEXEC on /tmp to prevent write+execute bypass.
    mount_tmpfs_noexec(&new_root.join("tmp"), 100)?;
    mount_proc(&new_root.join("proc"))?;

    // Minimal /dev
    let dev = new_root.join("dev");
    fs::create_dir_all(&dev).map_err(JailError::Io)?;

    // Device nodes use bind_mount_dev (omits NODEV so the device works).
    // /dev/null needs write (programs redirect to it).
    // /dev/zero, /dev/urandom, /dev/random are read-only entropy/zero sources.
    let dev_rw = ["/dev/null"];
    let dev_ro = ["/dev/zero", "/dev/urandom", "/dev/random"];

    for node in &dev_rw {
        let src = Path::new(node);
        let dst = new_root.join(node.trim_start_matches('/'));
        bind_mount_dev(src, &dst, Access::ReadWrite)?;
    }
    for node in &dev_ro {
        let src = Path::new(node);
        let dst = new_root.join(node.trim_start_matches('/'));
        bind_mount_dev(src, &dst, Access::ReadOnly)?;
    }

    Ok(())
}
