//! Core execution — shared types, helpers, and the two non-streaming
//! handlers (`exec_in_session`, `create_run`). [`stream`] and [`fork`]
//! reuse the types and helpers defined here.
//!
//! [`stream`]: super::stream
//! [`fork`]: super::fork

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::{CtlError, Result};
use crate::jails::{JailKind, JailStore};
use crate::sampler;

use super::AppState;

// ---------- shared request shapes ----------

#[derive(Debug, Deserialize, Default)]
pub(super) struct ExecOptions {
    /// Network policy. Omit (or `{"mode":"none"}`) for no network.
    #[serde(default)]
    pub(super) network: Option<NetworkSpec>,
    /// Seccomp level. "disabled" is intentionally not exposed.
    #[serde(default)]
    pub(super) seccomp: Option<SeccompSpec>,
    /// CPU quota (100 = one full core). Clamped to 1..=800.
    #[serde(default)]
    pub(super) cpu_percent: Option<u64>,
    /// PID cap. Clamped to 1..=1024.
    #[serde(default)]
    pub(super) max_pids: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub(super) enum NetworkSpec {
    None,
    Loopback,
    Allowlist {
        /// Domains (or globs like `*.api.example.com`). Non-empty, ≤ 32.
        #[serde(default)]
        domains: Vec<String>,
    },
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub(super) enum SeccompSpec {
    Standard,
    Strict,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExecRequest {
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
    timeout_secs: Option<u64>,
    memory_mb: Option<u64>,
    #[serde(flatten)]
    options: ExecOptions,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RunRequest {
    pub(super) code: String,
    #[serde(default = "default_language")]
    pub(super) language: String,
    pub(super) timeout_secs: Option<u64>,
    pub(super) memory_mb: Option<u64>,
    /// When present, the server `git clone`s this repo into the jail's
    /// source directory before running the code. The checkout is available
    /// inside the jail at `/workspace`.
    #[serde(default)]
    pub(super) git: Option<GitSpec>,
    #[serde(flatten)]
    pub(super) options: ExecOptions,
}

#[derive(Debug, Deserialize, Clone)]
pub(super) struct GitSpec {
    /// `https://…` URL. ssh/git protocols are rejected at the edge.
    repo: String,
    /// Optional branch / tag / commit.
    #[serde(default, rename = "ref")]
    git_ref: Option<String>,
}

pub(super) fn default_language() -> String {
    "javascript".into()
}

/// Resolve a language string to `(file, command)` for the jailed runtime.
pub(super) fn language_runtime(lang: &str) -> Result<(&'static str, &'static str)> {
    match lang {
        "javascript" | "js" => Ok(("main.js", "/usr/bin/node")),
        "python" | "py"     => Ok(("main.py", "/usr/bin/python3")),
        "bash" | "sh"       => Ok(("main.sh", "/bin/sh")),
        other => Err(CtlError::BadRequest(format!("unsupported language: {other}"))),
    }
}

// ---------- shared response shape ----------

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ExecResponse {
    stdout: String,
    stderr: String,
    exit_code: i32,
    duration_ms: u64,
    timed_out: bool,
    oom_killed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<StatsResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StatsResponse {
    memory_peak_bytes: u64,
    cpu_usage_usec: u64,
    io_read_bytes: u64,
    io_write_bytes: u64,
}

pub(super) fn output_to_response(output: agentjail::Output) -> ExecResponse {
    ExecResponse {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.exit_code,
        duration_ms: output.duration.as_millis() as u64,
        timed_out: output.timed_out,
        oom_killed: output.oom_killed,
        stats: output.stats.map(|s| StatsResponse {
            memory_peak_bytes: s.memory_peak_bytes,
            cpu_usage_usec: s.cpu_usage_usec,
            io_read_bytes: s.io_read_bytes,
            io_write_bytes: s.io_write_bytes,
        }),
    }
}

// ---------- handlers ----------

/// `POST /v1/sessions/:id/exec` — run a command in a session's jail.
pub(crate) async fn exec_in_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ExecRequest>,
) -> Result<Json<ExecResponse>> {
    let exec_cfg = state.exec_config.as_ref()
        .ok_or_else(|| CtlError::Internal("exec not enabled".into()))?;

    let session = state.sessions.get(&id).await
        .ok_or_else(|| CtlError::NotFound(format!("session {id}")))?;

    // Enforce session expiry.
    if let Some(exp) = session.expires_at {
        if exp < OffsetDateTime::now_utc() {
            return Err(CtlError::BadRequest("session expired".into()));
        }
    }

    // Validate inputs.
    let timeout = req.timeout_secs.unwrap_or(exec_cfg.default_timeout_secs).clamp(1, 3600);
    let memory = req.memory_mb.unwrap_or(exec_cfg.default_memory_mb).min(8192);

    // Acquire exec permit (bounded concurrency).
    let _permit = state.exec_semaphore.try_acquire()
        .map_err(|_| CtlError::BadRequest("too many concurrent executions".into()))?;
    let _guard = state.exec_metrics.start();

    let env: Vec<(String, String)> = std::iter::once(("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into()))
        .chain(session.env.iter().map(|(k, v)| (k.clone(), v.clone())))
        .collect();

    let source = tempfile::tempdir().map_err(CtlError::Io)?;
    let output = tempfile::tempdir().map_err(CtlError::Io)?;

    let config = jail_config(source.path(), output.path(), memory, timeout, env, &req.options)?;
    let jail = agentjail::Jail::new(config)?;
    let args_refs: Vec<&str> = req.args.iter().map(|s| s.as_str()).collect();

    let rec_id = state.jails.start(JailKind::Exec, req.cmd.clone(), Some(id.clone()), None).await;
    let result = run_monitored(&state.jails, rec_id, &jail, &req.cmd, &args_refs).await;
    let result = match result {
        Ok(r)  => { state.jails.finish(rec_id, &r).await; r }
        Err(e) => { state.jails.error(rec_id, e.to_string()).await; return Err(e.into()); }
    };

    tracing::info!(
        session_id = %id,
        cmd = %req.cmd,
        exit_code = result.exit_code,
        duration_ms = result.duration.as_millis() as u64,
        timed_out = result.timed_out,
        oom_killed = result.oom_killed,
        memory_peak = result.stats.as_ref().map(|s| s.memory_peak_bytes).unwrap_or(0),
        "exec completed"
    );

    Ok(Json(output_to_response(result)))
}

/// `POST /v1/runs` — one-shot code execution.
pub(crate) async fn create_run(
    State(state): State<AppState>,
    Json(req): Json<RunRequest>,
) -> Result<(StatusCode, Json<ExecResponse>)> {
    let exec_cfg = state.exec_config.as_ref()
        .ok_or_else(|| CtlError::Internal("exec not enabled".into()))?;

    // Validate.
    if req.code.len() > 1024 * 1024 {
        return Err(CtlError::BadRequest("code exceeds 1 MB".into()));
    }
    let timeout = req.timeout_secs.unwrap_or(exec_cfg.default_timeout_secs).clamp(1, 3600);
    let memory = req.memory_mb.unwrap_or(exec_cfg.default_memory_mb).min(8192);

    let _permit = state.exec_semaphore.try_acquire()
        .map_err(|_| CtlError::BadRequest("too many concurrent executions".into()))?;
    let _guard = state.exec_metrics.start();

    let source = tempfile::tempdir().map_err(CtlError::Io)?;
    let output_dir = tempfile::tempdir().map_err(CtlError::Io)?;

    let (filename, cmd) = language_runtime(&req.language)?;

    // Optional: `git clone` into the source tree before mounting.
    if let Some(g) = &req.git { git_clone(g, source.path()).await?; }

    std::fs::write(source.path().join(filename), &req.code).map_err(CtlError::Io)?;

    let run_env = vec![("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into())];
    let config = jail_config(source.path(), output_dir.path(), memory, timeout, run_env, &req.options)?;

    let jail = agentjail::Jail::new(config)?;
    let rec_id = state.jails.start(JailKind::Run, req.language.clone(), None, None).await;
    let args: Vec<String> = vec![format!("/workspace/{filename}")];
    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let result = run_monitored(&state.jails, rec_id, &jail, cmd, &args_ref).await;
    let result = match result {
        Ok(r)  => { state.jails.finish(rec_id, &r).await; r }
        Err(e) => { state.jails.error(rec_id, e.to_string()).await; return Err(e.into()); }
    };

    tracing::info!(
        language = %req.language,
        exit_code = result.exit_code,
        duration_ms = result.duration.as_millis() as u64,
        timed_out = result.timed_out,
        "run completed"
    );

    Ok((StatusCode::CREATED, Json(output_to_response(result))))
}

// ---------- shared execution plumbing ----------

/// Spawn + live-monitor + wait. Runs three cooperating tasks:
///   1. The jail process itself.
///   2. A cgroup sampler updating `memory_peak_bytes`, `cpu_usage_usec`,
///      and `io_*` every 500ms.
///   3. A stdout/stderr tailer that reads lines into a capped buffer and
///      flushes to `JailStore::tail` every 500ms so the Jails page
///      renders a live `tail -f` of any running jail, not just SSE ones.
pub(super) async fn run_monitored(
    jails: &Arc<dyn JailStore>,
    rec_id: i64,
    jail: &agentjail::Jail,
    cmd: &str,
    args: &[&str],
) -> agentjail::Result<agentjail::Output> {
    use std::sync::Mutex;
    let mut handle = jail.spawn(cmd, args)?;

    // Stats sampler (cgroup)
    let stats_task = handle.cgroup_path().map(|p| {
        let js = jails.clone();
        sampler::spawn(p, std::time::Duration::from_millis(500), move |s| {
            let js = js.clone();
            tokio::spawn(async move { js.sample_stats(rec_id, &s).await; });
        })
    });

    // Buffered stdout/stderr with periodic DB flush.
    let buf_stdout = Arc::new(Mutex::new(String::new()));
    let buf_stderr = Arc::new(Mutex::new(String::new()));

    // Flush ticker → JailStore::tail.
    let flush_task = {
        let js = jails.clone();
        let o  = buf_stdout.clone();
        let e  = buf_stderr.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            tick.tick().await; // skip immediate
            loop {
                tick.tick().await;
                let (so, se) = {
                    let so = o.lock().map(|g| g.clone()).unwrap_or_default();
                    let se = e.lock().map(|g| g.clone()).unwrap_or_default();
                    (so, se)
                };
                js.tail(rec_id, &so, &se).await;
            }
        })
    };

    // Drain stdout/stderr in-process. We need to read them here because
    // handle.wait() only collects them at the end. Use tokio::select over
    // both pipes + the wait future; aggregate into the shared buffers.
    let mut stdout_done = false;
    let mut stderr_done = false;
    while !stdout_done || !stderr_done {
        tokio::select! {
            biased;
            line = handle.stdout.read_line(), if !stdout_done => {
                match line {
                    Some(l) => { if let Ok(mut b) = buf_stdout.lock() { push_capped(&mut b, &l); } }
                    None    => { stdout_done = true; }
                }
            }
            line = handle.stderr.read_line(), if !stderr_done => {
                match line {
                    Some(l) => { if let Ok(mut b) = buf_stderr.lock() { push_capped(&mut b, &l); } }
                    None    => { stderr_done = true; }
                }
            }
        }
    }

    let out = handle.wait().await;
    if let Some(h) = stats_task { h.abort(); }
    flush_task.abort();

    // Push the final buffer so the DB reflects the full captured output
    // even when no one was tailing.
    let (so, se) = (
        buf_stdout.lock().map(|g| g.clone()).unwrap_or_default(),
        buf_stderr.lock().map(|g| g.clone()).unwrap_or_default(),
    );
    jails.tail(rec_id, &so, &se).await;

    // Merge captured bytes into the Output (handle.wait() returns empty
    // stdout/stderr because we drained the pipes line-by-line).
    out.map(|mut o| {
        if o.stdout.is_empty() && !so.is_empty() { o.stdout = so.into_bytes(); }
        if o.stderr.is_empty() && !se.is_empty() { o.stderr = se.into_bytes(); }
        o
    })
}

/// Shallow-clone a repo into the jail's source directory. Runs on the
/// host, *before* the jail locks down the filesystem.
///
/// Security:
///  - scheme must be `https://` (no ssh/git/file)
///  - URL length capped at 512 bytes; ref at 200
///  - `git` runs with a 60s hard timeout and `--depth=1`
pub(super) async fn git_clone(spec: &GitSpec, dst: &std::path::Path) -> Result<()> {
    if !spec.repo.starts_with("https://") || spec.repo.len() > 512 {
        return Err(CtlError::BadRequest("git.repo must be https:// (max 512 bytes)".into()));
    }
    if let Some(r) = &spec.git_ref {
        if r.len() > 200 || r.chars().any(|c| c.is_control()) {
            return Err(CtlError::BadRequest("git.ref invalid".into()));
        }
    }

    // Write a .git_keep so the clone target isn't empty before git fills it.
    // git clone refuses to clone into a non-empty dir, so stage into a subpath.
    let target = dst.join("__repo");
    std::fs::create_dir_all(&target).map_err(CtlError::Io)?;

    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("clone").arg("--depth=1").arg("--single-branch");
    if let Some(r) = &spec.git_ref {
        cmd.arg("--branch").arg(r);
    }
    cmd.arg(&spec.repo).arg(&target);
    cmd.kill_on_drop(true);

    let child_fut = cmd.output();
    let out = tokio::time::timeout(std::time::Duration::from_secs(60), child_fut)
        .await
        .map_err(|_| CtlError::BadRequest("git clone timed out (60s)".into()))?
        .map_err(CtlError::Io)?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: String = stderr.lines().rev().take(3).collect::<Vec<_>>().join(" · ");
        return Err(CtlError::BadRequest(format!("git clone failed: {tail}")));
    }

    // Flatten `__repo/*` up into `dst` so /workspace points directly at the
    // repo root (not /workspace/__repo).
    for entry in std::fs::read_dir(&target).map_err(CtlError::Io)? {
        let e = entry.map_err(CtlError::Io)?;
        let from = e.path();
        let to   = dst.join(e.file_name());
        std::fs::rename(&from, &to).map_err(CtlError::Io)?;
    }
    let _ = std::fs::remove_dir_all(&target);
    Ok(())
}

const OUTPUT_CAP_BYTES: usize = 16 * 1024;

fn push_capped(buf: &mut String, line: &str) {
    if buf.len() + line.len() > OUTPUT_CAP_BYTES {
        let take = OUTPUT_CAP_BYTES.saturating_sub(buf.len());
        buf.push_str(&line[..take.min(line.len())]);
        if !buf.ends_with("…") { buf.push('…'); }
        return;
    }
    buf.push_str(line);
}

/// Build a JailConfig with sensible defaults for API-driven execution.
/// Options are clamped; illegal allowlists return `400`.
pub(super) fn jail_config(
    source: &std::path::Path,
    output: &std::path::Path,
    memory_mb: u64,
    timeout_secs: u64,
    env: Vec<(String, String)>,
    options: &ExecOptions,
) -> Result<agentjail::JailConfig> {
    let network = match options.network.as_ref() {
        None | Some(NetworkSpec::None) => agentjail::Network::None,
        Some(NetworkSpec::Loopback)    => agentjail::Network::Loopback,
        Some(NetworkSpec::Allowlist { domains }) => {
            validate_domains(domains)?;
            agentjail::Network::Allowlist(domains.clone())
        }
    };
    let seccomp = match options.seccomp {
        None | Some(SeccompSpec::Standard) => agentjail::SeccompLevel::Standard,
        Some(SeccompSpec::Strict)          => agentjail::SeccompLevel::Strict,
    };
    let cpu_percent = options.cpu_percent.unwrap_or(100).clamp(1, 800);
    let max_pids    = options.max_pids.unwrap_or(64).clamp(1, 1024);

    let is_root = unsafe { libc::getuid() == 0 };
    Ok(agentjail::JailConfig {
        source: source.into(),
        output: output.into(),
        network,
        seccomp,
        landlock: false,
        memory_mb,
        cpu_percent,
        max_pids,
        timeout_secs,
        user_namespace: !is_root,
        pid_namespace: true,
        env,
        ..Default::default()
    })
}

fn validate_domains(domains: &[String]) -> Result<()> {
    if domains.is_empty() {
        return Err(CtlError::BadRequest(
            "allowlist must contain at least one domain".into(),
        ));
    }
    if domains.len() > 32 {
        return Err(CtlError::BadRequest(
            "allowlist limited to 32 domains".into(),
        ));
    }
    for d in domains {
        if d.is_empty() || d.len() > 253 {
            return Err(CtlError::BadRequest(format!("invalid domain: {d:?}")));
        }
        if d.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(CtlError::BadRequest(format!("invalid domain: {d:?}")));
        }
        if d.contains("://") {
            return Err(CtlError::BadRequest(format!(
                "domain must not include scheme: {d:?}"
            )));
        }
    }
    Ok(())
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn opts() -> ExecOptions { ExecOptions::default() }

    #[test]
    fn defaults_are_none_and_standard() {
        let c = jail_config(
            &PathBuf::from("/tmp/src"),
            &PathBuf::from("/tmp/out"),
            256, 60, vec![], &opts(),
        ).unwrap();
        assert!(matches!(c.network, agentjail::Network::None));
        assert!(matches!(c.seccomp, agentjail::SeccompLevel::Standard));
        assert_eq!(c.cpu_percent, 100);
        assert_eq!(c.max_pids, 64);
    }

    #[test]
    fn allowlist_roundtrips() {
        let o = ExecOptions {
            network: Some(NetworkSpec::Allowlist {
                domains: vec!["api.openai.com".into(), "*.mcp.example.com".into()],
            }),
            seccomp: Some(SeccompSpec::Strict),
            cpu_percent: Some(200),
            max_pids: Some(128),
            ..Default::default()
        };
        let c = jail_config(
            &PathBuf::from("/tmp/src"),
            &PathBuf::from("/tmp/out"),
            256, 60, vec![], &o,
        ).unwrap();
        match c.network {
            agentjail::Network::Allowlist(d) => assert_eq!(d.len(), 2),
            _ => panic!("expected allowlist"),
        }
        assert!(matches!(c.seccomp, agentjail::SeccompLevel::Strict));
        assert_eq!(c.cpu_percent, 200);
        assert_eq!(c.max_pids, 128);
    }

    #[test]
    fn cpu_and_pids_are_clamped() {
        let o = ExecOptions {
            cpu_percent: Some(9_999),
            max_pids: Some(100_000),
            ..Default::default()
        };
        let c = jail_config(
            &PathBuf::from("/tmp/src"),
            &PathBuf::from("/tmp/out"),
            256, 60, vec![], &o,
        ).unwrap();
        assert_eq!(c.cpu_percent, 800);
        assert_eq!(c.max_pids, 1024);
    }

    #[test]
    fn empty_allowlist_is_rejected() {
        let o = ExecOptions {
            network: Some(NetworkSpec::Allowlist { domains: vec![] }),
            ..Default::default()
        };
        assert!(jail_config(
            &PathBuf::from("/tmp/src"), &PathBuf::from("/tmp/out"),
            256, 60, vec![], &o,
        ).is_err());
    }

    #[test]
    fn domain_with_scheme_is_rejected() {
        let o = ExecOptions {
            network: Some(NetworkSpec::Allowlist {
                domains: vec!["https://api.openai.com".into()],
            }),
            ..Default::default()
        };
        assert!(jail_config(
            &PathBuf::from("/tmp/src"), &PathBuf::from("/tmp/out"),
            256, 60, vec![], &o,
        ).is_err());
    }

    #[test]
    fn allowlist_accepts_string_or_object_network_forms() {
        // object form: {"mode":"allowlist","domains":[...]}
        let json = serde_json::json!({
            "mode": "allowlist",
            "domains": ["api.openai.com"]
        });
        let n: NetworkSpec = serde_json::from_value(json).unwrap();
        match n {
            NetworkSpec::Allowlist { domains } => assert_eq!(domains, vec!["api.openai.com"]),
            _ => panic!("expected allowlist"),
        }

        // plain modes
        let n: NetworkSpec = serde_json::from_str(r#"{"mode":"none"}"#).unwrap();
        assert!(matches!(n, NetworkSpec::None));
        let n: NetworkSpec = serde_json::from_str(r#"{"mode":"loopback"}"#).unwrap();
        assert!(matches!(n, NetworkSpec::Loopback));
    }

    #[test]
    fn run_request_accepts_flattened_options() {
        let json = serde_json::json!({
            "code": "console.log(1)",
            "language": "javascript",
            "memory_mb": 128,
            "seccomp": "strict",
            "cpu_percent": 150,
            "network": { "mode": "loopback" }
        });
        let r: RunRequest = serde_json::from_value(json).unwrap();
        assert_eq!(r.memory_mb, Some(128));
        assert!(matches!(r.options.seccomp, Some(SeccompSpec::Strict)));
        assert_eq!(r.options.cpu_percent, Some(150));
    }
}
