//! Workspaces — persistent mount trees with multi-exec.
//!
//! Each workspace has a `source` dir (seeded from a git repo or empty) and
//! a read-write `output` dir under `AGENTJAIL_STATE_DIR`. Mutations persist
//! across [`exec`] calls and can be snapshot'd + rehydrated into a fresh
//! workspace via [`crate::routes::snapshots`].
//!
//! Concurrency: per-workspace mutex via [`WorkspaceLocks`]. Only one exec
//! runs at a time against a given workspace; simultaneous requests return
//! `409 Conflict`.

use std::path::Path;

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::error::{CtlError, Result};
use crate::jails::JailKind;
use crate::workspaces::{Workspace, WorkspaceSpec, new_workspace_id};

use super::AppState;
use super::exec::{
    CgroupRegistration, ExecOptions, ExecResponse, GitSpec, NetworkSpec, SeccompSpec, git_clone,
    jail_config, output_to_response, run_monitored_with,
};

// ---------- request / response shapes ----------

#[derive(Debug, Deserialize)]
pub(crate) struct CreateWorkspaceRequest {
    /// Optional git repo to clone into the workspace's source dir on
    /// creation. See [`super::exec::GitSpec`] for the allowed shape.
    #[serde(default)]
    git: Option<GitSpec>,
    /// Human-readable tag shown in the dashboard.
    #[serde(default)]
    label: Option<String>,
    /// Memory cap (MB); clamped to `exec_config.default_memory_mb..=8192`.
    #[serde(default)]
    memory_mb: Option<u64>,
    /// Exec timeout (seconds); clamped to `1..=3600`.
    #[serde(default)]
    timeout_secs: Option<u64>,
    /// Auto-pause after this many seconds of inactivity. 0/unset = never
    /// auto-pause. The idle reaper captures a snapshot + wipes the
    /// output dir on pause; the next exec auto-restores before running.
    #[serde(default)]
    idle_timeout_secs: Option<u64>,
    /// Inbound hostname → backend-URL forwards served by the gateway
    /// listener (when `AGENTJAIL_GATEWAY_ADDR` is set on the server).
    #[serde(default)]
    domains: Option<Vec<crate::workspaces::WorkspaceDomain>>,
    #[serde(default, flatten)]
    options: ExecOptions,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkspaceListQuery {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WorkspaceList {
    rows: Vec<Workspace>,
    total: u64,
    limit: usize,
    offset: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkspaceExecRequest {
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
    /// Per-exec override; falls back to the workspace's default timeout.
    #[serde(default)]
    timeout_secs: Option<u64>,
    /// Per-exec override; falls back to the workspace's default memory cap.
    #[serde(default)]
    memory_mb: Option<u64>,
    #[serde(default)]
    env: Vec<(String, String)>,
}

// ---------- handlers ----------

/// `POST /v1/workspaces`
#[tracing::instrument(
    name = "workspace.create",
    skip_all,
    fields(
        git_repo = req.git.as_ref().map(|g| g.repo.as_str()).unwrap_or(""),
        label = req.label.as_deref().unwrap_or(""),
        idle_secs = req.idle_timeout_secs.unwrap_or(0),
    ),
)]
pub(crate) async fn create_workspace(
    State(state): State<AppState>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> Result<(StatusCode, Json<Workspace>)> {
    let exec_cfg = state.exec_config.as_ref()
        .ok_or_else(|| CtlError::Internal("exec not enabled".into()))?;

    let memory_mb = req.memory_mb.unwrap_or(exec_cfg.default_memory_mb).min(8192);
    let timeout   = req.timeout_secs.unwrap_or(exec_cfg.default_timeout_secs).clamp(1, 3600);
    let idle      = req.idle_timeout_secs.unwrap_or(0);

    let spec = options_to_spec(&req.options, memory_mb, timeout, idle)?;

    let id = new_workspace_id();
    let ws_root = state.state_dir.join("workspaces").join(&id);
    let source_dir = ws_root.join("source");
    let output_dir = ws_root.join("output");
    std::fs::create_dir_all(&source_dir).map_err(CtlError::Io)?;
    std::fs::create_dir_all(&output_dir).map_err(CtlError::Io)?;

    // Optional git clone happens outside the jail, before first exec.
    let (git_repo, git_ref_value) = if let Some(g) = &req.git {
        git_clone(g, &source_dir).await.inspect_err(|_| {
            let _ = std::fs::remove_dir_all(&ws_root);
        })?;
        (Some(g.repo.clone()), g.git_ref.clone())
    } else {
        (None, None)
    };

    let ws = Workspace {
        id: id.clone(),
        created_at: time::OffsetDateTime::now_utc(),
        deleted_at: None,
        source_dir,
        output_dir,
        config: spec,
        git_repo,
        git_ref: git_ref_value,
        label: req.label,
        domains: req.domains.unwrap_or_default(),
        last_exec_at: None,
        paused_at: None,
        auto_snapshot: None,
    };
    state.workspaces.insert(ws.clone()).await.inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&ws_root);
    })?;

    Ok((StatusCode::CREATED, Json(ws)))
}

