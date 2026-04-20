//! Postgres-backed `SnapshotStore`.

use std::path::PathBuf;

use async_trait::async_trait;
use sqlx::{PgPool, Row};
use time::OffsetDateTime;

use crate::error::{CtlError, Result};
use crate::snapshots::{SnapshotRecord, SnapshotStore};

/// PG-backed snapshot store.
pub struct PgSnapshotStore {
    pool: PgPool,
}

impl PgSnapshotStore {
    /// New store over an open pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_record(row: &sqlx::postgres::PgRow) -> SnapshotRecord {
    SnapshotRecord {
        id:           row.get::<String, _>("id"),
        workspace_id: row.get::<Option<String>, _>("workspace_id"),
        name:         row.get::<Option<String>, _>("name"),
        created_at:   row.get::<OffsetDateTime, _>("created_at"),
        path:         PathBuf::from(row.get::<String, _>("path")),
        size_bytes:   row.get::<i64, _>("size_bytes") as u64,
    }
}

#[async_trait]
impl SnapshotStore for PgSnapshotStore {
    async fn insert(&self, snap: SnapshotRecord) -> Result<()> {
        let r = sqlx::query(
            "INSERT INTO snapshots (id, workspace_id, name, created_at, path, size_bytes)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&snap.id)
        .bind(snap.workspace_id.as_deref())
        .bind(snap.name.as_deref())
        .bind(snap.created_at)
        .bind(snap.path.to_string_lossy().as_ref())
        .bind(snap.size_bytes as i64)
        .execute(&self.pool)
        .await;

        match r {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(dbe)) if dbe.code().as_deref() == Some("23505") => {
                Err(CtlError::Conflict(format!("snapshot {} already exists", snap.id)))
            }
            Err(e) => Err(CtlError::Internal(format!("snapshot insert: {e}"))),
        }
    }

    async fn get(&self, id: &str) -> Option<SnapshotRecord> {
        let row = sqlx::query(
            "SELECT id, workspace_id, name, created_at, path, size_bytes
             FROM snapshots WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()?;
        Some(row_to_record(&row))
    }

    async fn list(
        &self,
        workspace_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> (Vec<SnapshotRecord>, u64) {
        let limit_i = limit.clamp(1, 500) as i64;
        let offset_i = offset as i64;

        // Two branches to avoid binding-count mismatch between filtered
        // and unfiltered queries.
        let (total, rows) = match workspace_id {
            Some(w) => {
                let t: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM snapshots WHERE workspace_id = $1",
                )
                .bind(w)
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0);
                let rows = sqlx::query(
                    "SELECT id, workspace_id, name, created_at, path, size_bytes
                     FROM snapshots
                     WHERE workspace_id = $1
                     ORDER BY created_at DESC
                     LIMIT $2 OFFSET $3",
                )
                .bind(w)
                .bind(limit_i)
                .bind(offset_i)
                .fetch_all(&self.pool)
                .await
                .unwrap_or_default();
                (t, rows)
            }
            None => {
                let t: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM snapshots")
                    .fetch_one(&self.pool)
                    .await
                    .unwrap_or(0);
                let rows = sqlx::query(
                    "SELECT id, workspace_id, name, created_at, path, size_bytes
                     FROM snapshots
                     ORDER BY created_at DESC
                     LIMIT $1 OFFSET $2",
                )
                .bind(limit_i)
                .bind(offset_i)
                .fetch_all(&self.pool)
                .await
                .unwrap_or_default();
                (t, rows)
            }
        };
        let parsed: Vec<SnapshotRecord> = rows.iter().map(row_to_record).collect();
        (parsed, total as u64)
    }

    async fn remove(&self, id: &str) -> Option<SnapshotRecord> {
        let row = sqlx::query(
            "DELETE FROM snapshots WHERE id = $1
             RETURNING id, workspace_id, name, created_at, path, size_bytes",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()?;
        Some(row_to_record(&row))
    }
}
