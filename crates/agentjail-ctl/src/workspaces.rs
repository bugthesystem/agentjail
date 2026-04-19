//! Persistent workspaces — long-lived mount trees that survive across HTTP
//! requests. Each `POST /v1/workspaces/:id/exec` spawns a fresh jail
//! against the same `source` + `output` directories, so filesystem mutations
//! persist between calls.
//!
//! This module defines the domain types ([`Workspace`], [`WorkspaceSpec`])
//! and the [`WorkspaceStore`] trait. The in-memory implementation is here;
//! the Postgres implementation lives in [`crate::db::workspaces_pg`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::{CtlError, Result};

/// A persistent workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    /// Opaque identifier, `wrk_<hex>`.
    pub id: String,
    /// When the workspace was created.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Soft-delete marker; when set, `source_dir`/`output_dir` are gone.
    #[serde(with = "time::serde::rfc3339::option")]
    pub deleted_at: Option<OffsetDateTime>,
    /// Absolute path to the jail's source mount.
    pub source_dir: PathBuf,
    /// Absolute path to the jail's output mount (read-write, persistent).
    pub output_dir: PathBuf,
    /// The full jail config chosen at creation time (network policy,
    /// seccomp, cpu/memory limits, timeouts). Re-applied on every exec.
    pub config: WorkspaceSpec,
    /// Optional provenance: the repo that was cloned into `source_dir`.
    pub git_repo: Option<String>,
    /// Optional git ref (branch/tag/commit).
    pub git_ref: Option<String>,
    /// Human-readable label.
    pub label: Option<String>,
    /// Hostname → backend-URL forwards handled by the gateway listener
    /// (opt-in via `AGENTJAIL_GATEWAY_ADDR`). Empty by default.
    #[serde(default)]
    pub domains: Vec<WorkspaceDomain>,
    /// When the most recent exec started. Drives idle-timeout detection.
    #[serde(with = "time::serde::rfc3339::option", default)]
    pub last_exec_at: Option<OffsetDateTime>,
    /// When the reaper paused this workspace. `None` = active.
    #[serde(with = "time::serde::rfc3339::option", default)]
    pub paused_at: Option<OffsetDateTime>,
    /// Snapshot id the reaper captured before wiping `output_dir`. The
    /// next exec restores from this before running; the field clears on
    /// resume.
    #[serde(default)]
    pub auto_snapshot: Option<String>,
}

/// One (hostname, backend) pair exposed through the agentjail-server
/// gateway listener. The caller supplies `backend_url` — this layer does
/// not discover jail-internal IPs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDomain {
    /// Hostname the gateway matches against the `Host` header (case-
    /// insensitive, no port).
    pub domain: String,
    /// Where to forward matched requests, e.g. `http://10.0.0.5:3000`.
    pub backend_url: String,
}

/// Serializable subset of jail options persisted with a workspace.
///
/// We intentionally keep this slimmer than `agentjail::JailConfig` — the
/// source/output/workdir paths are derived from the workspace id, and
/// environment variables are merged per-exec.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceSpec {
    /// Memory limit in MB.
    pub memory_mb: u64,
    /// Timeout (seconds) applied to each exec.
    pub timeout_secs: u64,
    /// CPU quota percent (100 = one full core).
    pub cpu_percent: u64,
    /// Process count cap.
    pub max_pids: u64,
    /// Network policy tag (`"none" | "loopback" | "allowlist"`).
    pub network_mode: String,
    /// Domains when `network_mode == "allowlist"`.
    #[serde(default)]
    pub network_domains: Vec<String>,
    /// Seccomp profile (`"standard" | "strict"`).
    pub seccomp: String,
    /// When `> 0`, the reaper pauses this workspace after it goes idle for
    /// this many seconds. `0` = never auto-pause.
    #[serde(default)]
    pub idle_timeout_secs: u64,
}

