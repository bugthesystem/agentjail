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
}

// ---------- health ----------

pub(crate) async fn healthz() -> &'static str {
    "ok"
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
