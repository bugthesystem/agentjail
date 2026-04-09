//! Cgroups v2 resource limits.

use crate::error::{JailError, Result};
use std::fs;
use std::path::{Path, PathBuf};

const CGROUP_ROOT: &str = "/sys/fs/cgroup";

/// Handle to a cgroup for resource limiting.
pub struct Cgroup {
    path: PathBuf,
}

impl Cgroup {
    /// Create a new cgroup with the given name.
    ///
    /// The cgroup is created under the current user's cgroup (for rootless)
    /// or under the system cgroup root.
    pub fn create(name: &str) -> Result<Self> {
        let path = find_cgroup_base()?.join(format!("agentjail-{}", name));

        if !path.exists() {
            fs::create_dir(&path).map_err(JailError::Cgroup)?;
        }

        Ok(Self { path })
    }

    /// Set memory limit in bytes.
    pub fn set_memory_limit(&self, bytes: u64) -> Result<()> {
        fs::write(self.path.join("memory.max"), bytes.to_string()).map_err(JailError::Cgroup)
    }

    /// Set CPU quota as a percentage (100 = one full core).
    pub fn set_cpu_quota(&self, percent: u64) -> Result<()> {
        // cpu.max format: "quota period"
        // quota in microseconds, period typically 100000 (100ms)
        let period: u64 = 100_000;
        let quota = (period * percent) / 100;
        let value = format!("{} {}", quota, period);

        fs::write(self.path.join("cpu.max"), value).map_err(JailError::Cgroup)
    }

    /// Set maximum number of processes/threads.
    pub fn set_pids_max(&self, max: u64) -> Result<()> {
        fs::write(self.path.join("pids.max"), max.to_string()).map_err(JailError::Cgroup)
    }

    /// Add a process to this cgroup.
    pub fn add_pid(&self, pid: u32) -> Result<()> {
        fs::write(self.path.join("cgroup.procs"), pid.to_string()).map_err(JailError::Cgroup)
    }

    /// Get the cgroup path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read current memory usage in bytes.
    pub fn memory_usage(&self) -> Option<u64> {
        fs::read_to_string(self.path.join("memory.current"))
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// Read peak memory usage in bytes.
    pub fn memory_peak(&self) -> Option<u64> {
        fs::read_to_string(self.path.join("memory.peak"))
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// Read CPU usage in microseconds.
    pub fn cpu_usage_usec(&self) -> Option<u64> {
        let stat = fs::read_to_string(self.path.join("cpu.stat")).ok()?;
        for line in stat.lines() {
            if let Some(val) = line.strip_prefix("usage_usec ") {
                return val.trim().parse().ok();
            }
        }
        None
    }

    /// Read current number of processes.
    pub fn pids_current(&self) -> Option<u64> {
        fs::read_to_string(self.path.join("pids.current"))
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}

impl Drop for Cgroup {
    fn drop(&mut self) {
        // Try to remove the cgroup, ignore errors
        // (it may still have processes or not exist)
        let _ = fs::remove_dir(&self.path);
    }
}

/// Find the base cgroup path for the current user.
fn find_cgroup_base() -> Result<PathBuf> {
    // Read /proc/self/cgroup to find our cgroup
    let cgroup_info = fs::read_to_string("/proc/self/cgroup").map_err(JailError::Cgroup)?;

    // Format: "0::/path/to/cgroup"
    for line in cgroup_info.lines() {
        if let Some(path) = line.strip_prefix("0::") {
            return Ok(PathBuf::from(CGROUP_ROOT).join(path.trim_start_matches('/')));
        }
    }

    // Fallback to root cgroup
    Ok(PathBuf::from(CGROUP_ROOT))
}
