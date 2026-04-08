//! Jail execution: fork, setup, exec.

use crate::cgroup::Cgroup;
use crate::config::{JailConfig, Network};
use crate::error::{JailError, Result};
use crate::namespace::{NamespaceConfig, enter_namespaces, setup_loopback, write_uid_gid_map};
use crate::pipe::{OutputStream, Pipe};
use crate::seccomp::apply_filter;
use crate::{landlock, mount};

use rustix::process::{Pid, WaitOptions, WaitStatus, waitpid};
use std::ffi::CString;
use std::os::fd::{AsRawFd, IntoRawFd};
use std::time::{Duration, Instant};

/// A configured jail ready to execute commands.
pub struct Jail {
    config: JailConfig,
    cgroup: Option<Cgroup>,
}

/// Handle to a running jailed process.
pub struct JailHandle {
    pid: u32,
    pub stdout: OutputStream,
    pub stderr: OutputStream,
    start_time: Instant,
    timeout: Duration,
}

/// Result of a completed jail execution.
#[derive(Debug)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
    pub duration: Duration,
    pub timed_out: bool,
}

impl Jail {
    /// Create a new jail from configuration.
    pub fn new(config: JailConfig) -> Result<Self> {
        if !config.source.exists() {
            return Err(JailError::PathNotFound(config.source.clone()));
        }
        if !config.output.exists() {
            return Err(JailError::PathNotFound(config.output.clone()));
        }

        let cgroup = if config.memory_mb > 0 || config.cpu_percent > 0 || config.max_pids > 0 {
            let name = format!("{}", std::process::id());
            match Cgroup::create(&name) {
                Ok(cg) => {
                    if config.memory_mb > 0 {
                        let _ = cg.set_memory_limit(config.memory_mb * 1024 * 1024);
                    }
                    if config.cpu_percent > 0 {
                        let _ = cg.set_cpu_quota(config.cpu_percent);
                    }
                    if config.max_pids > 0 {
                        let _ = cg.set_pids_max(config.max_pids);
                    }
                    Some(cg)
                }
                Err(_) => None,
            }
        } else {
            None
        };

        Ok(Self { config, cgroup })
    }

    /// Spawn a command in the jail.
    pub fn spawn(&self, cmd: &str, args: &[&str]) -> Result<JailHandle> {
        let stdout_pipe = Pipe::new()?;
        let stderr_pipe = Pipe::new()?;

        let config = self.config.clone();
        let cmd = cmd.to_string();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        let child_pid = unsafe {
            match libc::fork() {
                -1 => {
                    return Err(JailError::Fork(rustix::io::Errno::from_raw_os_error(
                        std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
                    )));
                }
                0 => {
                    // Child process
                    // Create new session and process group so we can kill all descendants
                    libc::setsid();

                    libc::dup2(stdout_pipe.write.as_raw_fd(), libc::STDOUT_FILENO);
                    libc::dup2(stderr_pipe.write.as_raw_fd(), libc::STDERR_FILENO);
                    drop(stdout_pipe);
                    drop(stderr_pipe);

                    if let Err(e) = setup_child(&config, &cmd, &args) {
                        eprintln!("jail setup failed: {}", e);
                        libc::_exit(127);
                    }
                    unreachable!()
                }
                pid => pid as u32,
            }
        };

        // Parent: close write ends
        drop(stdout_pipe.write);
        drop(stderr_pipe.write);

        // Write UID/GID maps if using user namespace
        if config.user_namespace {
            if let Some(pid) = Pid::from_raw(child_pid as i32) {
                let _ = write_uid_gid_map(pid);
            }
        }

        // Add child to cgroup
        if let Some(ref cg) = self.cgroup {
            let _ = cg.add_pid(child_pid);
        }

        let stdout = unsafe { OutputStream::from_raw_fd(stdout_pipe.read.into_raw_fd()) };
        let stderr = unsafe { OutputStream::from_raw_fd(stderr_pipe.read.into_raw_fd()) };

        let timeout = if config.timeout_secs > 0 {
            Duration::from_secs(config.timeout_secs)
        } else {
            Duration::from_secs(u64::MAX)
        };

        Ok(JailHandle {
            pid: child_pid,
            stdout,
            stderr,
            start_time: Instant::now(),
            timeout,
        })
    }

