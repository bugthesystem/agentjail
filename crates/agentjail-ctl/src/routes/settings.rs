//! `GET /v1/config` — read-only snapshot of the running server's
//! configuration.
//!
//! Exists so operators (and the web Settings page) can see what's
//! actually configured without SSH'ing to the host. All fields here are
//! safe to surface: bind addresses, GC thresholds, provider metadata.
//! Secrets (API keys, database URLs, real upstream credentials) are
//! never included.

use axum::{Json, extract::State};
use serde::Serialize;

use super::AppState;
use crate::ProviderInfo;

/// Reply for `GET /v1/config`.
#[derive(Debug, Serialize)]
pub(crate) struct SettingsResponse {
    proxy:         ProxySettings,
    control_plane: ControlPlaneSettings,
    gateway:       Option<GatewaySettings>,
    exec:          Option<ExecSettings>,
    persistence:   PersistenceSettings,
    snapshots:     SnapshotSettings,
}

#[derive(Debug, Serialize)]
struct ProxySettings {
    /// URL the sandbox points at (`PROXY_BASE_URL`).
    base_url: String,
    /// Phantom-proxy bind address. `null` when unknown to the control plane.
    bind_addr: Option<String>,
    /// Registered upstream providers (id, upstream base URL, request prefix).
    providers: Vec<ProviderInfo>,
}

#[derive(Debug, Serialize)]
struct ControlPlaneSettings {
    bind_addr: Option<String>,
}

#[derive(Debug, Serialize)]
struct GatewaySettings {
    bind_addr: String,
}

#[derive(Debug, Serialize)]
struct ExecSettings {
    default_memory_mb:   u64,
    default_timeout_secs: u64,
    max_concurrent:      usize,
}

#[derive(Debug, Serialize)]
struct PersistenceSettings {
    state_dir:         String,
    snapshot_pool_dir: Option<String>,
    idle_check_secs:   u64,
}

#[derive(Debug, Serialize)]
struct SnapshotSettings {
    /// GC policy. `null` when the GC sweeper is disabled.
    gc: Option<GcPolicy>,
}

#[derive(Debug, Serialize)]
struct GcPolicy {
    max_age_secs: Option<u64>,
    max_count:    Option<usize>,
    tick_secs:    u64,
}

pub(crate) async fn get_settings(State(state): State<AppState>) -> Json<SettingsResponse> {
    let p = state.platform.clone().unwrap_or_default();
    Json(SettingsResponse {
        proxy: ProxySettings {
            base_url:  state.proxy_base_url.clone(),
            bind_addr: p.proxy_addr.map(|a| a.to_string()),
            providers: p.providers.clone(),
        },
        control_plane: ControlPlaneSettings {
            bind_addr: p.ctl_addr.map(|a| a.to_string()),
        },
        gateway: p.gateway_addr.map(|a| GatewaySettings { bind_addr: a.to_string() }),
        exec: state.exec_config.as_ref().map(|e| ExecSettings {
            default_memory_mb:    e.default_memory_mb,
            default_timeout_secs: e.default_timeout_secs,
            max_concurrent:       e.max_concurrent,
        }),
        persistence: PersistenceSettings {
            state_dir:         state.state_dir.display().to_string(),
            snapshot_pool_dir: state.snapshot_pool_dir.as_ref().map(|p| p.display().to_string()),
            idle_check_secs:   p.idle_check_interval_secs,
        },
        snapshots: SnapshotSettings {
            gc: p.snapshot_gc.map(|gc| GcPolicy {
                max_age_secs: gc.max_age_secs,
                max_count:    gc.max_count,
                tick_secs:    gc.tick_secs,
            }),
        },
    })
}

