//! Jail execution: fork, setup, exec.

use crate::cgroup::Cgroup;
use crate::config::{JailConfig, Network};
use crate::error::{JailError, Result};
use crate::events::{EventReceiver, EventSender, JailEvent};
use crate::namespace::{NamespaceConfig, enter_namespaces, setup_loopback, write_uid_gid_map};
use crate::pipe::{OutputStream, Pipe};
use crate::proxy::ProxyConfig;
use crate::seccomp::apply_filter;
use crate::{events, gpu, landlock, mount, proxy};

use rustix::process::{Pid, WaitOptions, WaitStatus, waitpid};
use std::ffi::CString;
use std::os::fd::{AsRawFd, IntoRawFd};
use std::time::{Duration, Instant};

/// A configured jail ready to execute commands.
pub struct Jail {
    config: JailConfig,
    /// Pre-discovered GPU resources (if gpu.enabled).
    gpu_resources: Option<gpu::NvidiaResources>,
}

/// Handle to a running jailed process.
pub struct JailHandle {
    pid: u32,
    pub stdout: OutputStream,
    pub stderr: OutputStream,
    start_time: Instant,
    timeout: Duration,
    cgroup: Option<Cgroup>,
    /// Host-side veth interface name to clean up (Allowlist mode only).
    veth_host_iface: Option<String>,
    /// Shutdown signal for the proxy thread (Allowlist mode only).
    proxy_shutdown: Option<tokio::sync::watch::Sender<bool>>,
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
    /// Total bytes read from disk.
    pub io_read_bytes: u64,
    /// Total bytes written to disk.
    pub io_write_bytes: u64,
}

impl Jail {
    /// Create a new jail from configuration.
    ///
    /// Validates paths and discovers GPU resources upfront so errors
    /// are reported before forking.
    pub fn new(config: JailConfig) -> Result<Self> {
        if !config.source.exists() {
            return Err(JailError::PathNotFound(config.source.clone()));
        }
        if !config.output.exists() {
            return Err(JailError::PathNotFound(config.output.clone()));
        }

        let gpu_resources = if config.gpu.enabled {
            Some(gpu::discover(&config.gpu)?)
        } else {
            None
        };

        Ok(Self {
            config,
            gpu_resources,
        })
    }

    /// Create cgroup for a new spawn.
    fn create_cgroup(&self, pid: u32) -> Result<Option<Cgroup>> {
        let config = &self.config;
        let has_limits = config.memory_mb > 0
            || config.cpu_percent > 0
            || config.max_pids > 0
            || config.io_read_mbps > 0
            || config.io_write_mbps > 0;

        if !has_limits {
            return Ok(None);
        }

        let name = format!("{}-{}", std::process::id(), pid);
        let cg = Cgroup::create(&name)?;

        if config.memory_mb > 0 {
            cg.set_memory_limit(config.memory_mb * 1024 * 1024)?;
        }
        if config.cpu_percent > 0 {
            cg.set_cpu_quota(config.cpu_percent)?;
        }
        if config.max_pids > 0 {
            cg.set_pids_max(config.max_pids)?;
        }
        if config.io_read_mbps > 0 || config.io_write_mbps > 0 {
            let read_bps = config.io_read_mbps * 1024 * 1024;
            let write_bps = config.io_write_mbps * 1024 * 1024;
            if let Err(e) = cg.set_io_limit(
                config.output.to_str().unwrap_or("/"),
                read_bps,
                write_bps,
            ) {
                eprintln!("warning: I/O limits not applied: {}", e);
            }
        }

        Ok(Some(cg))
    }

