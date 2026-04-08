//! Seccomp-BPF syscall filtering.

use crate::config::SeccompLevel;
use crate::error::{JailError, Result};
use seccompiler::{SeccompAction, SeccompFilter, TargetArch};
use std::collections::BTreeMap;

// Syscall numbers for x86_64 (from Linux kernel)
const SYS_INIT_MODULE: i64 = 175;
const SYS_FINIT_MODULE: i64 = 313;
const SYS_DELETE_MODULE: i64 = 176;
const SYS_REBOOT: i64 = 169;
const SYS_KEXEC_LOAD: i64 = 246;
const SYS_KEXEC_FILE_LOAD: i64 = 320;
const SYS_PTRACE: i64 = 101;
const SYS_PROCESS_VM_READV: i64 = 310;
const SYS_PROCESS_VM_WRITEV: i64 = 311;
const SYS_MOUNT: i64 = 165;
const SYS_UMOUNT2: i64 = 166;
const SYS_PIVOT_ROOT: i64 = 155;
const SYS_SETHOSTNAME: i64 = 170;
const SYS_SETDOMAINNAME: i64 = 171;
const SYS_ACCT: i64 = 163;
const SYS_SWAPON: i64 = 167;
const SYS_SWAPOFF: i64 = 168;
const SYS_IOPL: i64 = 172;
const SYS_IOPERM: i64 = 173;
const SYS_SETTIMEOFDAY: i64 = 164;
const SYS_CLOCK_SETTIME: i64 = 227;
const SYS_ADJTIMEX: i64 = 159;
const SYS_ADD_KEY: i64 = 248;
const SYS_REQUEST_KEY: i64 = 249;
const SYS_KEYCTL: i64 = 250;

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

fn build_standard_filter() -> Result<SeccompFilter> {
    let blocked: &[i64] = &[
        SYS_INIT_MODULE,
        SYS_FINIT_MODULE,
        SYS_DELETE_MODULE,
        SYS_REBOOT,
        SYS_KEXEC_LOAD,
        SYS_KEXEC_FILE_LOAD,
        SYS_PTRACE,
        SYS_PROCESS_VM_READV,
        SYS_PROCESS_VM_WRITEV,
        SYS_MOUNT,
        SYS_UMOUNT2,
        SYS_PIVOT_ROOT,
        SYS_SETHOSTNAME,
        SYS_SETDOMAINNAME,
        SYS_ACCT,
        SYS_SWAPON,
        SYS_SWAPOFF,
        SYS_IOPL,
        SYS_IOPERM,
        SYS_SETTIMEOFDAY,
        SYS_CLOCK_SETTIME,
        SYS_ADJTIMEX,
        SYS_ADD_KEY,
        SYS_REQUEST_KEY,
        SYS_KEYCTL,
    ];

    build_blocklist_filter(blocked)
}

fn build_strict_filter() -> Result<SeccompFilter> {
    build_standard_filter()
}

fn build_blocklist_filter(blocked_syscalls: &[i64]) -> Result<SeccompFilter> {
    // For a blocklist filter:
    // - Default action: Allow (most syscalls pass through)
    // - Match action: Errno (blocked syscalls return EPERM)
    // - Empty rules map means "match all calls to this syscall number"

    let rules: BTreeMap<i64, Vec<seccompiler::SeccompRule>> = blocked_syscalls
        .iter()
        .map(|&num| (num, vec![]))  // Empty vec = match unconditionally
        .collect();

    SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        TargetArch::x86_64,
    )
    .map_err(|e| JailError::Seccomp(e.to_string()))
}
