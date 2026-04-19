//! # agentjail-ctl
//!
//! HTTP control plane for `agentjail` + `agentjail-phantom`.
//!
//! Provides a small REST surface for:
//! - Attaching real upstream credentials (OpenAI, Anthropic, ...).
//! - Creating *sessions* that return a set of phantom-token env vars to
//!   hand to a sandbox.
//! - Listing the phantom-proxy audit log.
//!
//! The jail-execution lifecycle itself is pluggable and intentionally out
//! of scope here — this crate is the credential-broker / session-keeper
//! layer. Run it standalone in front of your existing sandbox, or pair it
//! with `agentjail` for a full platform.
//!
//! ## Quick start
//!
//! ```no_run
//! use std::sync::Arc;
//! use agentjail_ctl::{ControlPlane, ControlPlaneConfig};
//! use agentjail_phantom::{InMemoryTokenStore, InMemoryKeyStore};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let tokens = Arc::new(InMemoryTokenStore::new());
//! let keys = Arc::new(InMemoryKeyStore::new());
//! let ctl = ControlPlane::new(ControlPlaneConfig {
//!     tokens,
//!     keys,
//!     proxy_base_url: "http://10.0.0.1:8443".into(),
//!     api_keys: vec![],
//!     exec: None,
//!     state_dir: None,
//!     snapshot_pool_dir: None,
//! });
//! let router = ctl.router();
//! let listener = tokio::net::TcpListener::bind("0.0.0.0:7000").await?;
//! axum::serve(listener, router).await?;
//! # Ok(()) }
//! ```

#![warn(missing_docs)]

mod audit;
mod auth;
mod credential;
pub mod db;
mod error;
mod exec;
mod jails;
mod routes;
mod sampler;
mod session;
mod snapshots;
mod workspaces;

use std::path::PathBuf;
use std::sync::Arc;

use agentjail_phantom::{InMemoryKeyStore, TokenStore};
use axum::routing::{delete, get, post};
use axum::{Router, middleware};
use routes::AppState;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

pub use audit::{AuditRow, AuditStore, AuditStoreSink, InMemoryAuditStore};
pub use auth::ApiKeys;
pub use credential::{CredentialRecord, CredentialStore, InMemoryCredentialStore};
pub use error::{CtlError, Result};
pub use exec::{ExecConfig, ExecMetrics};
pub use jails::{InMemoryJailStore, JailKind, JailRecord, JailStatus, JailStore};
pub use session::{InMemorySessionStore, Session, SessionStore};
pub use snapshots::{
    InMemorySnapshotStore, SnapshotGcConfig, SnapshotRecord, SnapshotStore, gc as snapshot_gc,
};
pub use workspaces::{
    ActiveCgroups, InMemoryWorkspaceStore, Workspace, WorkspaceDomain, WorkspaceLocks,
    WorkspaceSpec, WorkspaceStore, idle as workspace_idle,
};

/// Configuration for a [`ControlPlane`].
pub struct ControlPlaneConfig {
    /// Underlying phantom token store. Share this with the phantom proxy.
    pub tokens: Arc<dyn TokenStore>,
    /// Real-keys store. Share this with the phantom proxy.
    pub keys: Arc<InMemoryKeyStore>,
    /// Base URL the sandbox uses to reach the phantom proxy.
    ///
    /// Example: `"http://10.0.0.1:8443"`. Must not end with `/`.
    pub proxy_base_url: String,
    /// API keys accepted by the control plane. Empty list disables auth
    /// (useful only for dev and tests).
    pub api_keys: Vec<String>,
    /// Jail execution config. When set, enables `/v1/sessions/:id/exec`
    /// and `/v1/runs`. When `None`, those endpoints return 501.
    pub exec: Option<ExecConfig>,
    /// Root directory for persistent workspace + snapshot data. Created
    /// on startup if missing. Per-resource layout:
    /// `<state_dir>/workspaces/<id>/{source,output}` and
    /// `<state_dir>/snapshots/<id>/`.
    ///
    /// Falls back to `std::env::temp_dir().join("agentjail-state")` when
    /// unset — fine for dev, not for production (tmpfs is wiped at boot).
    pub state_dir: Option<PathBuf>,
    /// When set, snapshots are captured into a shared content-addressed
    /// object pool rooted here (rather than as standalone directory
    /// copies). Enables dedupe across snapshots + near-free restores via
    /// hardlink.
    pub snapshot_pool_dir: Option<PathBuf>,
}

