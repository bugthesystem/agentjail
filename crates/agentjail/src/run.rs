//! Jail execution: fork, setup, exec.

use crate::cgroup::Cgroup;
use crate::config::{JailConfig, Network};
use crate::error::{JailError, Result};
use crate::events::{EventReceiver, EventSender, JailEvent};
use crate::fork::{self, ForkInfo};
use crate::namespace::write_uid_gid_map;
use crate::pipe::{OutputStream, Pipe};
use crate::proxy::ProxyConfig;
use crate::{events, exec, gpu, netlink, proxy};

use rustix::process::{Pid, WaitOptions, WaitStatus, waitpid};
use std::net::{IpAddr, Ipv4Addr};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
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

        // For Allowlist mode, we need a sync channel so the child can signal
        // "I've entered my network namespace" and the parent can reply with
        // the veth ID after setting up the network bridge.
        let needs_veth = matches!(self.config.network, Network::Allowlist(_));
        let sync_pair = if needs_veth {
            Some(sync_socketpair()?)
        } else {
            None
        };

        let config = self.config.clone();
        let gpu_resources = self.gpu_resources.clone();
        let cmd = cmd.to_string();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        // Extract child-side fd before fork (child gets fds[0])
        let child_sync_fd = sync_pair.as_ref().map(|(child_fd, _)| child_fd.as_raw_fd());

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

                    if let Err(e) = exec::setup_child(&config, &gpu_resources, &cmd, &args, child_sync_fd) {
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
        let mut veth_iface_name = None;
        if let (Some((_child_fd, parent_fd)), Network::Allowlist(domains)) =
            (sync_pair, &config.network)
        {
            // Wait for child to signal "I'm in my network namespace"
            let mut buf = [0u8; 1];
            // SAFETY: Valid fd from socketpair, reading 1 byte.
            let n = unsafe { libc::read(parent_fd.as_raw_fd(), buf.as_mut_ptr() as *mut _, 1) };
            if n != 1 {
                return Err(JailError::Network(std::io::Error::other("child netns sync failed")));
            }

            let id = NEXT_VETH_ID.fetch_add(1, Ordering::Relaxed);
            let (host_ip, _jail_ip) = veth_addrs(id);
            let host_if = format!("aj-h{}", id);
            let jail_if = format!("aj-j{}", id);

            // Create veth pair, move jail end into child netns, configure host end
            netlink::create_veth_pair(&host_if, &jail_if)?;
            netlink::move_to_netns(&jail_if, child_pid)?;
            netlink::add_ipv4_addr(&host_if, host_ip, 30)?;
            netlink::set_link_up(&host_if)?;

            // Spawn proxy in parent (has real network access)
            proxy_shutdown = Some(spawn_allowlist_proxy(domains.clone(), host_ip));

            // Signal child with the veth ID so it can derive IPs
            let id_bytes = id.to_le_bytes();
            // SAFETY: Valid fd from socketpair, writing 4 bytes.
            let n = unsafe { libc::write(parent_fd.as_raw_fd(), id_bytes.as_ptr() as *const _, 4) };
            if n != 4 {
                return Err(JailError::Network(std::io::Error::other("veth ID sync failed")));
            }

            veth_iface_name = Some(host_if);
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

        Ok(JailHandle {
            pid: child_pid,
            stdout,
            stderr,
            start_time: Instant::now(),
            timeout,
            cgroup,
            veth_host_iface: veth_iface_name,
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

    /// Fork a running jail by cloning its filesystem state.
    ///
    /// Creates a copy-on-write clone of the output directory and spawns a
    /// new jail with the same configuration. The original jail continues
    /// running uninterrupted (it is frozen for sub-millisecond during the
    /// clone for consistency, then immediately thawed).
    ///
    /// On COW-capable filesystems (btrfs, xfs with reflink) the clone is
    /// nearly instant — data blocks are shared and only diverge on write.
    /// On other filesystems a regular copy is used as fallback.
    ///
    /// # Arguments
    ///
    /// * `running` — Handle to the running jail whose output directory will
    ///   be cloned. If `Some`, the jail's cgroup is frozen for a consistent
    ///   snapshot. Pass `None` to skip freezing (snapshot may be
    ///   inconsistent if the jail is actively writing).
    /// * `fork_output` — Output directory for the forked jail. Created if
    ///   it does not exist.
    /// * `cmd` / `args` — Command to execute inside the forked jail.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let (fork_handle, info) = jail.live_fork(
    ///     Some(&handle),
    ///     "/tmp/fork-output",
    ///     "python", &["evaluate.py"],
    /// )?;
    /// println!("cloned in {:?}", info.clone_duration);
    /// let result = fork_handle.wait().await?;
    /// ```
    pub fn live_fork(
        &self,
        running: Option<&JailHandle>,
        fork_output: impl Into<PathBuf>,
        cmd: &str,
        args: &[&str],
    ) -> Result<(JailHandle, ForkInfo)> {
        let fork_output = fork_output.into();

        // Freeze the running jail for a consistent snapshot.
        let frozen = running
            .map(|h| h.freeze().is_ok())
            .unwrap_or(false);

        // COW-clone the output directory.
        let clone_result = fork::cow_clone(&self.config.output, &fork_output);

        // Thaw immediately — even if the clone failed.
        if frozen {
            if let Some(h) = running {
                let _ = h.thaw();
            }
        }

        let mut fork_info = clone_result?;
        fork_info.was_frozen = frozen;

        // Build a forked Jail that shares everything except the output dir.
        let mut fork_config = self.config.clone();
        fork_config.output = fork_output;

        let fork_jail = Jail {
            config: fork_config,
            gpu_resources: self.gpu_resources.clone(),
        };

        let handle = fork_jail.spawn(cmd, args)?;

        Ok((handle, fork_info))
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

    /// Freeze all processes in this jail (via the cgroup freezer).
    ///
    /// Used during live forking to get a consistent filesystem snapshot.
    /// The freeze is sub-millisecond. Returns `Ok(())` even if no cgroup
    /// is configured (no-op in that case).
    pub fn freeze(&self) -> Result<()> {
        if let Some(ref cg) = self.cgroup {
            cg.freeze()
        } else {
            Ok(())
        }
    }

    /// Thaw (resume) all processes in this jail.
    pub fn thaw(&self) -> Result<()> {
        if let Some(ref cg) = self.cgroup {
            cg.thaw()
        } else {
            Ok(())
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
        // Signal proxy to stop
        if let Some(tx) = self.proxy_shutdown.take() {
            let _ = tx.send(true);
        }
        if let Some(iface) = self.veth_host_iface.take() {
            let _ = netlink::delete_link(&iface);
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

// ---------------------------------------------------------------------------
// Allowlist proxy helpers
// ---------------------------------------------------------------------------

const PROXY_PORT: u16 = 8080;

/// Monotonic counter for unique veth pair naming and IP addressing.
static NEXT_VETH_ID: AtomicU32 = AtomicU32::new(1);

/// Derive host/jail IP addresses from a veth ID.
pub(crate) fn veth_addrs(id: u32) -> (Ipv4Addr, Ipv4Addr) {
    let b2 = ((id >> 8) & 0xFF) as u8;
    let b3 = (id & 0xFF) as u8;
    let b3 = if b2 == 0 && b3 == 0 { 1 } else { b3 };
    (Ipv4Addr::new(10, b2, b3, 1), Ipv4Addr::new(10, b2, b3, 2))
}

/// Create a Unix socketpair for parent-child synchronization.
fn sync_socketpair() -> Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0i32; 2];
    // SAFETY: socketpair with valid args, fds array is correctly sized.
    let ret = unsafe {
        libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0, fds.as_mut_ptr())
    };
    if ret < 0 {
        return Err(JailError::Network(std::io::Error::last_os_error()));
    }
    // SAFETY: fds are valid, newly created by socketpair.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// Spawn the allowlist proxy in a background thread (parent process).
fn spawn_allowlist_proxy(
    allowlist: Vec<String>,
    bind_ip: Ipv4Addr,
) -> tokio::sync::watch::Sender<bool> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<std::result::Result<(), String>>(1);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("proxy runtime");

        let config = ProxyConfig {
            allowlist,
            port: PROXY_PORT,
            bind_ip: IpAddr::V4(bind_ip),
        };

        rt.block_on(async {
            if let Err(e) = proxy::run_proxy(config, tx, shutdown_rx).await {
                eprintln!("proxy error: {}", e);
            }
        });
    });

    match rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => eprintln!("proxy bind failed: {}", e),
        Err(_) => eprintln!("proxy thread died before signaling readiness"),
    }

    shutdown_tx
}

/// Proxy environment variables for the jailed process.
pub(crate) fn proxy_env_vars(host_ip: Ipv4Addr) -> Vec<(String, String)> {
    let url = format!("http://{}:{}", host_ip, PROXY_PORT);
    vec![
        ("HTTP_PROXY".into(), url.clone()),
        ("HTTPS_PROXY".into(), url.clone()),
        ("http_proxy".into(), url.clone()),
        ("https_proxy".into(), url),
    ]
}
