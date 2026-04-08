//! Error types for agentjail.

use std::io;

/// All errors that can occur during jail setup or execution.
#[derive(Debug, thiserror::Error)]
pub enum JailError {
    #[error("namespace setup failed: {0}")]
    Namespace(#[source] rustix::io::Errno),

    #[error("mount failed: {0}")]
    Mount(#[source] rustix::io::Errno),

    #[error("seccomp filter failed: {0}")]
    Seccomp(String),

    #[error("cgroup setup failed: {0}")]
    Cgroup(#[source] io::Error),

    #[error("landlock setup failed: {0}")]
    Landlock(#[source] landlock::RulesetError),

    #[error("failed to execute command: {0}")]
    Exec(#[source] io::Error),

    #[error("fork failed: {0}")]
    Fork(#[source] rustix::io::Errno),

    #[error("pipe creation failed: {0}")]
    Pipe(#[source] rustix::io::Errno),

    #[error("path does not exist: {0}")]
    PathNotFound(std::path::PathBuf),
}

pub type Result<T> = std::result::Result<T, JailError>;
