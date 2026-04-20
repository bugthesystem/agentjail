//! Child-process setup: namespaces, mounts, chroot, exec.
//!
//! Everything in this module runs after fork(), inside the child process,
//! before exec(). The parent never calls these functions.

use crate::config::{JailConfig, Network};
use crate::error::{JailError, Result};
use crate::namespace::{NamespaceConfig, enter_namespaces, setup_loopback};
use crate::veth::{proxy_env_vars, veth_addrs};
use crate::seccomp::apply_filter;
use crate::{gpu, landlock, mount, netlink};

use std::ffi::CString;

/// Setup the child process inside the jail.
///
/// `sync_fd`: For Allowlist mode, the child's end of a socketpair for syncing
/// with the parent. Child signals "in netns", parent replies with veth ID.
pub(crate) fn setup_child(
    config: &JailConfig,
    gpu_resources: &Option<gpu::NvidiaResources>,
    cmd: &str,
    args: &[String],
    sync_fd: Option<i32>,
) -> Result<()> {
    // 1. Enter namespaces (PID namespace entered separately via double-fork)
    let ns_config = NamespaceConfig {
        user: config.user_namespace,
        mount: true,
        pid: false,
        network: true,
        ipc: config.ipc_namespace,
    };
    enter_namespaces(ns_config)?;

    // 2. Setup network
    let mut proxy_host_ip = None;
    match &config.network {
        Network::None => {}
        Network::Loopback => {
            setup_loopback()?;
        }
        Network::Allowlist(_) => {
            if let Some(fd) = sync_fd {
                // Signal parent: "I've entered my network namespace"
                // SAFETY: Valid fd from socketpair, writing 1 byte.
                if unsafe { libc::write(fd, [1u8].as_ptr() as *const _, 1) } != 1 {
                    unsafe { libc::_exit(127) };
                }
                // Read veth ID from parent (4 bytes, LE u32)
                let mut id_buf = [0u8; 4];
                // SAFETY: Valid fd from socketpair, reading 4 bytes.
                if unsafe { libc::read(fd, id_buf.as_mut_ptr() as *mut _, 4) } != 4 {
                    unsafe { libc::_exit(127) };
                }
                // SAFETY: Done with sync channel.
                unsafe { libc::close(fd) };

                let veth_id = u32::from_le_bytes(id_buf);
                let (host_ip, jail_ip) = veth_addrs(veth_id);
                let jail_if = format!("aj-j{veth_id}");

                setup_loopback()?;
                netlink::add_ipv4_addr(&jail_if, jail_ip, 30)?;
                netlink::set_link_up(&jail_if)?;
                netlink::add_default_route(host_ip)?;

                proxy_host_ip = Some(host_ip);
            }
        }
    }

    // 3. Filesystem
    mount::make_root_private()?;
    let new_root = std::env::temp_dir().join(format!("agentjail-{}", std::process::id()));
    mount::setup_root(&new_root, &config.source, &config.output, config.source_rw)?;

    if let Some(res) = &gpu_resources {
        gpu::setup_mounts(&new_root, res)?;
    }

    // 4. Landlock
    if config.landlock && landlock::is_available() {
        let rules = [
            (config.source.as_path(), crate::config::Access::ReadOnly),
            (config.output.as_path(), crate::config::Access::ReadWrite),
        ];
        if let Err(e) = landlock::apply_rules(&rules) {
            eprintln!("warning: landlock enforcement failed: {e}");
        }
    }

    // 5. Chroot
    std::env::set_current_dir(&new_root).map_err(JailError::Exec)?;
    rustix::process::chroot(".").map_err(JailError::Namespace)?;
    std::env::set_current_dir("/workspace").map_err(JailError::Exec)?;

    // 6. Environment
    let mut env = config.env.clone();
    if let Some(host_ip) = proxy_host_ip {
        env.extend(proxy_env_vars(host_ip));
    }
    if gpu_resources.is_some() {
        env.extend(gpu::env_vars(&config.gpu));
    }

    // 7. Resource limits + privilege hardening.
    set_fd_limit(4096);
    // Prevent core dumps (could write sensitive memory to output dir).
    set_core_limit(0);
    // Prevent privilege escalation via setuid binaries, even if seccomp is disabled.
    // SAFETY: PR_SET_NO_NEW_PRIVS is always safe to set.
    unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };

    // 8. PID namespace double-fork
    if config.pid_namespace {
        enter_pid_namespace_and_exec(config, cmd, args, &env)?;
        unreachable!()
    }

    // 9. Seccomp (must be last before exec)
    apply_filter(config.seccomp)?;

    // 10. Exec
    do_exec(cmd, args, &env)
}

