//! Audit log read endpoint.

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};

use crate::audit::AuditRow;

use super::AppState;

#[derive(Debug, Deserialize)]
pub(crate) struct AuditQuery {
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AuditListResponse {
    rows: Vec<AuditRow>,
    total: u64,
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
