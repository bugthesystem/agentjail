//! Jail execution: fork, setup, exec.

use crate::cgroup::Cgroup;
use crate::config::{JailConfig, Network};
use crate::error::{JailError, Result};
use crate::events::{EventReceiver, EventSender, JailEvent};
use crate::namespace::{NamespaceConfig, enter_namespaces, setup_loopback, write_uid_gid_map};
use crate::pipe::{OutputStream, Pipe};
use crate::seccomp::apply_filter;
use crate::{events, landlock, mount};

use rustix::process::{Pid, WaitOptions, WaitStatus, waitpid};
use std::ffi::CString;
use std::os::fd::{AsRawFd, IntoRawFd};
use std::time::{Duration, Instant};

/// A configured jail ready to execute commands.
pub struct Jail {
    config: JailConfig,
}

/// Handle to a running jailed process.
pub struct JailHandle {
    pid: u32,
    pub stdout: OutputStream,
    pub stderr: OutputStream,
    start_time: Instant,
    timeout: Duration,
    cgroup: Option<Cgroup>,
}

/// Result of a completed jail execution.
#[derive(Debug)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
    pub duration: Duration,
    pub timed_out: bool,
    pub oom_killed: bool,
    pub stats: Option<ResourceStats>,
}

/// Resource usage statistics from cgroup.
#[derive(Debug, Clone, Default)]
pub struct ResourceStats {
    /// Peak memory usage in bytes.
    pub memory_peak_bytes: u64,
    /// Total CPU time used in microseconds.
    pub cpu_usage_usec: u64,
    /// Whether OOM killer was triggered.
    pub oom_killed: bool,
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

