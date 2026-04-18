//! Jail execution configuration.

/// Controls how the `/v1/sessions/:id/exec` and `/v1/runs` endpoints
/// configure jails. When `None` in `ControlPlaneConfig`, those endpoints
/// return 501.
#[derive(Debug, Clone)]
pub struct ExecConfig {
    /// Default memory limit in MB for exec calls (0 = unlimited).
    pub default_memory_mb: u64,
    /// Default timeout in seconds.
    pub default_timeout_secs: u64,
}

impl Default for ExecConfig {
    fn default() -> Self {
        Self {
            default_memory_mb: 0,
            default_timeout_secs: 300,
        }
    }
}
