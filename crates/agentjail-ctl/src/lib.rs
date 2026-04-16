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
mod error;
mod routes;
mod session;

use std::sync::Arc;

use agentjail_phantom::{InMemoryKeyStore, TokenStore};
use axum::routing::{delete, get, post};
use axum::{Router, middleware};
use routes::AppState;
use tower_http::trace::TraceLayer;

pub use audit::{AuditRow, AuditStore, AuditStoreSink, InMemoryAuditStore};
pub use auth::ApiKeys;
pub use credential::{CredentialRecord, CredentialStore, InMemoryCredentialStore};
pub use error::{CtlError, Result};
pub use session::{InMemorySessionStore, Session, SessionStore};

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
}

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
        let proxy_base_url = config.proxy_base_url.trim_end_matches('/').to_string();
        let state = AppState {
            tokens: config.tokens,
            keys: config.keys,
            sessions,
            credentials,
            audit,
            proxy_base_url,
        };
        Self {
            state,
            api_keys: ApiKeys::new(config.api_keys),
        }
    }

    /// Build the axum router.
    pub fn router(self) -> Router {
        let public = Router::new().route("/healthz", get(routes::healthz));

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
            .route("/v1/audit", get(routes::list_audit))
            .layer(middleware::from_fn_with_state(
                self.api_keys.clone(),
                auth::require_api_key,
            ))
            .with_state(self.state.clone());

        public.merge(guarded).layer(TraceLayer::new_for_http())
    }
}
