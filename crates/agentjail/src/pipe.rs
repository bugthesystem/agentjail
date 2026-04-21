//! Pipe utilities for capturing stdout/stderr.
//!
//! The output side is a `tokio::net::unix::pipe::Receiver` — it sets
//! `O_NONBLOCK` and registers the fd with tokio's reactor. Every read
//! is an epoll-driven wakeup, not a blocking-pool round-trip.
//!
//! Previously we wrapped the read end in `tokio::fs::File`, which ships
//! each read off to tokio's blocking thread pool. With the Receiver,
//! stdout-heavy jails no longer fight for a slot in that pool, and
//! idle jails cost nothing between events.

use crate::error::{JailError, Result};
use rustix::pipe::{PipeFlags, pipe_with};
use std::os::fd::OwnedFd;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::net::unix::pipe::Receiver;

/// A pair of pipe file descriptors. Created with `O_CLOEXEC` so neither
/// end leaks past an unrelated `exec()`.
pub struct Pipe {
    pub read: OwnedFd,
    pub write: OwnedFd,
}

impl Pipe {
    pub fn new() -> Result<Self> {
        let (read, write) = pipe_with(PipeFlags::CLOEXEC).map_err(JailError::Pipe)?;
        Ok(Self { read, write })
    }
}

/// Maximum bytes for a single line read from a jailed process.
const MAX_LINE_BYTES: usize = 1024 * 1024; // 1 MiB

/// Maximum total bytes to buffer from a jailed process's stdout/stderr.
const MAX_OUTPUT_BYTES: u64 = 256 * 1024 * 1024; // 256 MiB

/// Initial capacity for `read_all` — avoids the geometric grow-loop
/// on moderate-output jails.
const READ_ALL_INITIAL_CAP: usize = 64 * 1024;

/// Output stream from a running jail.
///
/// Holds the reader in an `Option` so internal callers can move it out
/// (via [`Self::closed`]) for concurrent drain tasks without breaking
/// the `JailHandle` Drop invariant.
pub struct OutputStream {
    reader: Option<BufReader<Receiver>>,
}

impl OutputStream {
    /// Create from an owned file descriptor. Takes ownership — the fd
    /// will be closed when the `OutputStream` is dropped.
    ///
    /// Sets `O_NONBLOCK` on the fd and registers it with tokio's reactor.
    /// Returns an error if the fd cannot be registered (non-pipe fd,
    /// runtime not running, etc.).
    pub fn from_owned_fd(fd: OwnedFd) -> std::io::Result<Self> {
        let recv = Receiver::from_owned_fd(fd)?;
        Ok(Self {
            reader: Some(BufReader::new(recv)),
        })
    }

    /// Sentinel stream that returns EOF immediately. Used as a placeholder
    /// after `mem::replace` so a `JailHandle` can move its real stream out
    /// for a background drain task.
    pub(crate) fn closed() -> Self {
        Self { reader: None }
    }

    pub async fn read_line(&mut self) -> Option<String> {
        let reader = self.reader.as_mut()?;
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) => None,
            Ok(_) => {
                // Truncate absurdly long lines to prevent OOM.
                if line.len() > MAX_LINE_BYTES {
                    line.truncate(MAX_LINE_BYTES);
                }
                Some(line)
            }
            Err(_) => None,
        }
    }

    pub async fn read_all(&mut self) -> Vec<u8> {
        let Some(reader) = self.reader.as_mut() else {
            return Vec::new();
        };
        let mut buf = Vec::with_capacity(READ_ALL_INITIAL_CAP);
        // Cap total output size so a malicious child can't OOM the parent.
        let _ = reader
            .take(MAX_OUTPUT_BYTES)
            .read_to_end(&mut buf)
            .await;
        buf
    }
}
