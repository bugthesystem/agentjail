//! Credential CRUD — stores provider API keys the phantom proxy resolves.

use agentjail_phantom::{SecretString, ServiceId};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use time::OffsetDateTime;

use crate::credential::{CredentialRecord, fingerprint};
use crate::error::{CtlError, Result};

use super::AppState;

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
    let now = OffsetDateTime::now_utc();
    let existing = state.credentials.get(req.service).await;
    let rec = CredentialRecord {
        service: req.service,
        added_at: existing.as_ref().map_or(now, |r| r.added_at),
        updated_at: now,
        fingerprint: fp,
    };
    // Persist first (DB is the source of truth when enabled), then update
    // the in-memory key the phantom proxy reads on the hot path.
    state.credentials.upsert_with_secret(rec.clone(), &req.secret).await;
    state.keys.set(req.service, SecretString::new(req.secret));
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
        Some(_) => {
            state.keys.unset(svc);
            Ok(StatusCode::NO_CONTENT)
        }
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
