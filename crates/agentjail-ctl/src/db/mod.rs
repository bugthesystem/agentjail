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
mod snapshots_pg;
mod workspaces_pg;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

pub use audit_pg::PgAuditStore;
pub use credentials_pg::{PgCredentialStore, rehydrate_keystore};
pub use jails_pg::PgJailStore;
pub use snapshots_pg::PgSnapshotStore;
pub use workspaces_pg::PgWorkspaceStore;

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
    sqlx::raw_sql(include_str!("../../migrations/0002_workspaces_snapshots.sql"))
        .execute(&pool)
        .await?;
    sqlx::raw_sql(include_str!("../../migrations/0003_workspace_idle.sql"))
        .execute(&pool)
        .await?;
    sqlx::raw_sql(include_str!("../../migrations/0004_workspace_domains.sql"))
        .execute(&pool)
        .await?;

    Ok(pool)
}
