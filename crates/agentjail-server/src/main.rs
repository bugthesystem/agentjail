//! Runs the phantom proxy + control plane together.
//!
//! Two HTTP listeners are started:
//!  * `PROXY_ADDR` — the sandbox-facing phantom proxy (default 127.0.0.1:8443)
//!  * `CTL_ADDR`   — the operator-facing control plane (default 127.0.0.1:7000)
//!
//! Both share the same in-memory token, key, and audit stores, so
//! phantom tokens issued by the control plane resolve in the proxy, and
//! proxy requests show up in the control plane's `/v1/audit` feed.
//!
//! ## Environment
//!
//! | Var                | Default              | What                       |
//! |--------------------|----------------------|----------------------------|
//! | `CTL_ADDR`         | `127.0.0.1:7000`     | Control plane bind address |
//! | `PROXY_ADDR`       | `127.0.0.1:8443`     | Phantom proxy bind address |
//! | `PROXY_BASE_URL`   | `http://<PROXY_ADDR>`| URL the sandbox uses       |
//! | `AGENTJAIL_API_KEY`| (none → auth off)    | Comma-separated API keys   |
//! | `OPENAI_API_KEY`   | —                    | Seeded as real key         |
//! | `ANTHROPIC_API_KEY`| —                    | Seeded as real key         |
//! | `GITHUB_TOKEN`     | —                    | Seeded as real key         |
//! | `STRIPE_API_KEY`   | —                    | Seeded as real key         |
//!
//! The `*_KEY` env vars are *only* read by this process and stored in the
//! host's in-memory key store. They are never forwarded to sandboxes —
//! sandboxes receive `phm_<hex>` phantom tokens and the proxy injects the
//! real value on their behalf.

use std::net::SocketAddr;
use std::sync::Arc;

use agentjail_ctl::{AuditStoreSink, ControlPlane, ControlPlaneConfig, InMemoryAuditStore};
use agentjail_phantom::providers::{
    AnthropicProvider, GitHubProvider, OpenAiProvider, StripeProvider,
};
use agentjail_phantom::{
    InMemoryKeyStore, InMemoryTokenStore, PhantomProxy, SecretString, ServiceId,
};
use anyhow::{Context, Result};
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::watch;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env()?;
    let stores = Stores::new_from_env();

    // Cross-service shutdown signal.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Build the phantom proxy.
    let audit_sink = Arc::new(AuditStoreSink::new(stores.audit.clone()));
    let proxy = PhantomProxy::builder()
        .provider(Arc::new(OpenAiProvider::new()))?
        .provider(Arc::new(AnthropicProvider::new()))?
        .provider(Arc::new(GitHubProvider::new()))?
        .provider(Arc::new(StripeProvider::new()))?
        .tokens(stores.tokens.clone())
        .keys(stores.keys.clone())
        .audit(audit_sink)
        .build()?;

    // Spawn the proxy.
    let proxy_shutdown = wait_for(shutdown_rx.clone());
    let proxy_handle = tokio::spawn({
        let p = proxy.clone();
        let addr = config.proxy_addr;
        async move {
            tracing::info!(%addr, "phantom proxy listening");
            p.serve(addr, proxy_shutdown).await
        }
    });

    // Build the control plane.
    let ctl = ControlPlane::with_stores(
        ControlPlaneConfig {
            tokens: stores.tokens.clone(),
            keys: stores.keys.clone(),
            proxy_base_url: config.proxy_base_url.clone(),
            api_keys: config.api_keys.clone(),
        },
        Arc::new(agentjail_ctl::InMemorySessionStore::new()),
        Arc::new(agentjail_ctl::InMemoryCredentialStore::new()),
        stores.audit.clone(),
    );
    let router = ctl.router();

    let ctl_listener = TcpListener::bind(config.ctl_addr)
        .await
        .with_context(|| format!("bind control plane to {}", config.ctl_addr))?;
    tracing::info!(addr = %config.ctl_addr, "control plane listening");

    let ctl_shutdown = wait_for(shutdown_rx.clone());
    let ctl_handle = tokio::spawn(async move {
        axum::serve(ctl_listener, router)
            .with_graceful_shutdown(ctl_shutdown)
            .await
    });

    // Block on ctrl-c / SIGTERM.
    wait_for_signal().await;
    tracing::info!("shutdown requested, draining in-flight requests");
    let _ = shutdown_tx.send(true);

    // Wait for both servers to drain. Ignore JoinError.
    let _ = ctl_handle.await;
    let _ = proxy_handle.await;
    tracing::info!("goodbye");
    Ok(())
}

async fn wait_for(mut rx: watch::Receiver<bool>) {
    let _ = rx.changed().await;
}

async fn wait_for_signal() {
    #[cfg(unix)]
    {
        let mut term = signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            _ = signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = signal::ctrl_c().await;
    }
}

// ---------- config ----------

struct Config {
    ctl_addr: SocketAddr,
    proxy_addr: SocketAddr,
    proxy_base_url: String,
    api_keys: Vec<String>,
}

impl Config {
    fn from_env() -> Result<Self> {
        let ctl_addr: SocketAddr = std::env::var("CTL_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:7000".into())
            .parse()
            .context("CTL_ADDR")?;
        let proxy_addr: SocketAddr = std::env::var("PROXY_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8443".into())
            .parse()
            .context("PROXY_ADDR")?;
        let proxy_base_url =
            std::env::var("PROXY_BASE_URL").unwrap_or_else(|_| format!("http://{proxy_addr}"));
        let api_keys = std::env::var("AGENTJAIL_API_KEY")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();

        if api_keys.is_empty() {
            tracing::warn!(
                "AGENTJAIL_API_KEY unset — control plane is OPEN. \
                 Set it in any non-dev environment."
            );
        }

        Ok(Self {
            ctl_addr,
            proxy_addr,
            proxy_base_url,
            api_keys,
        })
    }
}

struct Stores {
    tokens: Arc<InMemoryTokenStore>,
    keys: Arc<InMemoryKeyStore>,
    audit: Arc<InMemoryAuditStore>,
}

impl Stores {
    fn new_from_env() -> Self {
        let keys = Arc::new(InMemoryKeyStore::new());
        seed_if_set(&keys, ServiceId::OpenAi, "OPENAI_API_KEY");
        seed_if_set(&keys, ServiceId::Anthropic, "ANTHROPIC_API_KEY");
        seed_if_set(&keys, ServiceId::GitHub, "GITHUB_TOKEN");
        seed_if_set(&keys, ServiceId::Stripe, "STRIPE_API_KEY");
        Self {
            tokens: Arc::new(InMemoryTokenStore::new()),
            keys,
            audit: Arc::new(InMemoryAuditStore::new()),
        }
    }
}

fn seed_if_set(keys: &InMemoryKeyStore, service: ServiceId, env_var: &str) {
    if let Ok(v) = std::env::var(env_var)
        && !v.trim().is_empty()
    {
        keys.set(service, SecretString::new(v));
        tracing::info!(%service, %env_var, "seeded upstream key from env");
    }
}
