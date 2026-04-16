//! API-key authentication middleware.

use std::collections::HashSet;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use subtle::ConstantTimeEq;

/// Collection of valid control-plane API keys. Compared in constant time.
#[derive(Clone, Default)]
pub struct ApiKeys {
    keys: Arc<HashSet<String>>,
}

impl ApiKeys {
    /// Build from a static list. Empty list disables auth (useful in tests).
    #[must_use]
    pub fn new(keys: impl IntoIterator<Item = String>) -> Self {
        Self {
            keys: Arc::new(keys.into_iter().collect()),
        }
    }

    /// No keys configured — all requests accepted. Use only in tests.
    #[must_use]
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Whether any key is configured.
    #[must_use]
    pub fn is_enforced(&self) -> bool {
        !self.keys.is_empty()
    }

    fn matches(&self, presented: &str) -> bool {
        self.keys
            .iter()
            .any(|k| k.as_bytes().ct_eq(presented.as_bytes()).into())
    }
}

/// Middleware: require `Authorization: Bearer <api-key>` when enforced.
pub async fn require_api_key(
    State(keys): State<ApiKeys>,
    headers: HeaderMap,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    if !keys.is_enforced() {
        return next.run(req).await;
    }
    let Some(raw) = headers
        .get(http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return (StatusCode::UNAUTHORIZED, "missing api key").into_response();
    };
    let token = raw.strip_prefix("Bearer ").unwrap_or(raw).trim();
    if !keys.matches(token) {
        return (StatusCode::UNAUTHORIZED, "invalid api key").into_response();
    }
    next.run(req).await
}
