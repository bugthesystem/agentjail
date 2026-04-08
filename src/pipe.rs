//! Pipe utilities for capturing stdout/stderr.

use crate::error::{JailError, Result};
use rustix::pipe::{PipeFlags, pipe_with};
use std::os::fd::{FromRawFd, OwnedFd};
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

/// Output stream from a running jail.
pub struct OutputStream {
    reader: BufReader<tokio::fs::File>,
}

impl OutputStream {
    /// Create from a raw file descriptor.
    ///
    /// # Safety
    /// The fd must be valid and owned.
    pub unsafe fn from_raw_fd(fd: i32) -> Self {
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        let async_file = tokio::fs::File::from_std(file);
        Self {
            reader: BufReader::new(async_file),
        }
    }

    pub async fn read_line(&mut self) -> Option<String> {
        let mut line = String::new();
        match self.reader.read_line(&mut line).await {
            Ok(0) => None,
            Ok(_) => Some(line),
            Err(_) => None,
        }
    }

    pub async fn read_all(&mut self) -> Vec<u8> {
        let mut buf = Vec::new();
        let _ = self.reader.read_to_end(&mut buf).await;
        buf
    }
}
