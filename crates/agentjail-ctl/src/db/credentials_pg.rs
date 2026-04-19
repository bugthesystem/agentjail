//! Postgres-backed implementation of `CredentialStore` + a helper to
//! rehydrate the phantom `KeyStore` from the table at startup.

use agentjail_phantom::{InMemoryKeyStore, SecretString, ServiceId};
use async_trait::async_trait;
use sqlx::{PgPool, Row};
use std::sync::Arc;
use time::OffsetDateTime;

use crate::credential::{CredentialRecord, CredentialStore};

/// Postgres-backed credential metadata store. The *secret* itself also
/// lives in this table so we can rehydrate the in-memory `KeyStore` on
/// server restart (the phantom proxy reads keys on the hot path from
/// memory, not from DB).
pub struct PgCredentialStore {
    pool: PgPool,
}

impl PgCredentialStore {
    /// New store over an open pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

}

fn parse_service(s: &str) -> Option<ServiceId> {
    match s {
        "openai"    => Some(ServiceId::OpenAi),
        "anthropic" => Some(ServiceId::Anthropic),
        "github"    => Some(ServiceId::GitHub),
        "stripe"    => Some(ServiceId::Stripe),
        _ => None,
    }
}

fn row_to_record(row: &sqlx::postgres::PgRow) -> Option<CredentialRecord> {
    Some(CredentialRecord {
        service:     parse_service(row.get::<&str, _>("service"))?,
        fingerprint: row.get::<String, _>("fingerprint"),
        added_at:    row.get::<OffsetDateTime, _>("added_at"),
        updated_at:  row.get::<OffsetDateTime, _>("updated_at"),
    })
}

#[async_trait]
impl CredentialStore for PgCredentialStore {
    async fn upsert(&self, _rec: CredentialRecord) {
        // Handlers should call `upsert_with_secret`; this fallback exists to
        // satisfy the trait (in-memory still calls the metadata-only path).
    }

    async fn upsert_with_secret(&self, rec: CredentialRecord, secret: &str) {
        // Single source of truth: the DB. The row's added_at preserves the
        // original insert time on conflict, updated_at always bumps to now.
        let _ = sqlx::query(
            "INSERT INTO credentials (service, secret, fingerprint, added_at, updated_at)
             VALUES ($1, $2, $3, now(), now())
             ON CONFLICT (service) DO UPDATE
               SET secret = EXCLUDED.secret,
                   fingerprint = EXCLUDED.fingerprint,
                   updated_at = now()",
        )
        .bind(rec.service.name())
        .bind(secret)
        .bind(&rec.fingerprint)
        .execute(&self.pool)
        .await;
    }

    async fn remove(&self, service: ServiceId) -> Option<CredentialRecord> {
        let row = sqlx::query(
            "DELETE FROM credentials WHERE service = $1
             RETURNING service, fingerprint, added_at, updated_at",
        )
        .bind(service.name())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()?;

        row_to_record(&row)
    }

    async fn list(&self) -> Vec<CredentialRecord> {
        let rows = sqlx::query(
            "SELECT service, fingerprint, added_at, updated_at
             FROM credentials ORDER BY service",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        rows.iter().filter_map(row_to_record).collect()
    }

    async fn get(&self, service: ServiceId) -> Option<CredentialRecord> {
        let row = sqlx::query(
            "SELECT service, fingerprint, added_at, updated_at
             FROM credentials WHERE service = $1",
        )
        .bind(service.name())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()?;
        row_to_record(&row)
    }
}

/// Load every persisted credential from the DB and install the secrets in
/// the phantom proxy's key store. Call this once at startup after
/// connecting the pool.
pub async fn rehydrate_keystore(
    pool: &PgPool,
    keys: &Arc<InMemoryKeyStore>,
) -> Result<usize, sqlx::Error> {
    let rows = sqlx::query("SELECT service, secret FROM credentials")
        .fetch_all(pool)
        .await?;

    let mut loaded = 0;
    for row in &rows {
        let svc_str: &str = row.get("service");
        let secret: String = row.get("secret");
        if let Some(svc) = parse_service(svc_str) {
            keys.set(svc, SecretString::new(secret));
            loaded += 1;
        }
    }
    Ok(loaded)
}
