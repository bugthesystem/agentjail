//! Pipe utilities for capturing stdout/stderr.

use crate::error::{JailError, Result};
use rustix::pipe::{PipeFlags, pipe_with};
use std::os::fd::OwnedFd;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

/// A pair of pipe file descriptors.
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

/// Output stream from a running jail.
pub struct OutputStream {
    reader: BufReader<tokio::fs::File>,
}

impl OutputStream {
    /// Create from an owned file descriptor. Takes ownership — the fd
    /// will be closed when the `OutputStream` is dropped.
    pub fn from_owned_fd(fd: OwnedFd) -> Self {
        let file = std::fs::File::from(fd);
        let async_file = tokio::fs::File::from_std(file);
        Self {
            reader: BufReader::new(async_file),
        }
    }

    pub async fn read_line(&mut self) -> Option<String> {
        let mut line = String::new();
        match self.reader.read_line(&mut line).await {
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
        let mut buf = Vec::new();
        // Cap total output size so a malicious child can't OOM the parent.
        let _ = (&mut self.reader).take(MAX_OUTPUT_BYTES).read_to_end(&mut buf).await;
        buf
    }
}