/// Resolve a `state_dir` to an absolute path, creating the `workspaces/`
/// and `snapshots/` subdirs if missing.
fn ensure_state_dir(state_dir: Option<PathBuf>) -> PathBuf {
    let root = state_dir.unwrap_or_else(|| std::env::temp_dir().join("agentjail-state"));
    for sub in ["workspaces", "snapshots"] {
        let p = root.join(sub);
        if let Err(e) = std::fs::create_dir_all(&p) {
            tracing::warn!(path = %p.display(), error = %e, "failed to create state subdir");
        }
    }
    root
}

/// Opaque wrapper for a configured Postgres pool. Passed to
/// [`ControlPlane::with_postgres`] to swap the credential/audit/jail stores
/// to DB-backed implementations.
pub struct Postgres {
    /// Underlying pool. Public so callers can build their own store
    /// implementations (e.g. the server's bespoke audit sink).
    pub pool: sqlx::PgPool,
}

impl Postgres {
    /// Connect + run the embedded migrations.
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = db::connect(database_url).await?;
        Ok(Self { pool })
    }

    /// Populate the given in-memory `KeyStore` from persisted credentials.
    pub async fn rehydrate_keys(
        &self,
        keys: &Arc<InMemoryKeyStore>,
    ) -> Result<usize, sqlx::Error> {
        db::rehydrate_keystore(&self.pool, keys).await
    }
}

// Re-export the rehydration helper so callers can reach it without opening
// the db submodule.
pub use db::rehydrate_keystore;

/// Assembled control plane. Call [`Self::router`] for the axum router.
pub struct ControlPlane {
    state: AppState,
    api_keys: ApiKeys,
}

impl ControlPlane {
    /// Build from a config. Uses in-memory stores for sessions, credentials,
    /// and audit. Use [`Self::with_stores`] if you need to swap in durable
    /// implementations.
    #[must_use]
    pub fn new(config: ControlPlaneConfig) -> Self {
        Self::with_stores(
            config,
            Arc::new(InMemorySessionStore::new()),
            Arc::new(InMemoryCredentialStore::new()),
            Arc::new(InMemoryAuditStore::new()),
        )
    }

    /// Build with explicit store implementations.
    #[must_use]
    pub fn with_stores(
        config: ControlPlaneConfig,
        sessions: Arc<dyn SessionStore>,
        credentials: Arc<dyn CredentialStore>,
        audit: Arc<dyn AuditStore>,
    ) -> Self {
        let jails: Arc<dyn JailStore> = Arc::new(InMemoryJailStore::new());
        let workspaces: Arc<dyn WorkspaceStore> = Arc::new(InMemoryWorkspaceStore::new());
        let snapshots: Arc<dyn SnapshotStore> = Arc::new(InMemorySnapshotStore::new());
        Self::with_all_stores(config, sessions, credentials, audit, jails, workspaces, snapshots)
    }

    /// Build with explicit stores *including* the jail ledger + workspace
    /// registry + snapshot store — the most granular constructor. Used by
    /// the Postgres-backed wiring.
    #[must_use]
    pub fn with_all_stores(
        config: ControlPlaneConfig,
        sessions: Arc<dyn SessionStore>,
        credentials: Arc<dyn CredentialStore>,
        audit: Arc<dyn AuditStore>,
        jails: Arc<dyn JailStore>,
        workspaces: Arc<dyn WorkspaceStore>,
        snapshots: Arc<dyn SnapshotStore>,
    ) -> Self {
        let proxy_base_url = config.proxy_base_url.trim_end_matches('/').to_string();
        let max_concurrent = config.exec.as_ref().map(|e| e.max_concurrent).unwrap_or(16);
        let exec_semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
        let exec_metrics = Arc::new(ExecMetrics::new());
        let state_dir = ensure_state_dir(config.state_dir);
        let snapshot_pool_dir = config.snapshot_pool_dir.map(|p| {
            if let Err(e) = std::fs::create_dir_all(&p) {
                tracing::warn!(path = %p.display(), error = %e,
                    "failed to create snapshot pool dir");
            }
            p
        });
        let workspace_locks = Arc::new(WorkspaceLocks::new());
        let active_cgroups  = Arc::new(ActiveCgroups::new());
        let state = AppState {
            tokens: config.tokens,
            keys: config.keys,
            sessions,
            credentials,
            audit,
            proxy_base_url,
            exec_config: config.exec,
            exec_semaphore,
            exec_metrics,
            jails,
            workspaces,
            workspace_locks,
            active_cgroups,
            snapshots,
            state_dir,
            snapshot_pool_dir,
        };
        Self {
            state,
            api_keys: ApiKeys::new(config.api_keys),
        }
    }

