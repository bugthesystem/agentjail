//! Storage for *real* upstream keys on the host side.
//!
//! Real keys never enter the jail. They live in whatever [`KeyStore`] the
//! operator configures — env, file, keyring, Vault, etc.

use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;

use crate::provider::ServiceId;

/// A wrapper that avoids printing secrets in logs.
#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    /// Wrap a raw string as a secret.
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Reveal the underlying bytes. Call sites should be audited.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Look up the real upstream key for a service.
#[async_trait]
pub trait KeyStore: Send + Sync + 'static {
    /// Return the key for `service`, or `None` if none is configured.
    async fn get(&self, service: ServiceId) -> Option<SecretString>;
}

/// In-memory keystore. Typically populated from env or a config file at boot.
#[derive(Default)]
pub struct InMemoryKeyStore {
    inner: RwLock<HashMap<ServiceId, SecretString>>,
}

impl InMemoryKeyStore {
    /// Create an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or overwrite a key.
    pub fn set(&self, service: ServiceId, key: SecretString) {
        if let Ok(mut g) = self.inner.write() {
            g.insert(service, key);
        }
    }

    /// Remove a key. No-op if it wasn't set.
    pub fn unset(&self, service: ServiceId) {
        if let Ok(mut g) = self.inner.write() {
            g.remove(&service);
        }
    }

    /// Populate from environment variables:
    /// `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`. Missing vars are skipped.
    #[must_use]
    pub fn from_env() -> Self {
        let s = Self::new();
        if let Ok(v) = std::env::var("OPENAI_API_KEY") {
            s.set(ServiceId::OpenAi, SecretString::new(v));
        }
        if let Ok(v) = std::env::var("ANTHROPIC_API_KEY") {
            s.set(ServiceId::Anthropic, SecretString::new(v));
        }
        s
    }
}

#[async_trait]
impl KeyStore for InMemoryKeyStore {
    async fn get(&self, service: ServiceId) -> Option<SecretString> {
        self.inner.read().ok()?.get(&service).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_is_redacted() {
        let s = SecretString::new("sk-abcdef");
        assert_eq!(format!("{s:?}"), "<redacted>");
    }

    #[tokio::test]
    async fn in_memory_roundtrip() {
        let s = InMemoryKeyStore::new();
        s.set(ServiceId::OpenAi, SecretString::new("sk-real"));
        let got = s.get(ServiceId::OpenAi).await.unwrap();
        assert_eq!(got.expose(), "sk-real");
        assert!(s.get(ServiceId::Anthropic).await.is_none());
    }
}
