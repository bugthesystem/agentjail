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
use crate::workspaces::{Workspace, WorkspaceSpec, new_workspace_id};

use super::AppState;
use super::exec::{ExecOptions, GitSpec, NetworkSpec, SeccompSpec};
use super::exec_git::git_clone;

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
    /// Case-insensitive substring match on `id` / `label` / `git_repo`.
    #[serde(default)]
    q: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WorkspaceList {
    rows: Vec<Workspace>,
    total: u64,
    limit: usize,
    offset: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ForkWorkspaceRequest {
    /// How many forks to create (1..=16).
    count: usize,
    /// Optional label prefix for the children. Each fork gets
    /// `"{label}-{i}"` (or `"fork of {parent.id}-{i}"` when unset).
    #[serde(default)]
    label: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ForkWorkspaceResponse {
    /// Refreshed parent record (for the caller's convenience).
    parent: Workspace,
    /// New workspaces, in invocation order. Each is fully independent —
    /// its own source+output dirs, its own exec mutex.
    forks: Vec<Workspace>,
    /// The snapshot captured on the parent. Kept around so callers can
    /// `POST /v1/workspaces/from-snapshot` again later if they want more
    /// copies of the same point-in-time state.
    snapshot_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkspaceExecRequest {
    pub(super) cmd: String,
    #[serde(default)]
    pub(super) args: Vec<String>,
    /// Per-exec override; falls back to the workspace's default timeout.
    #[serde(default)]
    pub(super) timeout_secs: Option<u64>,
    /// Per-exec override; falls back to the workspace's default memory cap.
    #[serde(default)]
    pub(super) memory_mb: Option<u64>,
    #[serde(default)]
    pub(super) env: Vec<(String, String)>,
}

// ---------- handlers ----------

/// `POST /v1/workspaces`
#[tracing::instrument(
    name = "workspace.create",
    skip_all,
    fields(
        git_repo = req.git.as_ref().and_then(|g| g.primary().0).unwrap_or_default(),
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
        g.primary()
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

/// `POST /v1/workspaces/:id/fork` — atomic N-way fork of a persistent
/// workspace. Captures a single snapshot of the parent (freezing any
/// in-flight exec for consistency via the engine's `snapshot_frozen`
/// path), then spawns `count` new workspaces restored from that
/// snapshot. Each child is fully independent — own source+output dirs,
/// own exec mutex — so parallel execs against the forks are safe.
#[tracing::instrument(
    name = "workspace.fork",
    skip_all,
    fields(parent_id = %id, count = req.count),
)]
pub(crate) async fn fork_workspace(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<ForkWorkspaceRequest>,
) -> Result<(StatusCode, Json<ForkWorkspaceResponse>)> {
    if req.count == 0 || req.count > 16 {
        return Err(CtlError::BadRequest(
            "fork.count must be 1..=16".into(),
        ));
    }

    let parent = state
        .workspaces
        .get(&id)
        .await
        .ok_or_else(|| CtlError::NotFound(format!("workspace {id}")))?;

    // 1. Capture one snapshot of the parent. Freeze iff an exec is in
    //    flight so the forks get a consistent filesystem view.
    let snap_id = super::snapshots::new_snapshot_id();
    let snap_dir = state.state_dir.join("snapshots").join(&snap_id);
    let active = state.active_cgroups.get(&parent.id);
    let (snap, size_bytes) = super::snapshots::capture_snapshot(
        active.as_deref(),
        &parent.source_dir,
        &snap_dir,
        state.snapshot_pool_dir.as_deref(),
    )?;

    let snap_record = crate::snapshots::SnapshotRecord {
        id: snap_id.clone(),
        workspace_id: Some(parent.id.clone()),
        name: Some(format!("fork-origin:{}", parent.id)),
        created_at: time::OffsetDateTime::now_utc(),
        path: snap.path().to_path_buf(),
        size_bytes,
    };
    state
        .snapshots
        .insert(snap_record)
        .await
        .inspect_err(|_| {
            let _ = std::fs::remove_dir_all(&snap_dir);
        })?;

    // 2. Spawn `count` new workspaces from that snapshot. Each rehydrate
    //    runs sequentially on-host (FS-bound; parallelism gains are
    //    marginal and contention on the pool dir costs more).
    let mut forks: Vec<Workspace> = Vec::with_capacity(req.count);
    for i in 0..req.count {
        let new_id = new_workspace_id();
        let ws_root = state.state_dir.join("workspaces").join(&new_id);
        let source_dir = ws_root.join("source");
        let output_dir = ws_root.join("output");
        std::fs::create_dir_all(&source_dir).map_err(CtlError::Io)?;
        std::fs::create_dir_all(&output_dir).map_err(CtlError::Io)?;

        super::snapshots::restore_snapshot(
            snap.path(),
            &source_dir,
            state.snapshot_pool_dir.as_deref(),
        )
        .inspect_err(|_| {
            let _ = std::fs::remove_dir_all(&ws_root);
        })?;

        let label = match &req.label {
            Some(prefix) => format!("{prefix}-{i}"),
            None => format!("fork of {}-{i}", parent.id),
        };

        let child = Workspace {
            id: new_id.clone(),
            created_at: time::OffsetDateTime::now_utc(),
            deleted_at: None,
            source_dir,
            output_dir,
            config: parent.config.clone(),
            git_repo: parent.git_repo.clone(),
            git_ref: parent.git_ref.clone(),
            label: Some(label),
            domains: Vec::new(),
            last_exec_at: None,
            paused_at: None,
            auto_snapshot: None,
        };
        state
            .workspaces
            .insert(child.clone())
            .await
            .inspect_err(|_| {
                let _ = std::fs::remove_dir_all(&ws_root);
            })?;
        forks.push(child);
    }

    Ok((
        StatusCode::CREATED,
        Json(ForkWorkspaceResponse {
            parent,
            forks,
            snapshot_id: snap_id,
        }),
    ))
}

/// `GET /v1/workspaces`
pub(crate) async fn list_workspaces(
    State(state): State<AppState>,
    Query(q): Query<WorkspaceListQuery>,
) -> Json<WorkspaceList> {
    let limit  = q.limit.unwrap_or(50).min(500);
    let offset = q.offset.unwrap_or(0);
    let needle = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let (rows, total) = state.workspaces.list(limit, offset, needle).await;
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

/// Best-effort startup sweep: drop rows whose dirs no longer exist on disk.
/// Called from `ControlPlane::with_all_stores` once the store is wired up.
pub(crate) async fn reconcile_on_startup(
    store: &dyn crate::workspaces::WorkspaceStore,
    state_dir: &Path,
) {
    let (rows, _) = store.list(500, 0, None).await;
    for ws in rows {
        let expected = state_dir.join("workspaces").join(&ws.id);
        if !expected.exists() {
            tracing::warn!(workspace_id = %ws.id, "workspace dir missing — marking deleted");
            let _ = store.mark_deleted(&ws.id).await;
        }
    }
}

