//! Snapshot endpoints.
//!
//! - `POST   /v1/workspaces/:id/snapshot`     — capture a named snapshot
//! - `GET    /v1/snapshots`                   — list (optional ?workspace_id=)
//! - `GET    /v1/snapshots/:id`               — detail
//! - `DELETE /v1/snapshots/:id`               — remove
//! - `POST   /v1/workspaces/from-snapshot`    — rehydrate into a new workspace

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::{CtlError, Result};
use crate::snapshots::SnapshotRecord;
pub(super) use crate::snapshots::new_snapshot_id;
use crate::workspaces::{Workspace, new_workspace_id};

use super::AppState;

// ---------- request / response shapes ----------

#[derive(Debug, Deserialize, Default)]
pub(crate) struct CreateSnapshotRequest {
    /// Optional human-readable name for the snapshot.
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SnapshotListQuery {
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SnapshotList {
    rows: Vec<SnapshotRecord>,
    total: u64,
    limit: usize,
    offset: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FromSnapshotRequest {
    /// The snapshot to rehydrate into the new workspace's output dir.
    snapshot_id: String,
    /// Optional label for the new workspace.
    #[serde(default)]
    label: Option<String>,
}

// ---------- handlers ----------

/// `POST /v1/workspaces/:id/snapshot` — captures a snapshot of the
/// workspace's output dir, freezing the running exec (if any) around the
/// copy. This is the *mid-run snapshot* entry point: safe to call during
/// a long-running exec; idle workspaces skip the freeze step.
#[tracing::instrument(
    name = "snapshot.create",
    skip_all,
    fields(
        workspace_id = %id,
        name = req.name.as_deref().unwrap_or(""),
    ),
)]
pub(crate) async fn create_snapshot(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<CreateSnapshotRequest>,
) -> Result<(StatusCode, Json<SnapshotRecord>)> {
    let ws = state.workspaces.get(&id).await
        .ok_or_else(|| CtlError::NotFound(format!("workspace {id}")))?;

    let snap_id = new_snapshot_id();
    let snap_dir = state.state_dir.join("snapshots").join(&snap_id);

    // Workspace state lives in `source_dir` (mounted at `/workspace`
    // read-write); `output_dir` is the artifact drop zone. Snapshot the
    // dir the jail actually mutates.
    let active = state.active_cgroups.get(&ws.id);
    let (snap, size_bytes) = capture_snapshot(
        active.as_deref(),
        &ws.source_dir,
        &snap_dir,
        state.snapshot_pool_dir.as_deref(),
    )?;

    let record = SnapshotRecord {
        id: snap_id.clone(),
        workspace_id: Some(ws.id.clone()),
        name: req.name,
        created_at: OffsetDateTime::now_utc(),
        path: snap.path().to_path_buf(),
        size_bytes,
    };
    state.snapshots.insert(record.clone()).await.inspect_err(|_| {
        // Undo the on-disk copy if we can't persist the row.
        let _ = std::fs::remove_dir_all(&snap_dir);
    })?;

    Ok((StatusCode::CREATED, Json(record)))
}

/// `GET /v1/snapshots`
pub(crate) async fn list_snapshots(
    State(state): State<AppState>,
    Query(q): Query<SnapshotListQuery>,
) -> Json<SnapshotList> {
    let limit  = q.limit.unwrap_or(50).min(500);
    let offset = q.offset.unwrap_or(0);
    let (rows, total) = state
        .snapshots
        .list(q.workspace_id.as_deref(), limit, offset)
        .await;
    Json(SnapshotList { rows, total, limit, offset })
}

/// `GET /v1/snapshots/:id`
pub(crate) async fn get_snapshot(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<SnapshotRecord>> {
    state
        .snapshots
        .get(&id)
        .await
        .map(Json)
        .ok_or_else(|| CtlError::NotFound(format!("snapshot {id}")))
}

/// `DELETE /v1/snapshots/:id`
///
/// Removes the snapshot row and its on-disk dir (which for an
/// incremental snapshot is just the `manifest.json`). Blobs in the
/// content-addressed pool remain until the GC sweeper notices they're
/// unreferenced — we never delete them inline because another snapshot
/// may be mid-capture and about to reference the same hash.
pub(crate) async fn delete_snapshot(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode> {
    let Some(rec) = state.snapshots.remove(&id).await else {
        return Err(CtlError::NotFound(format!("snapshot {id}")));
    };
    if rec.path.exists() {
        if let Err(e) = std::fs::remove_dir_all(&rec.path) {
            tracing::warn!(snapshot_id = %rec.id, error = %e, "snapshot dir cleanup failed");
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/workspaces/from-snapshot` — rehydrate a snapshot into a
/// brand-new workspace. The source dir starts empty (the snapshot lives in
/// the workspace's *output* dir, which is what `live_fork` and
/// `Snapshot::restore_to` write to). Config is inherited from the parent
/// workspace when it's still around.
#[tracing::instrument(
    name = "workspace.from_snapshot",
    skip_all,
    fields(snapshot_id = %req.snapshot_id),
)]
pub(crate) async fn create_workspace_from_snapshot(
    State(state): State<AppState>,
    Json(req): Json<FromSnapshotRequest>,
) -> Result<(StatusCode, Json<Workspace>)> {
    let snap = state.snapshots.get(&req.snapshot_id).await
        .ok_or_else(|| CtlError::NotFound(format!("snapshot {}", req.snapshot_id)))?;

    // Parent workspace may be deleted; if so, we fall back to defaults so
    // the snapshot stays useful even after the parent is gone.
    let parent = match snap.workspace_id.as_deref() {
        Some(pid) => state.workspaces.get(pid).await,
        None => None,
    };

    let new_id = new_workspace_id();
    let ws_root = state.state_dir.join("workspaces").join(&new_id);
    let source_dir = ws_root.join("source");
    let output_dir = ws_root.join("output");
    std::fs::create_dir_all(&source_dir).map_err(CtlError::Io)?;
    std::fs::create_dir_all(&output_dir).map_err(CtlError::Io)?;

    // Restore into the new workspace's `source_dir` — that's the
    // writable surface the jail sees at `/workspace`.
    restore_snapshot(
        &snap.path,
        &source_dir,
        state.snapshot_pool_dir.as_deref(),
    )
    .inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&ws_root);
    })?;

    let config = parent
        .as_ref()
        .map(|p| p.config.clone())
        .unwrap_or_else(default_workspace_spec);

    let ws = Workspace {
        id: new_id.clone(),
        created_at: OffsetDateTime::now_utc(),
        deleted_at: None,
        source_dir,
        output_dir,
        config,
        git_repo: parent.as_ref().and_then(|p| p.git_repo.clone()),
        git_ref:  parent.as_ref().and_then(|p| p.git_ref.clone()),
        label:    req.label.or_else(|| {
            parent.as_ref().map(|_| format!("restored from {}", snap.id))
        }),
        domains:       Vec::new(),
        last_exec_at:  None,
        paused_at:     None,
        auto_snapshot: None,
    };
    state.workspaces.insert(ws.clone()).await.inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&ws_root);
    })?;
    Ok((StatusCode::CREATED, Json(ws)))
}

// ---------- helpers ----------

/// Capture a snapshot, picking the content-addressed path when a pool
/// dir is configured. Handles freeze-around-copy when an exec is live.
///
/// Returns `(engine_snapshot, reported_size_bytes)`. For incremental
/// snapshots the size is the manifest's logical sum; for full copies
/// it's the actual on-disk footprint.
pub(super) fn capture_snapshot(
    cgroup_path: Option<&std::path::Path>,
    output_dir: &std::path::Path,
    snap_dir: &std::path::Path,
    pool_dir: Option<&std::path::Path>,
) -> Result<(agentjail::Snapshot, u64)> {
    match pool_dir {
        Some(pool) => {
            // Incremental: freeze, hash-into-pool, thaw, write manifest.
            let frozen = cgroup_path.and_then(|p| agentjail::freeze_cgroup(p).ok().map(|()| p));
            let snap = agentjail::Snapshot::create_incremental(output_dir, snap_dir, pool);
            if let Some(p) = frozen {
                let _ = agentjail::thaw_cgroup(p);
            }
            let snap = snap.map_err(CtlError::Jail)?;
            let size = agentjail::load_manifest(snap_dir)
                .map(|m| m.size_bytes())
                .unwrap_or_else(|_| snap.size_bytes());
            Ok((snap, size))
        }
        None => {
            let snap = agentjail::snapshot_frozen(cgroup_path, output_dir, snap_dir)
                .map_err(CtlError::Jail)?;
            let size = snap.size_bytes();
            Ok((snap, size))
        }
    }
}

/// Counterpart to [`capture_snapshot`]. Picks full-vs-incremental based
/// on whether the snapshot dir holds a `manifest.json` (authoritative
/// marker regardless of what the server was started with).
pub(super) fn restore_snapshot(
    snap_dir: &std::path::Path,
    target_dir: &std::path::Path,
    pool_dir: Option<&std::path::Path>,
) -> Result<()> {
    let manifest_path = snap_dir.join("manifest.json");
    if manifest_path.exists() {
        let pool = pool_dir.ok_or_else(|| {
            CtlError::BadRequest(
                "snapshot is content-addressed but AGENTJAIL_SNAPSHOT_POOL_DIR is not set".into(),
            )
        })?;
        agentjail::Snapshot::restore_incremental(snap_dir, pool, target_dir)
            .map_err(CtlError::Jail)
    } else {
        let loaded = agentjail::Snapshot::load(snap_dir, target_dir).map_err(CtlError::Jail)?;
        loaded.restore_to(target_dir).map_err(CtlError::Jail)
    }
}

fn default_workspace_spec() -> crate::workspaces::WorkspaceSpec {
    crate::workspaces::WorkspaceSpec {
        memory_mb:         512,
        timeout_secs:      300,
        cpu_percent:       100,
        max_pids:          64,
        network_mode:      "none".into(),
        network_domains:   Vec::new(),
        seccomp:           "standard".into(),
        idle_timeout_secs: 0,
    }
}
