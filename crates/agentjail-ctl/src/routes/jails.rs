//! Jail history listing + detail.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

use crate::error::{CtlError, Result};
use crate::jails::{JailKind, JailQuery, JailRecord, JailStatus};

use super::AppState;

#[derive(Debug, Deserialize)]
pub(crate) struct JailsQuery {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    /// Case-insensitive substring over label / session_id / error.
    #[serde(default)]
    q: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct JailsList {
    rows:  Vec<JailRecord>,
    total: u64,
    limit: usize,
    offset: usize,
}

pub(crate) async fn list_jails(
    State(state): State<AppState>,
    Query(q):     Query<JailsQuery>,
) -> Json<JailsList> {
    let limit  = q.limit.unwrap_or(50).min(500);
    let offset = q.offset.unwrap_or(0);
    let status = match q.status.as_deref() {
        Some("running")   => Some(JailStatus::Running),
        Some("completed") => Some(JailStatus::Completed),
        Some("error")     => Some(JailStatus::Error),
        _ => None,
    };
    let kind = match q.kind.as_deref() {
        Some("run")    => Some(JailKind::Run),
        Some("exec")   => Some(JailKind::Exec),
        Some("fork")   => Some(JailKind::Fork),
        Some("stream") => Some(JailKind::Stream),
        _ => None,
    };
    let query = JailQuery {
        limit, offset, status, kind,
        q: q.q.clone().filter(|s| !s.trim().is_empty()),
    };
    let (rows, total) = state.jails.page(query).await;
    Json(JailsList { rows, total, limit, offset })
}

pub(crate) async fn get_jail(
    State(state): State<AppState>,
    Path(id):     Path<String>,
) -> Result<Json<JailRecord>> {
    let id: i64 = id.parse().map_err(|_| CtlError::BadRequest("invalid id".into()))?;
    state.jails.get(id).await.map(Json)
        .ok_or_else(|| CtlError::NotFound(format!("jail {id}")))
}
