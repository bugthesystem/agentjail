//! Session lifecycle — issues phantom tokens per service.

use std::collections::HashMap;
use std::time::Duration;

use agentjail_phantom::{KeyStore, PathGlob, Scope, ServiceId};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::{CtlError, Result};
use crate::session::{Session, new_session_id};

use super::AppState;

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
