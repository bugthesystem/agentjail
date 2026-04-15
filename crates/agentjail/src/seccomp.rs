//! Seccomp-BPF syscall filtering.

use crate::config::SeccompLevel;
use crate::error::{JailError, Result};
use seccompiler::{SeccompAction, SeccompFilter, TargetArch};
use std::collections::BTreeMap;

#[cfg(target_arch = "x86_64")]
const ARCH: TargetArch = TargetArch::x86_64;

#[cfg(target_arch = "aarch64")]
const ARCH: TargetArch = TargetArch::aarch64;

/// Apply a seccomp filter to the current process.
///
/// This is a one-way operation - once applied, cannot be removed.
pub fn apply_filter(level: SeccompLevel) -> Result<()> {
    let filter = match level {
        SeccompLevel::Disabled => return Ok(()),
        SeccompLevel::Standard => build_standard_filter()?,
        SeccompLevel::Strict => build_strict_filter()?,
    };

    let bpf: seccompiler::BpfProgram = filter
        .try_into()
        .map_err(|e: seccompiler::BackendError| JailError::Seccomp(e.to_string()))?;

    seccompiler::apply_filter(&bpf).map_err(|e| JailError::Seccomp(e.to_string()))?;

    Ok(())
}

/// Syscalls blocked in both Standard and Strict modes.
fn base_blocked_syscalls() -> Vec<i64> {
    #[allow(unused_mut)]
    let mut v = vec![
        // Module loading
        libc::SYS_init_module,
        libc::SYS_finit_module,
        libc::SYS_delete_module,
        // Reboot / shutdown
        libc::SYS_reboot,
        libc::SYS_kexec_load,
        // Process tracing / introspection
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        // Mount (old API)
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        // Mount (new API, Linux 5.2+) — bypasses SYS_mount block
        libc::SYS_open_tree,
        libc::SYS_move_mount,
        libc::SYS_fsopen,
        libc::SYS_fsconfig,
        libc::SYS_fsmount,
        libc::SYS_fspick,
        // Namespace escape
        libc::SYS_unshare,
        libc::SYS_setns,
        // Host identity
        libc::SYS_sethostname,
        libc::SYS_setdomainname,
        // Accounting / swap
        libc::SYS_acct,
        libc::SYS_swapon,
        libc::SYS_swapoff,
        // Clock manipulation
        libc::SYS_settimeofday,
        libc::SYS_clock_settime,
        libc::SYS_adjtimex,
        // Keyring
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_keyctl,
        // BPF / perf — information leak and privilege escalation
        libc::SYS_bpf,
        libc::SYS_perf_event_open,
        // Exploitable race condition primitives
        libc::SYS_userfaultfd,
    ];

    // iopl/ioperm are x86-only (hardware port I/O).
    #[cfg(target_arch = "x86_64")]
    {
        v.push(libc::SYS_iopl);
        v.push(libc::SYS_ioperm);
        v.push(libc::SYS_kexec_file_load);
    }

    v
}

fn build_standard_filter() -> Result<SeccompFilter> {
    build_blocklist_filter(&base_blocked_syscalls())
}

fn build_strict_filter() -> Result<SeccompFilter> {
    // Everything in Standard, plus block network socket creation.
    let mut blocked = base_blocked_syscalls();
    blocked.extend_from_slice(&[
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_sendto,
        libc::SYS_recvfrom,
        libc::SYS_sendmsg,
        libc::SYS_recvmsg,
        libc::SYS_sendmmsg,
        libc::SYS_recvmmsg,
        libc::SYS_shutdown,
    ]);
    build_blocklist_filter(&blocked)
}

fn build_blocklist_filter(blocked_syscalls: &[i64]) -> Result<SeccompFilter> {
    // Default action: Allow (most syscalls pass through)
    // Match action: Errno (blocked syscalls return EPERM)
    // Empty rules vec = match unconditionally for that syscall number

    let rules: BTreeMap<i64, Vec<seccompiler::SeccompRule>> = blocked_syscalls
        .iter()
        .map(|&num| (num, vec![]))
        .collect();

    SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        ARCH,
    )
    .map_err(|e| JailError::Seccomp(e.to_string()))
}