/// Contract for workspace persistence. Mirrors [`crate::SessionStore`] and
/// [`crate::JailStore`] shape so the server can swap an in-memory impl for
/// a Postgres-backed one via `ControlPlane::with_postgres`.
#[async_trait]
pub trait WorkspaceStore: Send + Sync + 'static {
    /// Insert a new workspace. Returns an error if the id already exists.
    async fn insert(&self, ws: Workspace) -> Result<()>;
    /// Fetch by id. Deleted workspaces return `None` (soft-delete).
    async fn get(&self, id: &str) -> Option<Workspace>;
    /// Find the first live workspace that declares `host` in its
    /// `domains` list. Case-insensitive match on the `domain` field.
    /// Used by the gateway listener to route incoming requests.
    async fn by_domain(&self, host: &str) -> Option<(Workspace, WorkspaceDomain)>;
    /// List live workspaces, newest first. `limit` capped at 500.
    async fn list(&self, limit: usize, offset: usize) -> (Vec<Workspace>, u64);
    /// Mark deleted + set `deleted_at = now()`.
    async fn mark_deleted(&self, id: &str) -> Option<Workspace>;
    /// Bump `last_exec_at`. Called on every successful exec dispatch.
    async fn touch(&self, id: &str);
    /// Mark paused with an auto-snapshot id.
    async fn mark_paused(&self, id: &str, auto_snapshot: &str);
    /// Clear `paused_at` + `auto_snapshot` after a restore. Returns the
    /// snapshot id that was cleared (so the caller can delete it, or
    /// re-use it on failure).
    async fn mark_resumed(&self, id: &str) -> Option<String>;
}

/// Generate a new workspace id: `wrk_<24hex>`.
#[must_use]
pub fn new_workspace_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut b);
    format!("wrk_{}", hex::encode(b))
}

