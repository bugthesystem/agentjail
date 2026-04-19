//! HTTP route handlers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use agentjail_phantom::{
    InMemoryKeyStore, KeyStore, PathGlob, Scope, SecretString, ServiceId, TokenStore,
};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::audit::{AuditRow, AuditStore};
use crate::credential::{CredentialRecord, CredentialStore, fingerprint};
use crate::error::{CtlError, Result};
use crate::session::{Session, SessionStore, new_session_id};

/// Shared service state passed to every handler.
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) tokens: Arc<dyn TokenStore>,
    pub(crate) keys: Arc<InMemoryKeyStore>,
    pub(crate) sessions: Arc<dyn SessionStore>,
    pub(crate) credentials: Arc<dyn CredentialStore>,
    pub(crate) audit: Arc<dyn AuditStore>,
    pub(crate) proxy_base_url: String,
    pub(crate) exec_config: Option<crate::exec::ExecConfig>,
    pub(crate) exec_semaphore: Arc<tokio::sync::Semaphore>,
    pub(crate) exec_metrics: Arc<crate::exec::ExecMetrics>,
}

// ---------- health ----------

pub(crate) async fn healthz() -> &'static str {
    "ok"
}

/// `GET /v1/stats` — live metrics (public, no auth).
pub(crate) async fn stats(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "active_execs": state.exec_metrics.active(),
        "total_execs": state.exec_metrics.total(),
        "sessions": state.sessions.list().await.len(),
        "credentials": state.credentials.list().await.len(),
    }))
}

// ---------- credentials ----------

#[derive(Debug, Deserialize)]
pub(crate) struct PutCredentialRequest {
    service: ServiceId,
    secret: String,
}

pub(crate) async fn put_credential(
    State(state): State<AppState>,
    Json(req): Json<PutCredentialRequest>,
) -> Result<Json<CredentialRecord>> {
    if req.secret.trim().is_empty() {
        return Err(CtlError::BadRequest("secret must be non-empty".into()));
    }
    let fp = fingerprint(&req.secret);
    state.keys.set(req.service, SecretString::new(req.secret));
    let now = OffsetDateTime::now_utc();
    let existing = state.credentials.get(req.service).await;
    let rec = CredentialRecord {
        service: req.service,
        added_at: existing.as_ref().map_or(now, |r| r.added_at),
        updated_at: now,
        fingerprint: fp,
    };
    state.credentials.upsert(rec.clone()).await;
    Ok(Json(rec))
}

pub(crate) async fn list_credentials(State(state): State<AppState>) -> Json<Vec<CredentialRecord>> {
    Json(state.credentials.list().await)
}

pub(crate) async fn delete_credential(
    State(state): State<AppState>,
    Path(service): Path<String>,
) -> Result<StatusCode> {
    let svc = parse_service(&service)?;
    match state.credentials.remove(svc).await {
        Some(_) => Ok(StatusCode::NO_CONTENT),
        None => Err(CtlError::NotFound(format!("credential {service}"))),
    }
}

fn parse_service(s: &str) -> Result<ServiceId> {
    match s {
        "openai" => Ok(ServiceId::OpenAi),
        "anthropic" => Ok(ServiceId::Anthropic),
        "github" => Ok(ServiceId::GitHub),
        "stripe" => Ok(ServiceId::Stripe),
        other => Err(CtlError::BadRequest(format!("unknown service {other}"))),
    }
}

// ---------- sessions ----------

