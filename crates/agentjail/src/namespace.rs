//! Linux namespace setup.
//!
//! Namespaces provide isolation:
//! - User: UID/GID mapping, rootless operation
//! - Mount: Private filesystem view
//! - PID: Isolated process tree
//! - Network: Isolated network stack
//! - IPC: Isolated shared memory/semaphores

use crate::error::{JailError, Result};
use rustix::process::Pid;
use rustix::thread::{UnshareFlags, unshare};
use std::fs;

/// Namespace configuration for a jail.
#[derive(Debug, Clone, Copy)]
pub struct NamespaceConfig {
    pub user: bool,
    pub mount: bool,
    pub pid: bool,
    pub network: bool,
    pub ipc: bool,
}

impl Default for NamespaceConfig {
    fn default() -> Self {
        Self {
            user: true,
            mount: true,
            pid: true,
            network: true,
            ipc: true,
        }
    }
}

/// Enter new namespaces based on config.
pub fn enter_namespaces(config: NamespaceConfig) -> Result<()> {
    let mut flags = UnshareFlags::empty();

    if config.user {
        flags |= UnshareFlags::NEWUSER;
    }
    if config.mount {
        flags |= UnshareFlags::NEWNS;
    }
    if config.pid {
        flags |= UnshareFlags::NEWPID;
    }
    if config.network {
        flags |= UnshareFlags::NEWNET;
    }
    if config.ipc {
        flags |= UnshareFlags::NEWIPC;
    }

    unshare(flags).map_err(JailError::Namespace)?;

    Ok(())
}

/// Write UID/GID mappings for user namespace.
///
/// Maps the current user to root (0) inside the namespace.
/// Must be called from parent process after child enters user namespace.
pub fn write_uid_gid_map(child_pid: Pid) -> Result<()> {
    let uid = rustix::process::getuid();
    let gid = rustix::process::getgid();

    // Deny setgroups first (required for unprivileged user namespaces)
    let setgroups_path = format!("/proc/{}/setgroups", child_pid.as_raw_nonzero());
    fs::write(&setgroups_path, "deny").map_err(JailError::Cgroup)?;

    // Map current UID to root (0) inside namespace
    let uid_map_path = format!("/proc/{}/uid_map", child_pid.as_raw_nonzero());
    let uid_map = format!("0 {} 1\n", uid.as_raw());
    fs::write(&uid_map_path, uid_map).map_err(JailError::Cgroup)?;

    // Map current GID to root (0) inside namespace
    let gid_map_path = format!("/proc/{}/gid_map", child_pid.as_raw_nonzero());
    let gid_map = format!("0 {} 1\n", gid.as_raw());
    fs::write(&gid_map_path, gid_map).map_err(JailError::Cgroup)?;

    Ok(())
}

/// Setup loopback interface in network namespace.
pub fn setup_loopback() -> Result<()> {
    std::process::Command::new("ip")
        .args(["link", "set", "lo", "up"])
        .output()
        .map_err(JailError::Exec)?;

    Ok(())
}
