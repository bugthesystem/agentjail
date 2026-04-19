//! Postgres backing for the control plane.
//!
//! All three write-heavy stores (credentials, audit, jails) get a concrete
//! Postgres implementation of their existing trait, so callers don't need
//! to know which backend is in use. Switched on via
//! `ControlPlaneConfig::database_url`; when `None`, the in-memory stores
//! remain the default.

mod audit_pg;
mod credentials_pg;
mod jails_pg;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

pub use audit_pg::PgAuditStore;
pub use credentials_pg::{PgCredentialStore, rehydrate_keystore};
pub use jails_pg::PgJailStore;

/// Connect to Postgres + run the embedded migrations. Idempotent.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(Duration::from_secs(3))
        .connect(database_url)
        .await?;

    sqlx::raw_sql(include_str!("../../migrations/0001_init.sql"))
        .execute(&pool)
        .await?;

    Ok(pool)
}
