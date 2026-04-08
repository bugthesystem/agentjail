//! # agentjail
//!
//! Minimal Linux sandbox for running untrusted code.
//!
//! ## Quick Start
//!
//! ```ignore
//! use agentjail::{Jail, JailConfig, preset_build};
//!
//! // Use a preset
//! let config = preset_build("/path/to/source", "/path/to/output");
//! let jail = Jail::new(config)?;
//! let output = jail.run("npm", &["run", "build"]).await?;
//!
//! // Or configure manually
//! let config = JailConfig {
//!     source: "/code".into(),
//!     output: "/artifacts".into(),
//!     memory_mb: 1024,
//!     timeout_secs: 60,
//!     ..Default::default()
//! };
//! let jail = Jail::new(config)?;
//! ```
//!
//! ## Security Layers
//!
//! The jail provides defense in depth:
//!
//! 1. **Namespaces** - Isolated process, mount, network, and user views
//! 2. **Seccomp** - Syscall filtering to block dangerous operations
//! 3. **Cgroups** - Resource limits (memory, CPU, PIDs)
//! 4. **Landlock** - Filesystem access control
//!
//! ## Output
//!
//! - **stdout/stderr**: Captured as streams, available during execution
//! - **Artifacts**: Written to the output directory via bind mount

mod cgroup;
mod config;
mod error;
mod landlock;
mod mount;
mod namespace;
mod pipe;
mod run;
mod seccomp;

// Public API
pub use config::{
    Access, JailConfig, Mount, Network, SeccompLevel, preset_agent, preset_build, preset_dev,
};
pub use error::{JailError, Result};
pub use run::{Jail, JailHandle, Output};