        Ok(Self { config })
    }

    /// Create cgroup for a new spawn.
    fn create_cgroup(&self, pid: u32) -> Option<Cgroup> {
        let config = &self.config;
        if config.memory_mb == 0 && config.cpu_percent == 0 && config.max_pids == 0 {
            return None;
        }

        let name = format!("{}-{}", std::process::id(), pid);
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
    }

    /// Spawn a command in the jail.
    pub fn spawn(&self, cmd: &str, args: &[&str]) -> Result<JailHandle> {
        let stdout_pipe = Pipe::new()?;
        let stderr_pipe = Pipe::new()?;

        let config = self.config.clone();
        let cmd = cmd.to_string();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        // SAFETY: fork() is safe when we immediately either _exit() or exec() in child.
        // Parent continues normally after fork returns.
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

        // Create and configure cgroup for this process
        let cgroup = self.create_cgroup(child_pid);
        if let Some(ref cg) = cgroup {
            let _ = cg.add_pid(child_pid);
        }

        // SAFETY: We own these fds from the pipe and transfer ownership to OutputStream.
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
            cgroup,
        })
    }

    /// Run a command and wait for completion.
    pub async fn run(&self, cmd: &str, args: &[&str]) -> Result<Output> {
        let handle = self.spawn(cmd, args)?;
        handle.wait().await
    }

    /// Spawn with event stream for monitoring.
    pub fn spawn_with_events(
        &self,
        cmd: &str,
        args: &[&str],
    ) -> Result<(JailHandle, EventReceiver)> {
        let handle = self.spawn(cmd, args)?;
        let (tx, rx) = events::channel();

        // Send started event
        let _ = tx.send(JailEvent::Started { pid: handle.pid });

        Ok((handle, rx))
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

        // Collect stats before cgroup is cleaned up
        let stats = self.collect_stats();
        let oom_killed = stats.as_ref().map(|s| s.oom_killed).unwrap_or(false);

        // Read output (process is dead, pipes will EOF)
        let stdout = self.stdout.read_all().await;
        let stderr = self.stderr.read_all().await;

        Ok(Output {
            stdout,
            stderr,
            exit_code,
            duration: start_time.elapsed(),
            timed_out,
            oom_killed,
            stats,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn kill(&self) {
        // SAFETY: Sending SIGKILL to a process we spawned.
        unsafe {
            libc::kill(self.pid as i32, libc::SIGKILL);
        }
    }

    /// Get current resource usage (live monitoring).
    pub fn stats(&self) -> Option<ResourceStats> {
        self.collect_stats()
    }

    fn collect_stats(&self) -> Option<ResourceStats> {
        let cg = self.cgroup.as_ref()?;
        Some(ResourceStats {
            memory_peak_bytes: cg.memory_peak().unwrap_or(0),
            cpu_usage_usec: cg.cpu_usage_usec().unwrap_or(0),
            oom_killed: cg.oom_killed(),
        })
    }

    /// Wait while streaming events to the sender.
    ///
    /// Streams stdout/stderr line by line and sends completion event.
    pub async fn wait_with_events(mut self, tx: EventSender) -> Result<Output> {
        let pid = self.pid;
        let timeout = self.timeout;
        let start_time = self.start_time;

        let mut all_stdout = Vec::new();
        let mut all_stderr = Vec::new();
        let mut timed_out = false;
        let mut stdout_done = false;
        let mut stderr_done = false;

        let remaining = timeout.saturating_sub(start_time.elapsed());

        let result = tokio::time::timeout(remaining, async {
            loop {
                tokio::select! {
                    line = self.stdout.read_line(), if !stdout_done => {
                        match line {
                            Some(l) => {
                                all_stdout.extend_from_slice(l.as_bytes());
                                let _ = tx.send(JailEvent::Stdout(l.trim_end().to_string()));
                            }
                            None => stdout_done = true,
                        }
                    }
                    line = self.stderr.read_line(), if !stderr_done => {
                        match line {
                            Some(l) => {
                                all_stderr.extend_from_slice(l.as_bytes());
                                let _ = tx.send(JailEvent::Stderr(l.trim_end().to_string()));
                            }
                            None => stderr_done = true,
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(50)), if stdout_done && stderr_done => {
                        // Both streams closed, check if process exited
                        if let Ok(Some(status)) = waitpid(Pid::from_raw(pid as i32), WaitOptions::NOHANG) {
                            return extract_exit_code(status);
                        }
                    }
                }
            }
        })
        .await;

        let exit_code = match result {
            Ok(code) => code,
            Err(_) => {
                timed_out = true;
                let _ = tx.send(JailEvent::TimedOut);
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                    libc::kill(pid as i32, libc::SIGKILL);
                }
                wait_for_pid(pid).await
            }
        };

        let duration = start_time.elapsed();
        let stats = self.collect_stats();
        let oom_killed = stats.as_ref().map(|s| s.oom_killed).unwrap_or(false);

        if !timed_out {
            let _ = tx.send(JailEvent::Completed { exit_code, duration });
        }

        if oom_killed {
            let _ = tx.send(JailEvent::OomKilled);
        }

        Ok(Output {
            stdout: all_stdout,
            stderr: all_stderr,
            exit_code,
            duration,
            timed_out,
            oom_killed,
            stats,
        })
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
    // 1. Enter namespaces (PID namespace entered separately for double-fork)
    let ns_config = NamespaceConfig {
        user: config.user_namespace,
        mount: true,
        pid: false, // Handled separately via double-fork
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

    // 6. Enter PID namespace and double-fork to become PID 1
    if config.pid_namespace {
        enter_pid_namespace_and_exec(config, cmd, args)?;
        unreachable!()
    }

    // 7. Apply seccomp (must be last before exec)
    apply_filter(config.seccomp)?;

    // 8. Exec
    do_exec(cmd, args, &config.env)
}

/// Enter PID namespace via double-fork pattern.
///
/// After unshare(NEWPID), the current process is NOT in the new PID namespace.
/// Only children of this process will be. So we fork, and that child becomes
/// PID 1 in the new namespace, then execs the target command.
fn enter_pid_namespace_and_exec(config: &JailConfig, cmd: &str, args: &[String]) -> Result<()> {
    use rustix::thread::{UnshareFlags, unshare};

    // Enter new PID namespace
    unshare(UnshareFlags::NEWPID).map_err(JailError::Namespace)?;

    // Fork - child will be PID 1 in the new namespace
    // SAFETY: fork() is safe when we immediately either _exit() or exec() in child.
    let pid = unsafe { libc::fork() };

    match pid {
        -1 => Err(JailError::Fork(rustix::io::Errno::from_raw_os_error(
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        ))),
        0 => {
            // Child: now PID 1 in the new namespace
            // Remount /proc for this PID namespace
            let _ = remount_proc();

            // Apply seccomp (must be last before exec)
            if let Err(e) = apply_filter(config.seccomp) {
                eprintln!("seccomp failed: {}", e);
                unsafe { libc::_exit(127) };
            }

            // Exec the target command
            if let Err(e) = do_exec(cmd, args, &config.env) {
                eprintln!("exec failed: {}", e);
                unsafe { libc::_exit(127) };
            }
            unreachable!()
        }
        child_pid => {
            // Parent: wait for child and propagate exit status
            let mut status: libc::c_int = 0;
            // SAFETY: Waiting for our own child with valid pointer.
            unsafe {
                libc::waitpid(child_pid, &mut status, 0);
            }

            let exit_code = if libc::WIFEXITED(status) {
                libc::WEXITSTATUS(status)
            } else if libc::WIFSIGNALED(status) {
                128 + libc::WTERMSIG(status)
            } else {
                1
            };

            // SAFETY: Exiting the intermediate process cleanly.
            unsafe { libc::_exit(exit_code) };
        }
    }
}

/// Remount /proc for the new PID namespace.
fn remount_proc() -> Result<()> {
    use std::ffi::CString;

    let proc = CString::new("/proc").unwrap();
    let procfs = CString::new("proc").unwrap();

    // First unmount the old /proc (from parent PID namespace)
    // SAFETY: Unmounting /proc with valid C string.
    unsafe {
        libc::umount2(proc.as_ptr(), libc::MNT_DETACH);
    }

    // Mount fresh /proc for this PID namespace
    // SAFETY: Mounting proc filesystem with valid C strings.
    let ret = unsafe {
        libc::mount(
            procfs.as_ptr(),
            proc.as_ptr(),
            procfs.as_ptr(),
            0,
            std::ptr::null(),
        )
    };

    if ret == 0 {
        Ok(())
    } else {
        Err(JailError::Exec(std::io::Error::last_os_error()))
    }
}

/// Execute the target command.
fn do_exec(cmd: &str, args: &[String], env: &[(String, String)]) -> Result<()> {
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

    let c_env: Vec<CString> = env
        .iter()
        .filter_map(|(k, v)| CString::new(format!("{}={}", k, v)).ok())
        .collect();

    let c_env_ptrs: Vec<*const libc::c_char> = c_env
        .iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect();

    // SAFETY: execve with valid C strings and null-terminated arrays.
    unsafe {
        libc::execve(c_cmd.as_ptr(), c_args_ptrs.as_ptr(), c_env_ptrs.as_ptr());
    }

    Err(JailError::Exec(std::io::Error::last_os_error()))
}
