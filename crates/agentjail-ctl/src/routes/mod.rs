//! HTTP route handlers, split by resource.
//!
//! Each submodule owns the request/response types, the handler(s), and any
//! helpers that are only used by that resource. Shared execution plumbing
//! (jail config, run_monitored, git_clone, the common request/response
//! types) lives in [`exec`] and is re-used by [`stream`] and [`fork`].

use std::sync::Arc;

use agentjail_phantom::{InMemoryKeyStore, TokenStore};
use axum::Json;
use axum::response::{IntoResponse, Response};

use crate::audit::AuditStore;
use crate::credential::CredentialStore;
use crate::error::CtlError;
use crate::jails::JailStore;
use crate::session::SessionStore;

mod audit;
mod credentials;
mod exec;
mod fork;
mod health;
mod jails;
mod sessions;
mod stream;

pub(crate) use audit::list_audit;
pub(crate) use credentials::{delete_credential, list_credentials, put_credential};
pub(crate) use exec::{create_run, exec_in_session};
pub(crate) use fork::create_fork_run;
pub(crate) use health::{healthz, stats};
pub(crate) use jails::{get_jail, list_jails};
pub(crate) use sessions::{create_session, delete_session, get_session, list_sessions};
pub(crate) use stream::create_stream_run;

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
    pub(crate) jails: Arc<dyn JailStore>,
}

impl IntoResponse for CtlError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = Json(serde_json::json!({
            "error": self.to_string(),
        }));
        (status, body).into_response()
    }
}