    /// Run a command and wait for completion.
    pub async fn run(&self, cmd: &str, args: &[&str]) -> Result<Output> {
        let handle = self.spawn(cmd, args)?;
        handle.wait().await
    }
}

impl JailHandle {
    /// Wait for the process to complete and collect output.
    pub async fn wait(mut self) -> Result<Output> {
        let pid = self.pid;
        let timeout = self.timeout;
        let start_time = self.start_time;
        let mut timed_out = false;

        let remaining = timeout.saturating_sub(start_time.elapsed());
        let wait_result = tokio::time::timeout(remaining, wait_for_pid(pid)).await;

        let exit_code = match wait_result {
            Ok(code) => code,
            Err(_) => {
                // Timeout - kill the entire process group
                timed_out = true;
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                    libc::kill(pid as i32, libc::SIGKILL);
                }
                wait_for_pid(pid).await
            }
        };

        // Read output (process is dead, pipes will EOF)
        let stdout = self.stdout.read_all().await;
        let stderr = self.stderr.read_all().await;

        Ok(Output {
            stdout,
            stderr,
            exit_code,
            duration: start_time.elapsed(),
            timed_out,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn kill(&self) {
        unsafe {
            libc::kill(self.pid as i32, libc::SIGKILL);
        }
    }
}

async fn wait_for_pid(pid: u32) -> i32 {
    loop {
        match waitpid(Pid::from_raw(pid as i32), WaitOptions::NOHANG) {
            Ok(Some(status)) => return extract_exit_code(status),
            Ok(None) => tokio::time::sleep(Duration::from_millis(50)).await,
            Err(_) => return -1,
        }
    }
}

fn extract_exit_code(status: WaitStatus) -> i32 {
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

/// Setup the child process inside the jail.
fn setup_child(config: &JailConfig, cmd: &str, args: &[String]) -> Result<()> {
    // 1. Enter namespaces
    let ns_config = NamespaceConfig {
        user: config.user_namespace,
        mount: true,
        pid: false, // Disabled - requires double-fork
        network: config.network == Network::None,
        ipc: config.ipc_namespace,
    };
    enter_namespaces(ns_config)?;

    // 2. Setup network if loopback mode
    if config.network == Network::Loopback {
        let _ = setup_loopback();
    }

    // 3. Setup filesystem mounts
    mount::make_root_private()?;

    let new_root = std::env::temp_dir().join(format!("agentjail-{}", std::process::id()));
    mount::setup_root(&new_root, &config.source, &config.output)?;

    // 4. Apply landlock
    if config.landlock && landlock::is_available() {
        let rules = [
            (config.source.as_path(), crate::config::Access::ReadOnly),
            (config.output.as_path(), crate::config::Access::ReadWrite),
        ];
        let _ = landlock::apply_rules(&rules);
    }

    // 5. Chroot into new root
    std::env::set_current_dir(&new_root).map_err(JailError::Exec)?;
    rustix::process::chroot(".").map_err(JailError::Namespace)?;
    std::env::set_current_dir("/workspace").map_err(JailError::Exec)?;

    // 6. Apply seccomp (must be last)
    apply_filter(config.seccomp)?;

    // 7. Exec
    let c_cmd =
        CString::new(cmd).map_err(|_| JailError::Exec(std::io::Error::other("invalid command")))?;

    let c_args: Vec<CString> = std::iter::once(c_cmd.clone())
        .chain(args.iter().filter_map(|a| CString::new(a.as_str()).ok()))
        .collect();

    let c_args_ptrs: Vec<*const libc::c_char> = c_args
        .iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect();

    let c_env: Vec<CString> = config
        .env
        .iter()
        .filter_map(|(k, v)| CString::new(format!("{}={}", k, v)).ok())
        .collect();

    let c_env_ptrs: Vec<*const libc::c_char> = c_env
        .iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect();

    unsafe {
        libc::execve(c_cmd.as_ptr(), c_args_ptrs.as_ptr(), c_env_ptrs.as_ptr());
    }

    Err(JailError::Exec(std::io::Error::last_os_error()))
}