    /// Build with Postgres-backed stores for credentials, audit, jails, and
    /// workspaces. Sessions stay in-memory because they have TTL-based
    /// eviction and they're short-lived. Call `Postgres::rehydrate_keys`
    /// separately before serving traffic if you want to seed the phantom
    /// key store.
    #[must_use]
    pub fn with_postgres(config: ControlPlaneConfig, pg: &Postgres) -> Self {
        let sessions:    Arc<dyn SessionStore>    = Arc::new(InMemorySessionStore::new());
        let credentials: Arc<dyn CredentialStore> = Arc::new(db::PgCredentialStore::new(pg.pool.clone()));
        let audit:       Arc<dyn AuditStore>      = Arc::new(db::PgAuditStore::new(pg.pool.clone()));
        let jails:       Arc<dyn JailStore>       = Arc::new(db::PgJailStore::new(pg.pool.clone()));
        let workspaces:  Arc<dyn WorkspaceStore>  = Arc::new(db::PgWorkspaceStore::new(pg.pool.clone()));
        let snapshots:   Arc<dyn SnapshotStore>   = Arc::new(db::PgSnapshotStore::new(pg.pool.clone()));
        Self::with_all_stores(config, sessions, credentials, audit, jails, workspaces, snapshots)
    }

    /// Best-effort startup reconciliation. Drops workspace rows whose
    /// on-disk dirs have disappeared (e.g. tmpfs was wiped, or a deleted
    /// workspace's row somehow survived). Call after construction.
    pub async fn reconcile(&self) {
        routes::reconcile_workspaces_on_startup(
            self.state.workspaces.as_ref(),
            &self.state.state_dir,
        )
        .await;
    }

    /// Build the axum router.
    pub fn router(self) -> Router {
        let public = Router::new()
            .route("/healthz", get(routes::healthz))
            .route("/v1/stats", get(routes::stats))
            .with_state(self.state.clone());

        let guarded = Router::new()
            .route(
                "/v1/credentials",
                post(routes::put_credential).get(routes::list_credentials),
            )
            .route(
                "/v1/credentials/:service",
                delete(routes::delete_credential),
            )
            .route(
                "/v1/sessions",
                post(routes::create_session).get(routes::list_sessions),
            )
            .route(
                "/v1/sessions/:id",
                get(routes::get_session).delete(routes::delete_session),
            )
            .route("/v1/sessions/:id/exec", post(routes::exec_in_session))
            .route("/v1/runs", post(routes::create_run))
            .route("/v1/runs/fork", post(routes::create_fork_run))
            .route("/v1/runs/stream", post(routes::create_stream_run))
            .route("/v1/audit", get(routes::list_audit))
            .route("/v1/jails", get(routes::list_jails))
            .route("/v1/jails/:id", get(routes::get_jail))
            .route(
                "/v1/workspaces",
                post(routes::create_workspace).get(routes::list_workspaces),
            )
            .route(
                "/v1/workspaces/:id",
                get(routes::get_workspace).delete(routes::delete_workspace),
            )
            .route(
                "/v1/workspaces/:id/exec",
                post(routes::exec_in_workspace),
            )
            .route(
                "/v1/workspaces/:id/snapshot",
                post(routes::create_snapshot),
            )
            .route(
                "/v1/workspaces/from-snapshot",
                post(routes::create_workspace_from_snapshot),
            )
            .route(
                "/v1/snapshots",
                get(routes::list_snapshots),
            )
            .route(
                "/v1/snapshots/:id",
                get(routes::get_snapshot).delete(routes::delete_snapshot),
            )
            .layer(RequestBodyLimitLayer::new(1024 * 1024)) // 1 MB
            .layer(middleware::from_fn_with_state(
                self.api_keys.clone(),
                auth::require_api_key,
            ))
            .with_state(self.state.clone());

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        public.merge(guarded)
            .layer(cors)
            .layer(TraceLayer::new_for_http())
    }
}