/// Per-workspace exclusion for filesystem writes.
///
/// A workspace's `output_dir` is a shared mount across execs. To keep it
/// coherent we hand out one async mutex per workspace id. `try_lock` on
/// this mutex is how the exec + snapshot routes decide whether to run
/// serially or return 409.
///
/// This is ephemeral: the locks live as long as the process does. No
/// cross-restart coherence is needed — the OS file system is the ultimate
/// source of truth.
#[derive(Default)]
pub struct WorkspaceLocks {
    inner: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl WorkspaceLocks {
    /// New, empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get (or create) the mutex associated with a workspace id.
    #[must_use]
    pub fn lock_for(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        g.entry(id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// Drop the mutex slot when a workspace is deleted so maps don't grow
    /// unbounded. Safe to call on a missing id.
    pub fn forget(&self, id: &str) {
        if let Ok(mut g) = self.inner.lock() {
            g.remove(id);
        }
    }
}

/// Tracks the cgroup path of each workspace's currently-running exec.
///
/// The snapshot route reads this to freeze-before-copy when a snapshot is
/// requested mid-exec. An entry exists only for the duration of the
/// in-flight exec; absence means "no freeze needed".
#[derive(Default)]
pub struct ActiveCgroups {
    inner: Mutex<HashMap<String, PathBuf>>,
}

impl ActiveCgroups {
    /// New, empty tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the cgroup path for a workspace's in-flight exec. Overwrites
    /// any stale entry for the same id.
    pub fn insert(&self, workspace_id: &str, cgroup_path: PathBuf) {
        if let Ok(mut g) = self.inner.lock() {
            g.insert(workspace_id.to_string(), cgroup_path);
        }
    }

    /// Drop the record after the exec completes.
    pub fn remove(&self, workspace_id: &str) {
        if let Ok(mut g) = self.inner.lock() {
            g.remove(workspace_id);
        }
    }

    /// Current cgroup path for a workspace, if any exec is in flight.
    #[must_use]
    pub fn get(&self, workspace_id: &str) -> Option<PathBuf> {
        self.inner.lock().ok()?.get(workspace_id).cloned()
    }
}

/// Idle-timeout reaper utilities.
///
/// Periodically scans live workspaces and pauses any that have been
/// idle longer than their per-workspace `idle_timeout_secs`. Pausing
/// captures a snapshot, wipes the output dir, and marks the workspace
/// with the snapshot id; the next exec auto-restores.
pub mod idle {
    use super::{Workspace, WorkspaceStore};
    use crate::snapshots::{SnapshotRecord, SnapshotStore, new_snapshot_id};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use time::OffsetDateTime;

    /// Reaper wiring. All fields are `Arc` / `PathBuf` so the struct can
    /// be cloned into a background task cheaply.
    #[derive(Clone)]
    pub struct IdleReaperConfig {
        /// Workspace store the reaper reads + pauses rows through.
        pub workspaces: Arc<dyn WorkspaceStore>,
        /// Snapshot store used for the auto-snapshot on pause.
        pub snapshots:  Arc<dyn SnapshotStore>,
        /// `<state_dir>/snapshots/<id>/` is where captures land.
        pub state_dir:  PathBuf,
        /// Optional content-addressed pool; when set, auto-snapshots use
        /// the incremental / dedupe path.
        pub pool_dir:   Option<PathBuf>,
        /// How often the reaper runs, in seconds. `0` disables the
        /// sweeper.
        pub tick_secs:  u64,
    }

    /// Run a single reaper pass. Returns the number of workspaces that
    /// were paused. Safe to call concurrently with execs because the
    /// workspace's `last_exec_at` is re-read under the per-row lock that
    /// PG provides; in-memory backends race benignly (worst case: a
    /// near-simultaneous exec touches the row right after we snapshot,
    /// and the next tick unpauses via the restore path).
    pub async fn run_once(cfg: &IdleReaperConfig) -> usize {
        let (rows, _) = cfg.workspaces.list(500, 0).await;
        let now = OffsetDateTime::now_utc();
        let mut paused = 0usize;

        for ws in rows {
            if ws.paused_at.is_some() {
                continue;
            }
            if ws.config.idle_timeout_secs == 0 {
                continue;
            }
            let last = ws.last_exec_at.unwrap_or(ws.created_at);
            let cutoff = now - time::Duration::seconds(ws.config.idle_timeout_secs as i64);
            if last >= cutoff {
                continue;
            }
            match pause_one(cfg, &ws).await {
                Ok(()) => paused += 1,
                Err(e) => {
                    tracing::warn!(
                        workspace_id = %ws.id,
                        error = %e,
                        "idle-reaper: pause failed"
                    );
                }
            }
        }
        paused
    }

    /// Spawn a long-running sweeper. Returns `None` when `tick_secs == 0`
    /// (the reaper is disabled).
    pub fn spawn_sweeper(cfg: IdleReaperConfig) -> Option<tokio::task::JoinHandle<()>> {
        if cfg.tick_secs == 0 {
            return None;
        }
        let tick = std::time::Duration::from_secs(cfg.tick_secs.max(1));
        Some(tokio::spawn(async move {
            let mut iv = tokio::time::interval(tick);
            iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            iv.tick().await; // skip immediate
            loop {
                iv.tick().await;
                let n = run_once(&cfg).await;
                if n > 0 {
                    tracing::info!(paused = n, "workspace idle reaper");
                }
            }
        }))
    }

    async fn pause_one(cfg: &IdleReaperConfig, ws: &Workspace) -> Result<(), String> {
        let snap_id = new_snapshot_id();
        let snap_dir = cfg.state_dir.join("snapshots").join(&snap_id);

        // Snapshot the output dir — freeze isn't needed because the
        // workspace is idle by definition.
        let snap = match cfg.pool_dir.as_deref() {
            Some(pool) => agentjail::Snapshot::create_incremental(&ws.output_dir, &snap_dir, pool)
                .map_err(|e| format!("snapshot create: {e}"))?,
            None => agentjail::Snapshot::create(&ws.output_dir, &snap_dir)
                .map_err(|e| format!("snapshot create: {e}"))?,
        };
        let size_bytes = match cfg.pool_dir.as_deref() {
            Some(_) => agentjail::load_manifest(&snap_dir)
                .map(|m| m.size_bytes())
                .unwrap_or_else(|_| snap.size_bytes()),
            None => snap.size_bytes(),
        };

        let record = SnapshotRecord {
            id: snap_id.clone(),
            workspace_id: Some(ws.id.clone()),
            name: Some(format!("auto:{}", ws.id)),
            created_at: OffsetDateTime::now_utc(),
            path: snap.path().to_path_buf(),
            size_bytes,
        };
        if let Err(e) = cfg.snapshots.insert(record).await {
            let _ = std::fs::remove_dir_all(&snap_dir);
            return Err(format!("snapshot insert: {e}"));
        }

        // Wipe output_dir to reclaim disk now that the snapshot is safe.
        let _ = wipe_dir_contents(&ws.output_dir);

        cfg.workspaces.mark_paused(&ws.id, &snap_id).await;
        Ok(())
    }

    fn wipe_dir_contents(dir: &Path) -> std::io::Result<()> {
        if !dir.exists() {
            return Ok(());
        }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let p = entry.path();
            let ft = entry.file_type()?;
            if ft.is_symlink() {
                std::fs::remove_file(&p)?;
            } else if ft.is_dir() {
                std::fs::remove_dir_all(&p)?;
            } else {
                std::fs::remove_file(&p)?;
            }
        }
        Ok(())
    }
}

// ---------- in-memory impl ----------

/// Default in-memory workspace store. Data is lost on restart; Postgres is
/// the durable choice.
#[derive(Default)]
pub struct InMemoryWorkspaceStore {
    inner: RwLock<HashMap<String, Workspace>>,
}

impl InMemoryWorkspaceStore {
    /// New, empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl WorkspaceStore for InMemoryWorkspaceStore {
    async fn insert(&self, ws: Workspace) -> Result<()> {
        let mut g = self
            .inner
            .write()
            .map_err(|_| CtlError::Internal("workspace store poisoned".into()))?;
        if g.contains_key(&ws.id) {
            return Err(CtlError::Conflict(format!(
                "workspace {} already exists",
                ws.id
            )));
        }
        g.insert(ws.id.clone(), ws);
        Ok(())
    }

    async fn get(&self, id: &str) -> Option<Workspace> {
        let g = self.inner.read().ok()?;
        g.get(id).filter(|w| w.deleted_at.is_none()).cloned()
    }

    async fn by_domain(&self, host: &str) -> Option<(Workspace, WorkspaceDomain)> {
        let g = self.inner.read().ok()?;
        let host_lc = host.to_ascii_lowercase();
        for w in g.values() {
            if w.deleted_at.is_some() {
                continue;
            }
            for d in &w.domains {
                if d.domain.eq_ignore_ascii_case(&host_lc) {
                    return Some((w.clone(), d.clone()));
                }
            }
        }
        None
    }

    async fn list(&self, limit: usize, offset: usize) -> (Vec<Workspace>, u64) {
        let Ok(g) = self.inner.read() else {
            return (Vec::new(), 0);
        };
        let mut live: Vec<Workspace> =
            g.values().filter(|w| w.deleted_at.is_none()).cloned().collect();
        live.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        let total = live.len() as u64;
        let rows = live
            .into_iter()
            .skip(offset)
            .take(limit.max(1).min(500))
            .collect();
        (rows, total)
    }

    async fn mark_deleted(&self, id: &str) -> Option<Workspace> {
        let mut g = self.inner.write().ok()?;
        let ws = g.get_mut(id)?;
        if ws.deleted_at.is_some() {
            return None;
        }
        ws.deleted_at = Some(OffsetDateTime::now_utc());
        Some(ws.clone())
    }

    async fn touch(&self, id: &str) {
        if let Ok(mut g) = self.inner.write()
            && let Some(ws) = g.get_mut(id)
        {
            ws.last_exec_at = Some(OffsetDateTime::now_utc());
        }
    }

    async fn mark_paused(&self, id: &str, auto_snapshot: &str) {
        if let Ok(mut g) = self.inner.write()
            && let Some(ws) = g.get_mut(id)
        {
            ws.paused_at = Some(OffsetDateTime::now_utc());
            ws.auto_snapshot = Some(auto_snapshot.to_string());
        }
    }

    async fn mark_resumed(&self, id: &str) -> Option<String> {
        let mut g = self.inner.write().ok()?;
        let ws = g.get_mut(id)?;
        let snap = ws.auto_snapshot.take();
        ws.paused_at = None;
        snap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> Workspace {
        Workspace {
            id: id.into(),
            created_at: OffsetDateTime::now_utc(),
            deleted_at: None,
            source_dir: PathBuf::from(format!("/tmp/wrk/{id}/source")),
            output_dir: PathBuf::from(format!("/tmp/wrk/{id}/output")),
            config: WorkspaceSpec {
                memory_mb: 512,
                timeout_secs: 60,
                cpu_percent: 100,
                max_pids: 64,
                network_mode: "none".into(),
                network_domains: vec![],
                seccomp: "standard".into(),
                idle_timeout_secs: 0,
            },
            git_repo: None,
            git_ref: None,
            label: None,
            domains: Vec::new(),
            last_exec_at: None,
            paused_at: None,
            auto_snapshot: None,
        }
    }

    #[tokio::test]
    async fn insert_and_get() {
        let store = InMemoryWorkspaceStore::new();
        store.insert(sample("wrk_a")).await.unwrap();
        let fetched = store.get("wrk_a").await.unwrap();
        assert_eq!(fetched.id, "wrk_a");
    }

    #[tokio::test]
    async fn duplicate_insert_is_conflict() {
        let store = InMemoryWorkspaceStore::new();
        store.insert(sample("wrk_a")).await.unwrap();
        let err = store.insert(sample("wrk_a")).await.unwrap_err();
        assert!(matches!(err, CtlError::Conflict(_)));
    }

    #[tokio::test]
    async fn soft_delete_hides_from_get_and_list() {
        let store = InMemoryWorkspaceStore::new();
        store.insert(sample("wrk_a")).await.unwrap();
        store.insert(sample("wrk_b")).await.unwrap();
        store.mark_deleted("wrk_a").await.unwrap();

        assert!(store.get("wrk_a").await.is_none());
        assert!(store.get("wrk_b").await.is_some());
        let (rows, total) = store.list(100, 0).await;
        assert_eq!(total, 1);
        assert_eq!(rows[0].id, "wrk_b");
    }

    #[tokio::test]
    async fn list_pagination_is_stable() {
        let store = InMemoryWorkspaceStore::new();
        for i in 0..5 {
            let mut ws = sample(&format!("wrk_{i}"));
            ws.created_at = OffsetDateTime::now_utc() + time::Duration::seconds(i as i64);
            store.insert(ws).await.unwrap();
        }
        let (rows, total) = store.list(2, 1).await;
        assert_eq!(total, 5);
        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn idle_pause_resume_cycle() {
        let store = InMemoryWorkspaceStore::new();
        store.insert(sample("wrk_a")).await.unwrap();

        store.touch("wrk_a").await;
        let w = store.get("wrk_a").await.unwrap();
        assert!(w.last_exec_at.is_some());
        assert!(w.paused_at.is_none());

        store.mark_paused("wrk_a", "snap_auto_1").await;
        let w = store.get("wrk_a").await.unwrap();
        assert!(w.paused_at.is_some());
        assert_eq!(w.auto_snapshot.as_deref(), Some("snap_auto_1"));

        let snap = store.mark_resumed("wrk_a").await;
        assert_eq!(snap.as_deref(), Some("snap_auto_1"));
        let w = store.get("wrk_a").await.unwrap();
        assert!(w.paused_at.is_none());
        assert!(w.auto_snapshot.is_none());
    }
}
