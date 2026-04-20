//! POSIX process utilities used by [`crate::run`].
//!
//! Split out from `run.rs` to keep the file holding the public
//! `Jail` / `JailHandle` surface focused on the domain model. These
//! helpers are all OS-level: send a signal, reap a pid, translate a
//! `WaitStatus` into an exit code.

use crate::run::JailPid;
use rustix::process::{WaitOptions, WaitStatus, waitpid};
use std::time::Duration;

/// Kill a process and its entire process group.
pub(crate) fn kill_tree(pid: JailPid) {
    // SAFETY: libc::kill with any pid is defined; the negative pid
    // means "process group `-pid`". Best-effort — ignore errors (the
    // target may already be dead).
    unsafe {
        libc::kill(-pid.as_i32(), libc::SIGKILL);
        libc::kill(pid.as_i32(), libc::SIGKILL);
    }
}

/// Poll `waitpid(NOHANG)` until the child exits. Returns the exit code
/// in the same convention as [`extract_exit_code`].
pub(crate) async fn wait_for_pid(pid: JailPid) -> i32 {
    loop {
        match waitpid(pid.to_rustix(), WaitOptions::NOHANG) {
            Ok(Some(status)) => return extract_exit_code(status),
            Ok(None) => tokio::time::sleep(Duration::from_millis(50)).await,
            Err(_) => return -1,
        }
    }
}

/// Translate a POSIX `WaitStatus` into a shell-style exit code:
/// clean exit → its status; killed by signal N → `128 + N`; otherwise
/// `-1`.
pub(crate) fn extract_exit_code(status: WaitStatus) -> i32 {
    if status.exited() {
        status.exit_status().map(|c| c as i32).unwrap_or(-1)
    } else if status.signaled() {
        status
            .terminating_signal()
            .map(|s| 128 + s as i32)
            .unwrap_or(-1)
    } else {
        -1
    }
}