#[derive(Debug, Deserialize)]
pub(crate) struct CreateSessionRequest {
    #[serde(default)]
    services: Vec<ServiceId>,
    /// Optional TTL in seconds.
    #[serde(default)]
    ttl_secs: Option<u64>,
    /// Optional per-service allow-list of path globs. Keys must be a subset
    /// of `services`. Missing entries mean unrestricted scope.
    ///
    /// Example:
    /// ```json
    /// { "services": ["openai", "github"],
    ///   "scopes":   { "github": ["/repos/my-org/*/issues*"] } }
    /// ```
    #[serde(default)]
    scopes: HashMap<ServiceId, Vec<String>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CreateSessionResponse {
    #[serde(flatten)]
    session: SessionView,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct SessionView {
    id: String,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    expires_at: Option<OffsetDateTime>,
    services: Vec<ServiceId>,
    env: HashMap<String, String>,
}

impl From<Session> for SessionView {
    fn from(s: Session) -> Self {
        Self {
            id: s.id,
            created_at: s.created_at,
            expires_at: s.expires_at,
            services: s.services,
            env: s.env,
        }
    }
}

pub(crate) async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<CreateSessionResponse>)> {
    if req.services.is_empty() {
        return Err(CtlError::BadRequest(
            "services must contain at least one entry".into(),
        ));
    }
    // Verify every requested service has a real key.
    for svc in &req.services {
        if state.keys.get(*svc).await.is_none() {
            return Err(CtlError::BadRequest(format!(
                "no credential configured for service {svc}"
            )));
        }
    }

    let id = new_session_id();
    let ttl = req.ttl_secs.map(Duration::from_secs);
    let expires_at = ttl.map(|d| OffsetDateTime::now_utc() + d);

    // Validate scopes: every key must be in services.
    for svc in req.scopes.keys() {
        if !req.services.contains(svc) {
            return Err(CtlError::BadRequest(format!(
                "scope for service {svc} but it is not in services"
            )));
        }
    }

    let mut env = HashMap::new();

    for svc in &req.services {
        let scope = req
            .scopes
            .get(svc)
            .map(|paths| Scope {
                allowed_paths: paths.iter().map(|p| PathGlob::new(p.clone())).collect(),
            })
            .unwrap_or_else(Scope::any);
        let token = state.tokens.issue(id.clone(), *svc, scope, ttl).await;
        let token_str = token.to_string();
        match svc {
            ServiceId::OpenAi => {
                env.insert("OPENAI_API_KEY".into(), token_str);
                env.insert(
                    "OPENAI_BASE_URL".into(),
                    format!("{}/v1/openai/v1", state.proxy_base_url),
                );
            }
            ServiceId::Anthropic => {
                env.insert("ANTHROPIC_API_KEY".into(), token_str);
                env.insert(
                    "ANTHROPIC_BASE_URL".into(),
                    format!("{}/v1/anthropic", state.proxy_base_url),
                );
            }
            ServiceId::GitHub => {
                env.insert("GITHUB_TOKEN".into(), token_str);
                env.insert(
                    "GITHUB_API_URL".into(),
                    format!("{}/v1/github", state.proxy_base_url),
                );
            }
            ServiceId::Stripe => {
                env.insert("STRIPE_API_KEY".into(), token_str);
                env.insert(
                    "STRIPE_API_BASE".into(),
                    format!("{}/v1/stripe", state.proxy_base_url),
                );
            }
        }
    }

    let session = Session {
        id: id.clone(),
        created_at: OffsetDateTime::now_utc(),
        expires_at,
        services: req.services,
        env: env.clone(),
    };
    state.sessions.insert(session.clone()).await?;

    Ok((
        StatusCode::CREATED,
        Json(CreateSessionResponse {
            session: session.into(),
        }),
    ))
}

pub(crate) async fn list_sessions(State(state): State<AppState>) -> Json<Vec<SessionView>> {
    Json(
        state
            .sessions
            .list()
            .await
            .into_iter()
            .map(SessionView::from)
            .collect(),
    )
}

pub(crate) async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SessionView>> {
    state
        .sessions
        .get(&id)
        .await
        .map(SessionView::from)
        .map(Json)
        .ok_or_else(|| CtlError::NotFound(format!("session {id}")))
}

pub(crate) async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let Some(session) = state.sessions.remove(&id).await else {
        return Err(CtlError::NotFound(format!("session {id}")));
    };
    // Revoke every phantom token we issued for this session.
    state.tokens.revoke_session(&session.id).await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- audit ----------

#[derive(Debug, Deserialize)]
pub(crate) struct AuditQuery {
    #[serde(default)]
    limit: Option<usize>,
}

pub(crate) async fn list_audit(
    State(state): State<AppState>,
    Query(q): Query<AuditQuery>,
) -> Json<AuditListResponse> {
    let limit = q.limit.unwrap_or(100).min(1000);
    let rows = state.audit.recent(limit).await;
    let total = state.audit.total().await;
    Json(AuditListResponse { rows, total })
}

#[derive(Debug, Serialize)]
pub(crate) struct AuditListResponse {
    rows: Vec<AuditRow>,
    total: u64,
}

// ---------- exec ----------

#[derive(Debug, Deserialize, Default)]
pub(crate) struct ExecOptions {
    /// Network policy. Omit (or `{"mode":"none"}`) for no network.
    #[serde(default)]
    network: Option<NetworkSpec>,
    /// Seccomp level. "disabled" is intentionally not exposed.
    #[serde(default)]
    seccomp: Option<SeccompSpec>,
    /// CPU quota (100 = one full core). Clamped to 1..=800.
    #[serde(default)]
    cpu_percent: Option<u64>,
    /// PID cap. Clamped to 1..=1024.
    #[serde(default)]
    max_pids: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub(crate) enum NetworkSpec {
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
pub(crate) enum SeccompSpec {
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
    code: String,
    #[serde(default = "default_language")]
    language: String,
    timeout_secs: Option<u64>,
    memory_mb: Option<u64>,
    #[serde(flatten)]
    options: ExecOptions,
}

fn default_language() -> String {
    "javascript".into()
}

/// Resolve a language string to `(file, command)` for the jailed runtime.
fn language_runtime(lang: &str) -> Result<(&'static str, &'static str)> {
    match lang {
        "javascript" | "js" => Ok(("main.js", "/usr/bin/node")),
        "python" | "py"     => Ok(("main.py", "/usr/bin/python3")),
        "bash" | "sh"       => Ok(("main.sh", "/bin/sh")),
        other => Err(CtlError::BadRequest(format!("unsupported language: {other}"))),
    }
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub(crate) struct StatsResponse {
    memory_peak_bytes: u64,
    cpu_usage_usec: u64,
    io_read_bytes: u64,
    io_write_bytes: u64,
}

fn output_to_response(output: agentjail::Output) -> ExecResponse {
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
    let result = jail.run(&req.cmd, &args_refs).await?;

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

    std::fs::write(source.path().join(filename), &req.code).map_err(CtlError::Io)?;

    let run_env = vec![("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into())];
    let config = jail_config(source.path(), output_dir.path(), memory, timeout, run_env, &req.options)?;

    let jail = agentjail::Jail::new(config)?;
    let result = jail.run(cmd, &[&format!("/workspace/{filename}")]).await?;

    tracing::info!(
        language = %req.language,
        exit_code = result.exit_code,
        duration_ms = result.duration.as_millis() as u64,
        timed_out = result.timed_out,
        "run completed"
    );

    Ok((StatusCode::CREATED, Json(output_to_response(result))))
}

// ---------- stream (SSE) ----------

/// `POST /v1/runs/stream` — run code and stream stdout/stderr lines as SSE.
///
/// Emits:
///   event: started    data: {"pid": ...}
///   event: stdout     data: <line>
///   event: stderr     data: <line>
///   event: completed  data: {"exit_code":..,"duration_ms":..,"timed_out":..,"oom_killed":..,
///                            "memory_peak_bytes":..}
///
/// The response closes right after `completed`.
pub(crate) async fn create_stream_run(
    State(state): State<AppState>,
    Json(req): Json<RunRequest>,
) -> Result<axum::response::sse::Sse<
    futures::stream::BoxStream<
        'static,
        std::result::Result<axum::response::sse::Event, std::convert::Infallible>,
    >,
>> {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use futures::StreamExt;

    let exec_cfg = state.exec_config.as_ref()
        .ok_or_else(|| CtlError::Internal("exec not enabled".into()))?;

    if req.code.len() > 1024 * 1024 {
        return Err(CtlError::BadRequest("code exceeds 1 MB".into()));
    }
    let timeout = req.timeout_secs.unwrap_or(exec_cfg.default_timeout_secs).clamp(1, 3600);
    let memory  = req.memory_mb.unwrap_or(exec_cfg.default_memory_mb).min(8192);

    let permit = state.exec_semaphore.clone().try_acquire_owned()
        .map_err(|_| CtlError::BadRequest("too many concurrent executions".into()))?;

    let (filename, cmd) = language_runtime(&req.language)?;

    let source_dir = tempfile::tempdir().map_err(CtlError::Io)?;
    let output_dir = tempfile::tempdir().map_err(CtlError::Io)?;
    std::fs::write(source_dir.path().join(filename), &req.code).map_err(CtlError::Io)?;

    let run_env = vec![("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into())];
    let config  = jail_config(source_dir.path(), output_dir.path(), memory, timeout, run_env, &req.options)?;

    let jail = agentjail::Jail::new(config)?;
    let mut handle = jail.spawn(cmd, &[&format!("/workspace/{filename}")])?;
    let pid: u32 = handle.pid().as_raw();

    let started_guard = state.exec_metrics.clone().start_owned();

    // Keep tempdirs + permit alive for the whole stream lifetime.
    let keepalive = (permit, started_guard, source_dir, output_dir);

    let stream = async_stream::stream! {
        // 1. started
        let started_payload = serde_json::json!({ "pid": pid });
        if let Ok(ev) = Event::default().event("started").json_data(started_payload) {
            yield Ok(ev);
        }

        // 2. drain stdout + stderr line-by-line until both EOF
        let mut stdout_done = false;
        let mut stderr_done = false;
        while !stdout_done || !stderr_done {
            tokio::select! {
                biased;
                line = handle.stdout.read_line(), if !stdout_done => {
                    match line {
                        Some(l) => yield Ok(Event::default().event("stdout").data(trim_nl(l))),
                        None    => { stdout_done = true; }
                    }
                }
                line = handle.stderr.read_line(), if !stderr_done => {
                    match line {
                        Some(l) => yield Ok(Event::default().event("stderr").data(trim_nl(l))),
                        None    => { stderr_done = true; }
                    }
                }
            }
        }

        // 3. wait for exit + collect stats
        let output = handle.wait().await;
        let ev = match output {
            Ok(o) => {
                let payload = serde_json::json!({
                    "exit_code":         o.exit_code,
                    "duration_ms":       u64::try_from(o.duration.as_millis()).unwrap_or(u64::MAX),
                    "timed_out":         o.timed_out,
                    "oom_killed":        o.oom_killed,
                    "memory_peak_bytes": o.stats.as_ref().map(|s| s.memory_peak_bytes).unwrap_or(0),
                    "cpu_usage_usec":    o.stats.as_ref().map(|s| s.cpu_usage_usec).unwrap_or(0),
                });
                Event::default().event("completed").json_data(payload)
            }
            Err(e) => Event::default().event("error").json_data(
                serde_json::json!({ "message": e.to_string() })
            ),
        };
        if let Ok(ev) = ev { yield Ok(ev); }

        drop(keepalive);
    };

    Ok(Sse::new(stream.boxed()).keep_alive(KeepAlive::default()))
}

fn trim_nl(mut s: String) -> String {
    if s.ends_with('\n') { s.pop(); }
    if s.ends_with('\r') { s.pop(); }
    s
}

// ---------- fork ----------

/// One-shot live-fork demo: spawn parent, COW-clone the output mid-run,
/// then spawn child against the forked state. Returns both results and
/// the ForkInfo (clone duration, bytes copied, reflink method).
#[derive(Debug, Deserialize)]
pub(crate) struct ForkRequest {
    parent_code: String,
    child_code: String,
    #[serde(default = "default_language")]
    language: String,
    /// How long the parent runs before we freeze + fork. Default 200ms.
    #[serde(default)]
    fork_after_ms: Option<u64>,
    timeout_secs: Option<u64>,
    memory_mb: Option<u64>,
    #[serde(flatten)]
    options: ExecOptions,
}

#[derive(Debug, Serialize)]
pub(crate) struct ForkResponse {
    parent: ExecResponse,
    child: ExecResponse,
    fork: ForkMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct ForkMeta {
    clone_ms: u64,
    files_cloned: u64,
    files_cow: u64,
    bytes_cloned: u64,
    method: String,
    was_frozen: bool,
}

impl From<agentjail::ForkInfo> for ForkMeta {
    fn from(f: agentjail::ForkInfo) -> Self {
        Self {
            clone_ms:     u64::try_from(f.clone_duration.as_millis()).unwrap_or(u64::MAX),
            files_cloned: f.files_cloned,
            files_cow:    f.files_cow,
            bytes_cloned: f.bytes_cloned,
            method:       format!("{:?}", f.clone_method).to_lowercase(),
            was_frozen:   f.was_frozen,
        }
    }
}

/// `POST /v1/runs/fork` — run parent, live_fork, run child on fork.
pub(crate) async fn create_fork_run(
    State(state): State<AppState>,
    Json(req): Json<ForkRequest>,
) -> Result<(StatusCode, Json<ForkResponse>)> {
    let exec_cfg = state.exec_config.as_ref()
        .ok_or_else(|| CtlError::Internal("exec not enabled".into()))?;

    if req.parent_code.len() > 1024 * 1024 || req.child_code.len() > 1024 * 1024 {
        return Err(CtlError::BadRequest("code exceeds 1 MB".into()));
    }
    let timeout = req.timeout_secs.unwrap_or(exec_cfg.default_timeout_secs).clamp(1, 3600);
    let memory  = req.memory_mb.unwrap_or(exec_cfg.default_memory_mb).min(8192);
    // Default waits 1.5s so the parent namespaces + chroot + cgroup finish
    // setting up before we freeze and cow_clone. Clamped to 30s.
    let fork_after = req.fork_after_ms.unwrap_or(1500).min(30_000);

    // Two slots: one for parent, one for child, both through the same permit.
    let _permit = state.exec_semaphore.try_acquire()
        .map_err(|_| CtlError::BadRequest("too many concurrent executions".into()))?;
    let _guard = state.exec_metrics.start();

    let (filename, cmd) = language_runtime(&req.language)?;

    // Parent source + output dirs. Write BOTH parent and child files into the
    // same source dir so the forked jail can read child.js via /workspace.
    let source_dir  = tempfile::tempdir().map_err(CtlError::Io)?;
    let parent_out  = tempfile::tempdir().map_err(CtlError::Io)?;
    let fork_parent = std::path::Path::new("child_").to_path_buf();
    let child_name  = format!("child_{filename}");

    std::fs::write(source_dir.path().join(filename), &req.parent_code).map_err(CtlError::Io)?;
    std::fs::write(source_dir.path().join(&child_name), &req.child_code).map_err(CtlError::Io)?;

    let run_env = vec![("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into())];
    let config  = jail_config(source_dir.path(), parent_out.path(), memory, timeout, run_env, &req.options)?;

    let parent_jail = agentjail::Jail::new(config)?;
    let parent_handle = parent_jail.spawn(cmd, &[&format!("/workspace/{filename}")])?;

    // Give parent time to write its checkpoint before we fork.
    tokio::time::sleep(std::time::Duration::from_millis(fork_after)).await;

    // Fork — COW-clone the output mid-run.
    let child_out = tempfile::tempdir().map_err(CtlError::Io)?;
    let (child_jail, fork_info) = parent_jail
        .live_fork(Some(&parent_handle), child_out.path())?;

    // Run child in the fork + await parent in parallel.
    let child_args = vec![format!("/workspace/{child_name}")];
    let child_args_refs: Vec<&str> = child_args.iter().map(|s| s.as_str()).collect();
    let (child_res, parent_res) = tokio::join!(
        child_jail.run(cmd, &child_args_refs),
        parent_handle.wait(),
    );
    let child_output  = child_res?;
    let parent_output = parent_res?;

    // Keep dirs + _ = fork_parent alive; drop at end of scope.
    let _ = fork_parent;
    drop(source_dir);
    drop(parent_out);
    drop(child_out);

    tracing::info!(
        language     = %req.language,
        parent_exit  = parent_output.exit_code,
        child_exit   = child_output.exit_code,
        fork_ms      = fork_info.clone_duration.as_millis() as u64,
        files_cow    = fork_info.files_cow,
        files_cloned = fork_info.files_cloned,
        "fork run completed"
    );

    Ok((
        StatusCode::CREATED,
        Json(ForkResponse {
            parent: output_to_response(parent_output),
            child:  output_to_response(child_output),
            fork:   fork_info.into(),
        }),
    ))
}

/// Build a JailConfig with sensible defaults for API-driven execution.
/// Options are clamped; illegal allowlists return `400`.
fn jail_config(
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

// ---------- error conversion ----------

impl IntoResponse for CtlError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = Json(serde_json::json!({
            "error": self.to_string(),
        }));
        (status, body).into_response()
    }
}