/// `GET /v1/workspaces`
pub(crate) async fn list_workspaces(
    State(state): State<AppState>,
    Query(q): Query<WorkspaceListQuery>,
) -> Json<WorkspaceList> {
    let limit  = q.limit.unwrap_or(50).min(500);
    let offset = q.offset.unwrap_or(0);
    let (rows, total) = state.workspaces.list(limit, offset).await;
    Json(WorkspaceList { rows, total, limit, offset })
}

/// `GET /v1/workspaces/:id`
pub(crate) async fn get_workspace(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Workspace>> {
    state
        .workspaces
        .get(&id)
        .await
        .map(Json)
        .ok_or_else(|| CtlError::NotFound(format!("workspace {id}")))
}

/// `DELETE /v1/workspaces/:id` — soft-deletes + removes on-disk dirs.
/// Snapshots of this workspace keep their FK (ON DELETE SET NULL) so they
/// remain usable.
pub(crate) async fn delete_workspace(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode> {
    let Some(ws) = state.workspaces.mark_deleted(&id).await else {
        return Err(CtlError::NotFound(format!("workspace {id}")));
    };

    // Best-effort on-disk cleanup. If another process has the dir open,
    // the soft-delete in DB is still authoritative.
    let ws_root = state.state_dir.join("workspaces").join(&ws.id);
    if let Err(e) = std::fs::remove_dir_all(&ws_root) {
        tracing::warn!(workspace_id = %ws.id, error = %e, "workspace dir cleanup failed");
    }
    state.workspace_locks.forget(&ws.id);
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/workspaces/:id/exec` — run a command against the workspace's
/// persistent filesystem. Returns `409` if another exec is in flight.
#[tracing::instrument(
    name = "workspace.exec",
    skip_all,
    fields(
        workspace_id = %id,
        cmd = %req.cmd,
    ),
)]
pub(crate) async fn exec_in_workspace(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<WorkspaceExecRequest>,
) -> Result<Json<ExecResponse>> {
    if state.exec_config.is_none() {
        return Err(CtlError::Internal("exec not enabled".into()));
    }

    let ws = state.workspaces.get(&id).await
        .ok_or_else(|| CtlError::NotFound(format!("workspace {id}")))?;

    // Acquire exclusive exec lock on this workspace.
    let lock = state.workspace_locks.lock_for(&ws.id);
    let _guard = lock.try_lock()
        .map_err(|_| CtlError::Conflict(
            "another exec is in flight against this workspace".into(),
        ))?;

    // Auto-resume: if the reaper paused this workspace, restore its
    // auto-snapshot before running. The on-disk dir was emptied at
    // pause time, so we rehydrate here and clear the pause marker.
    if ws.paused_at.is_some()
        && let Some(snap_id) = ws.auto_snapshot.as_deref()
        && let Some(snap) = state.snapshots.get(snap_id).await
    {
        super::snapshots::restore_snapshot_public(
            &snap.path,
            &ws.output_dir,
            state.snapshot_pool_dir.as_deref(),
        )?;
        state.workspaces.mark_resumed(&ws.id).await;
    }

    // Global concurrency gate on the ctl crate.
    let _permit = state.exec_semaphore.try_acquire()
        .map_err(|_| CtlError::BadRequest("too many concurrent executions".into()))?;
    let _metrics = state.exec_metrics.start();

    // Per-call overrides fall back to the workspace defaults.
    let memory_mb = req.memory_mb
        .unwrap_or(ws.config.memory_mb)
        .min(8192);
    let timeout = req.timeout_secs
        .unwrap_or(ws.config.timeout_secs)
        .clamp(1, 3600);

    // Start from canonical PATH; allow the client to append extra env.
    let mut env: Vec<(String, String)> = Vec::with_capacity(req.env.len() + 1);
    env.push(("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into()));
    env.extend(req.env.iter().cloned());

    let options = spec_to_options(&ws.config)?;
    // Workspaces are the freestyle-style "one persistent dir" model —
    // /workspace is read-write so `bun install`, `cargo build`, etc. can
    // mutate the source tree directly.
    let config = jail_config(
        &ws.source_dir, &ws.output_dir, memory_mb, timeout, env, &options,
        /* source_rw */ true,
    )?;
    let jail = agentjail::Jail::new(config)?;
    let args_refs: Vec<&str> = req.args.iter().map(|s| s.as_str()).collect();

    // Bump last_exec_at before spawning so the idle reaper treats this
    // workspace as active even while the exec is running.
    state.workspaces.touch(&ws.id).await;

    let label = format!("workspace:{}/{}", ws.id, req.cmd);
    let rec_id = state.jails.start(JailKind::Workspace, label, None, None).await;
    // Publish the cgroup path for the duration of this exec so a
    // concurrent snapshot can freeze-before-copy. The registration
    // auto-clears on drop.
    let registration = CgroupRegistration::new(state.active_cgroups.clone(), ws.id.clone());
    let result = run_monitored_with(
        &state.jails, rec_id, &jail, &req.cmd, &args_refs, Some(registration),
    ).await;
    let out = match result {
        Ok(o)  => { state.jails.finish(rec_id, &o).await; o }
        Err(e) => { state.jails.error(rec_id, e.to_string()).await; return Err(e.into()); }
    };

    tracing::info!(
        workspace_id = %ws.id,
        cmd = %req.cmd,
        exit_code = out.exit_code,
        duration_ms = out.duration.as_millis() as u64,
        "workspace exec completed"
    );

    Ok(Json(output_to_response(out)))
}

// ---------- helpers ----------

/// Convert a create-workspace options block into the serializable
/// [`WorkspaceSpec`] that gets persisted with the workspace.
fn options_to_spec(
    options: &ExecOptions,
    memory_mb: u64,
    timeout_secs: u64,
    idle_timeout_secs: u64,
) -> Result<WorkspaceSpec> {
    // Validate up front — network-allowlist rules live in exec.rs as
    // `validate_domains`, reached through `jail_config`. Here we just
    // classify the mode tag + preserve the domains list.
    let (network_mode, network_domains) = match options.network.as_ref() {
        None | Some(NetworkSpec::None) => ("none".to_string(), vec![]),
        Some(NetworkSpec::Loopback)    => ("loopback".to_string(), vec![]),
        Some(NetworkSpec::Allowlist { domains }) => ("allowlist".to_string(), domains.clone()),
    };
    let seccomp = match options.seccomp {
        None | Some(SeccompSpec::Standard) => "standard".to_string(),
        Some(SeccompSpec::Strict)          => "strict".to_string(),
    };
    Ok(WorkspaceSpec {
        memory_mb,
        timeout_secs,
        cpu_percent: options.cpu_percent.unwrap_or(100).clamp(1, 800),
        max_pids:    options.max_pids.unwrap_or(64).clamp(1, 1024),
        network_mode,
        network_domains,
        seccomp,
        idle_timeout_secs,
    })
}

/// Inverse of [`options_to_spec`] — the exec path rebuilds an ExecOptions
/// so it can flow through the shared `jail_config` validator.
fn spec_to_options(spec: &WorkspaceSpec) -> Result<ExecOptions> {
    let network = match spec.network_mode.as_str() {
        "none"      => None,
        "loopback"  => Some(NetworkSpec::Loopback),
        "allowlist" => Some(NetworkSpec::Allowlist {
            domains: spec.network_domains.clone(),
        }),
        other => {
            return Err(CtlError::Internal(format!(
                "workspace has unknown network_mode: {other}"
            )));
        }
    };
    let seccomp = match spec.seccomp.as_str() {
        "standard" => Some(SeccompSpec::Standard),
        "strict"   => Some(SeccompSpec::Strict),
        other => {
            return Err(CtlError::Internal(format!(
                "workspace has unknown seccomp: {other}"
            )));
        }
    };
    Ok(ExecOptions {
        network,
        seccomp,
        cpu_percent: Some(spec.cpu_percent),
        max_pids:    Some(spec.max_pids),
    })
}

/// Best-effort startup sweep: drop rows whose dirs no longer exist on disk.
/// Called from `ControlPlane::with_all_stores` once the store is wired up.
pub(crate) async fn reconcile_on_startup(
    store: &dyn crate::workspaces::WorkspaceStore,
    state_dir: &Path,
) {
    let (rows, _) = store.list(500, 0).await;
    for ws in rows {
        let expected = state_dir.join("workspaces").join(&ws.id);
        if !expected.exists() {
            tracing::warn!(workspace_id = %ws.id, "workspace dir missing — marking deleted");
            let _ = store.mark_deleted(&ws.id).await;
        }
    }
}