    /// Spawn a command in the jail.
    pub fn spawn(&self, cmd: &str, args: &[&str]) -> Result<JailHandle> {
        let stdout_pipe = Pipe::new()?;
        let stderr_pipe = Pipe::new()?;

        // For Allowlist mode, we need a sync pipe so the child can signal
        // "I've entered my network namespace" and wait for the parent to
        // set up the veth pair + proxy before continuing.
        let needs_veth = matches!(self.config.network, Network::Allowlist(_));
        let sync_pipe = if needs_veth {
            Some((Pipe::new()?, Pipe::new()?)) // (child→parent, parent→child)
        } else {
            None
        };

        let config = self.config.clone();
        let gpu_resources = self.gpu_resources.clone();
        let cmd = cmd.to_string();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        // Extract sync pipe fds for child before moving into closure
        let child_sync = sync_pipe.as_ref().map(|(c2p, p2c)| {
            (c2p.write.as_raw_fd(), p2c.read.as_raw_fd())
        });

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
                    // Create new session and process group so we can kill all descendants.
                    if libc::setsid() == -1 {
                        libc::_exit(127);
                    }

                    libc::dup2(stdout_pipe.write.as_raw_fd(), libc::STDOUT_FILENO);
                    libc::dup2(stderr_pipe.write.as_raw_fd(), libc::STDERR_FILENO);
                    drop(stdout_pipe);
                    drop(stderr_pipe);

                    if let Err(e) = setup_child(&config, &gpu_resources, &cmd, &args, child_sync) {
                        eprintln!("jail setup failed: {}", e);
                        libc::_exit(127);
                    }
                    unreachable!()
                }
                pid => pid as u32,
            }
        };

        // Parent: close write ends of stdout/stderr
        drop(stdout_pipe.write);
        drop(stderr_pipe.write);

        // Write UID/GID maps if using user namespace.
        // When running as root, user namespace uid mapping may fail (root
        // already has full capabilities). Only propagate the error for
        // unprivileged users where the mapping is required.
        if config.user_namespace {
            if let Some(pid) = Pid::from_raw(child_pid as i32) {
                if let Err(e) = write_uid_gid_map(pid) {
                    if rustix::process::getuid().is_root() {
                        eprintln!("warning: uid/gid map failed (running as root): {}", e);
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        // For Allowlist mode: wait for child to enter netns, then set up veth + proxy
        let mut proxy_shutdown = None;
        if let (Some((c2p, p2c)), Network::Allowlist(domains)) =
            (sync_pipe, &config.network)
        {
            // Close child-side fds in parent
            drop(c2p.write);
            drop(p2c.read);

            // Wait for child to signal "I'm in my network namespace"
            let mut buf = [0u8; 1];
            let _ = read_byte(c2p.read.as_raw_fd(), &mut buf);

            let (host_ip, _jail_ip) = veth_ips(child_pid);

            // Set up veth pair bridging parent netns ↔ child netns
            if let Err(e) = setup_veth_pair(child_pid) {
                eprintln!("veth setup failed: {}", e);
            }

            // Spawn proxy in parent (has real network access)
            proxy_shutdown = Some(spawn_allowlist_proxy(domains.clone(), &host_ip));

            // Signal child to continue
            let _ = write_byte(p2c.write.as_raw_fd(), 1);
        }

        // Create and configure cgroup for this process
        let cgroup = self.create_cgroup(child_pid)?;
        if let Some(ref cg) = cgroup {
            cg.add_pid(child_pid)?;
        }

        // SAFETY: We own these fds from the pipe and transfer ownership to OutputStream.
        let stdout = unsafe { OutputStream::from_raw_fd(stdout_pipe.read.into_raw_fd()) };
        let stderr = unsafe { OutputStream::from_raw_fd(stderr_pipe.read.into_raw_fd()) };

        let timeout = if config.timeout_secs > 0 {
            Duration::from_secs(config.timeout_secs)
        } else {
            Duration::from_secs(u64::MAX)
        };

        let veth_host_iface = if needs_veth {
            Some(format!("aj-h{}", child_pid))
        } else {
            None
        };

        Ok(JailHandle {
            pid: child_pid,
            stdout,
            stderr,
            start_time: Instant::now(),
            timeout,
            cgroup,
            veth_host_iface,
            proxy_shutdown,
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

        // Clean up veth interface (removes both ends + stops proxy bind)
        self.cleanup_veth();

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
        let io = cg.io_stats().unwrap_or_default();
        Some(ResourceStats {
            memory_peak_bytes: cg.memory_peak().unwrap_or(0),
            cpu_usage_usec: cg.cpu_usage_usec().unwrap_or(0),
            oom_killed: cg.oom_killed(),
            io_read_bytes: io.read_bytes,
            io_write_bytes: io.write_bytes,
        })
    }

    /// Shut down the proxy and remove the host-side veth interface.
    fn cleanup_veth(&mut self) {
        // Signal proxy to stop (drop releases the watch channel)
        if let Some(tx) = self.proxy_shutdown.take() {
            let _ = tx.send(true);
        }
        if let Some(iface) = self.veth_host_iface.take() {
            let _ = run_cmd("ip", &["link", "del", &iface]);
        }
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

        // Clean up veth interface
        self.cleanup_veth();

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

impl Drop for JailHandle {
    fn drop(&mut self) {
        self.cleanup_veth();
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
///
/// `sync_fds`: For Allowlist mode, (write_fd, read_fd) for syncing with parent.
/// Child signals parent after entering netns, then waits for parent to set up veth.
fn setup_child(
    config: &JailConfig,
    gpu_resources: &Option<gpu::NvidiaResources>,
    cmd: &str,
    args: &[String],
    sync_fds: Option<(i32, i32)>,
) -> Result<()> {
    // 1. Enter namespaces (PID namespace entered separately for double-fork)
    // Network namespace is ALWAYS created to isolate the network stack.
    // The difference between modes is what we configure inside the namespace:
    //   None      → empty namespace, no interfaces up
    //   Loopback  → only loopback (lo) brought up
    //   Allowlist → loopback up + veth to parent where proxy runs
    let ns_config = NamespaceConfig {
        user: config.user_namespace,
        mount: true,
        pid: false, // Handled separately via double-fork
        network: true,
        ipc: config.ipc_namespace,
    };
    enter_namespaces(ns_config)?;

    // 2. Setup network based on mode
    match &config.network {
        Network::None => {
            // No interfaces up — complete network blackout.
        }
        Network::Loopback => {
            setup_loopback()?;
        }
        Network::Allowlist(_) => {
            // Signal parent that we've entered the network namespace
            if let Some((write_fd, read_fd)) = sync_fds {
                let _ = write_byte(write_fd, 1);
                // Wait for parent to set up the veth pair
                let mut buf = [0u8; 1];
                let _ = read_byte(read_fd, &mut buf);
                // Close sync fds
                unsafe {
                    libc::close(write_fd);
                    libc::close(read_fd);
                }
            }
            // Bring up loopback + configure our veth end
            setup_loopback()?;
            setup_veth_child()?;
        }
    }

    // 3. Setup filesystem mounts
    mount::make_root_private()?;

    let new_root = std::env::temp_dir().join(format!("agentjail-{}", std::process::id()));
    mount::setup_root(&new_root, &config.source, &config.output)?;

    // 3.5. GPU passthrough: mount pre-discovered NVIDIA devices + libraries
    if let Some(res) = &gpu_resources {
        gpu::setup_mounts(&new_root, res)?;
    }

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

    // 6. Build final env (add proxy vars if using allowlist, GPU vars if enabled)
    let mut env = config.env.clone();
    if matches!(config.network, Network::Allowlist(_)) {
        env.extend(proxy_env_vars());
    }
    if gpu_resources.is_some() {
        env.extend(gpu::env_vars(&config.gpu));
    }

    // 7. Enter PID namespace and double-fork to become PID 1
    if config.pid_namespace {
        enter_pid_namespace_and_exec(config, cmd, args, &env)?;
        unreachable!()
    }

    // 8. Apply seccomp (must be last before exec)
    apply_filter(config.seccomp)?;

    // 9. Exec
    do_exec(cmd, args, &env)
}

/// Enter PID namespace via double-fork pattern.
///
/// After unshare(NEWPID), the current process is NOT in the new PID namespace.
/// Only children of this process will be. So we fork, and that child becomes
/// PID 1 in the new namespace, then execs the target command.
fn enter_pid_namespace_and_exec(
    config: &JailConfig,
    cmd: &str,
    args: &[String],
    env: &[(String, String)],
) -> Result<()> {
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
            if let Err(e) = do_exec(cmd, args, env) {
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

const PROXY_PORT: u16 = 8080;

/// Derive unique veth IP addresses from child PID to avoid collisions
/// between concurrent jails. Uses 10.<pid_hi>.<pid_lo>.{1,2}/24.
fn veth_ips(child_pid: u32) -> (String, String) {
    // Use the lower 16 bits of the PID to derive the second and third octets.
    // This gives us up to 65535 concurrent jails without collision.
    let b2 = ((child_pid >> 8) & 0xFF) as u8;
    let b3 = (child_pid & 0xFF) as u8;
    // Avoid 0.0 subnet (invalid) — offset by 1
    let b3 = if b2 == 0 && b3 == 0 { 1 } else { b3 };
    let host_ip = format!("10.{}.{}.1", b2, b3);
    let jail_ip = format!("10.{}.{}.2", b2, b3);
    (host_ip, jail_ip)
}

/// Spawn the allowlist proxy in a background thread.
///
/// Runs in the **parent** process (which has real network access).
/// Listens on `bind_ip:PROXY_PORT` so the child can reach it via the veth.
/// Returns a shutdown sender — drop it or send `true` to stop the proxy.
fn spawn_allowlist_proxy(
    allowlist: Vec<String>,
    bind_ip: &str,
) -> tokio::sync::watch::Sender<bool> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<std::result::Result<(), String>>(1);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let bind_ip = bind_ip.to_string();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create tokio runtime for proxy");

        let config = ProxyConfig {
            allowlist,
            port: PROXY_PORT,
            bind_ip,
        };

        rt.block_on(async {
            if let Err(e) = proxy::run_proxy(config, tx, shutdown_rx).await {
                eprintln!("proxy error: {}", e);
            }
        });
    });

    // Wait for proxy to confirm it's listening (or report failure).
    match rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => eprintln!("proxy bind failed: {}", e),
        Err(_) => eprintln!("proxy thread died before signaling readiness"),
    }

    shutdown_tx
}

/// Get proxy environment variables pointing to the parent-side proxy via veth.
/// Called from the child, uses its PID to derive the host IP.
fn proxy_env_vars() -> Vec<(String, String)> {
    let (host_ip, _) = veth_ips(std::process::id());
    let proxy_url = format!("http://{}:{}", host_ip, PROXY_PORT);
    vec![
        ("HTTP_PROXY".into(), proxy_url.clone()),
        ("HTTPS_PROXY".into(), proxy_url.clone()),
        ("http_proxy".into(), proxy_url.clone()),
        ("https_proxy".into(), proxy_url),
    ]
}

// ---------------------------------------------------------------------------
// Veth pair setup for allowlist proxy
// ---------------------------------------------------------------------------

/// Read a single byte from an fd (blocking).
fn read_byte(fd: i32, buf: &mut [u8; 1]) -> std::io::Result<()> {
    let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, 1) };
    if n == 1 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

/// Write a single byte to an fd.
fn write_byte(fd: i32, val: u8) -> std::io::Result<()> {
    let n = unsafe { libc::write(fd, &val as *const u8 as *const libc::c_void, 1) };
    if n == 1 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

/// Create a veth pair and move one end into the child's network namespace.
///
/// Called from the parent after the child has entered its netns.
/// Sets up: aj-h<pid> (parent side) ↔ aj-j<pid> (child side).
/// IPs are derived from the PID to avoid collisions between concurrent jails.
fn setup_veth_pair(child_pid: u32) -> std::io::Result<()> {
    let host_if = format!("aj-h{}", child_pid);
    let jail_if = format!("aj-j{}", child_pid);
    let (host_ip, _) = veth_ips(child_pid);

    // Create veth pair
    run_cmd("ip", &[
        "link", "add", &host_if, "type", "veth", "peer", "name", &jail_if,
    ])?;

    // Move jail end into child's netns
    let pid_str = child_pid.to_string();
    run_cmd("ip", &["link", "set", &jail_if, "netns", &pid_str])?;

    // Configure host end
    run_cmd("ip", &[
        "addr", "add", &format!("{}/24", host_ip), "dev", &host_if,
    ])?;
    run_cmd("ip", &["link", "set", &host_if, "up"])?;

    // No IP forwarding or NAT — the child can ONLY reach the proxy on
    // the host veth IP. The proxy (running in the parent with full network
    // access) makes the real outbound connections. This prevents the jailed
    // process from bypassing the allowlist via direct connections.

    Ok(())
}

/// Configure the child's veth end (called from inside the child's netns).
fn setup_veth_child() -> Result<()> {
    // At this point we haven't entered a PID namespace yet, so getpid()
    // returns the same PID the parent used to name the interface.
    let pid = std::process::id();
    let jail_if = format!("aj-j{}", pid);
    let (host_ip, jail_ip) = veth_ips(pid);

    run_cmd("ip", &[
        "addr", "add", &format!("{}/24", jail_ip), "dev", &jail_if,
    ]).map_err(JailError::Exec)?;
    run_cmd("ip", &["link", "set", &jail_if, "up"])
        .map_err(JailError::Exec)?;
    // Default route via the host side of the veth
    run_cmd("ip", &["route", "add", "default", "via", &host_ip])
        .map_err(JailError::Exec)?;

    Ok(())
}

/// Run a command and check its exit status.
fn run_cmd(cmd: &str, args: &[&str]) -> std::io::Result<()> {
    let status = std::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "{} {:?} failed with {}",
            cmd, args, status
        )))
    }
}
