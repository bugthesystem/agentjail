//! Credential metadata + persistence.
//!
//! The control plane stores two things for each upstream:
//!   1. The *real* key (in the underlying `agentjail-phantom` `KeyStore`).
//!   2. Metadata (service, when it was added, last-rotated-at) for the UI.

use std::collections::HashMap;
use std::sync::RwLock;

use agentjail_phantom::ServiceId;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// User-facing record of a configured credential. Never contains the secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialRecord {
    /// Service this credential is for.
    pub service: ServiceId,
    /// When it was first attached.
    #[serde(with = "time::serde::rfc3339")]
    pub added_at: OffsetDateTime,
    /// When it was last rotated.
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    /// A short, non-reversible fingerprint (`sha256` prefix of the secret)
    /// so the UI can tell "still the same key" vs "got rotated" without
    /// ever holding the secret.
    pub fingerprint: String,
}

/// Metadata-only storage for credentials.
#[async_trait::async_trait]
pub trait CredentialStore: Send + Sync + 'static {
    /// Add or replace the metadata for a service.
    async fn upsert(&self, rec: CredentialRecord);
    /// Remove metadata for a service.
    async fn remove(&self, service: ServiceId) -> Option<CredentialRecord>;
    /// List all records.
    async fn list(&self) -> Vec<CredentialRecord>;
    /// Fetch one.
    async fn get(&self, service: ServiceId) -> Option<CredentialRecord>;
}

/// In-memory implementation.
#[derive(Default)]
pub struct InMemoryCredentialStore {
    inner: RwLock<HashMap<ServiceId, CredentialRecord>>,
}

impl InMemoryCredentialStore {
    /// New, empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl CredentialStore for InMemoryCredentialStore {
    async fn upsert(&self, rec: CredentialRecord) {
        if let Ok(mut g) = self.inner.write() {
            g.insert(rec.service, rec);
        }
    }

    async fn remove(&self, service: ServiceId) -> Option<CredentialRecord> {
        self.inner.write().ok()?.remove(&service)
    }

    async fn list(&self) -> Vec<CredentialRecord> {
        let Ok(g) = self.inner.read() else {
            return Vec::new();
        };
        let mut v: Vec<CredentialRecord> = g.values().cloned().collect();
        v.sort_by(|a, b| a.service.name().cmp(b.service.name()));
        v
    }

    async fn get(&self, service: ServiceId) -> Option<CredentialRecord> {
        self.inner.read().ok()?.get(&service).cloned()
    }
}

/// Short, deterministic, non-reversible fingerprint of a secret.
///
/// Single-pass FNV-1a hash rendered as 16 hex chars. Good enough to show
/// rotation visually in the UI; not a cryptographic primitive.
#[must_use]
pub(crate) fn fingerprint(secret: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in secret.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    format!("{hash:016x}")
}