/// Enter PID namespace via double-fork pattern.
///
/// After unshare(NEWPID), the current process is NOT in the new PID namespace.
/// Only children will be. So we fork, and the child becomes PID 1.
fn enter_pid_namespace_and_exec(
    config: &JailConfig,
    cmd: &str,
    args: &[String],
    env: &[(String, String)],
) -> Result<()> {
    use rustix::thread::{UnshareFlags, unshare};

    unshare(UnshareFlags::NEWPID).map_err(JailError::Namespace)?;

    // SAFETY: fork() is safe when we immediately either _exit() or exec() in child.
    let pid = unsafe { libc::fork() };

    match pid {
        -1 => Err(JailError::Fork(rustix::io::Errno::from_raw_os_error(
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        ))),
        0 => {
            // Re-arm parent-death signal (cleared by fork). If the
            // intermediate process dies we should die too, keeping the
            // chain: parent → child → grandchild all linked.
            unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) };

            // Remount /proc for the new PID namespace. This MUST succeed
            // or the child sees the host process tree (critical info leak).
            if let Err(e) = remount_proc() {
                eprintln!("/proc remount failed (host PID leak): {e}");
                unsafe { libc::_exit(127) };
            }

            if let Err(e) = apply_filter(config.seccomp) {
                eprintln!("seccomp failed: {e}");
                unsafe { libc::_exit(127) };
            }

            if let Err(e) = do_exec(cmd, args, env) {
                eprintln!("exec failed: {e}");
                unsafe { libc::_exit(127) };
            }
            unreachable!()
        }
        child_pid => {
            let mut status: libc::c_int = 0;
            // SAFETY: Waiting for our own child with valid pointer.
            unsafe { libc::waitpid(child_pid, &mut status, 0) };

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
    // SAFETY: These are hardcoded strings with no null bytes.
    let proc = c"/proc";
    let procfs = c"proc";

    // SAFETY: Unmounting /proc with valid C string.
    unsafe { libc::umount2(proc.as_ptr(), libc::MNT_DETACH) };

    // SAFETY: Mounting proc filesystem with valid C strings.
    let ret = unsafe {
        libc::mount(procfs.as_ptr(), proc.as_ptr(), procfs.as_ptr(), 0, std::ptr::null())
    };

    if ret == 0 { Ok(()) } else { Err(JailError::Exec(std::io::Error::last_os_error())) }
}

/// Execute the target command (never returns on success).
fn do_exec(cmd: &str, args: &[String], env: &[(String, String)]) -> Result<()> {
    let c_cmd =
        CString::new(cmd).map_err(|_| JailError::Exec(std::io::Error::other("invalid command")))?;

    let c_args: Vec<CString> = std::iter::once(Ok(c_cmd.clone()))
        .chain(args.iter().map(|a| {
            CString::new(a.as_str())
                .map_err(|_| JailError::Exec(std::io::Error::other(format!("argument contains null byte: {a:?}"))))
        }))
        .collect::<Result<Vec<_>>>()?;

    let c_args_ptrs: Vec<*const libc::c_char> = c_args
        .iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect();

    let c_env: Vec<CString> = env
        .iter()
        .map(|(k, v)| {
            CString::new(format!("{k}={v}"))
                .map_err(|_| JailError::Exec(std::io::Error::other(format!("env var contains null byte: {k}={v}"))))
        })
        .collect::<Result<Vec<_>>>()?;

    let c_env_ptrs: Vec<*const libc::c_char> = c_env
        .iter()
        .map(|s| s.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect();

    // SAFETY: execve with valid C strings and null-terminated arrays.
    unsafe { libc::execve(c_cmd.as_ptr(), c_args_ptrs.as_ptr(), c_env_ptrs.as_ptr()) };

    Err(JailError::Exec(std::io::Error::last_os_error()))
}

/// Set RLIMIT_NOFILE to prevent child from exhausting system-wide fds.
fn set_fd_limit(max: u64) {
    let rlim = libc::rlimit {
        rlim_cur: max,
        rlim_max: max,
    };
    unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &rlim) };
}

/// Set RLIMIT_CORE to prevent core dumps (sensitive memory to output dir).
fn set_core_limit(max: u64) {
    let rlim = libc::rlimit {
        rlim_cur: max,
        rlim_max: max,
    };
    unsafe { libc::setrlimit(libc::RLIMIT_CORE, &rlim) };
}
