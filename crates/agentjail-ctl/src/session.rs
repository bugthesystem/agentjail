//! Session storage + domain types.

use std::collections::HashMap;
use std::sync::RwLock;

use agentjail_phantom::ServiceId;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// A session is one issued-to-a-sandbox bundle of phantom credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Opaque identifier, `sess_<hex>`.
    pub id: String,
    /// When the session was created.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// When the session's tokens expire. `None` = no expiry.
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
    /// Services the session has phantom tokens for.
    pub services: Vec<ServiceId>,
    /// The environment variable map handed to the sandbox.
    /// Keys: `OPENAI_API_KEY`, `OPENAI_BASE_URL`, ...
    pub env: HashMap<String, String>,
}

/// Storage for sessions. Safe for concurrent use.
#[async_trait::async_trait]
pub trait SessionStore: Send + Sync + 'static {
    /// Persist a new session. Returns an error if the id is already used.
    async fn insert(&self, session: Session) -> crate::Result<()>;
    /// Fetch by id.
    async fn get(&self, id: &str) -> Option<Session>;
    /// Return every session, ordered newest first.
    async fn list(&self) -> Vec<Session>;
    /// Remove and return the session.
    async fn remove(&self, id: &str) -> Option<Session>;
}

/// In-memory implementation. Lost on restart.
#[derive(Default)]
pub struct InMemorySessionStore {
    inner: RwLock<HashMap<String, Session>>,
}

impl InMemorySessionStore {
    /// New, empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl SessionStore for InMemorySessionStore {
    async fn insert(&self, session: Session) -> crate::Result<()> {
        let mut g = self
            .inner
            .write()
            .map_err(|_| crate::CtlError::Internal("session store poisoned".into()))?;
        if g.contains_key(&session.id) {
            return Err(crate::CtlError::Conflict(format!(
                "session {} already exists",
                session.id
            )));
        }
        g.insert(session.id.clone(), session);
        Ok(())
    }

    async fn get(&self, id: &str) -> Option<Session> {
        self.inner.read().ok()?.get(id).cloned()
    }

    async fn list(&self) -> Vec<Session> {
        let Ok(g) = self.inner.read() else {
            return Vec::new();
        };
        let mut v: Vec<Session> = g.values().cloned().collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }

    async fn remove(&self, id: &str) -> Option<Session> {
        self.inner.write().ok()?.remove(id)
    }
}

/// Random 12-byte session id rendered as `sess_<24hex>`.
#[must_use]
pub fn new_session_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut b);
    format!("sess_{}", hex::encode(b))
}
