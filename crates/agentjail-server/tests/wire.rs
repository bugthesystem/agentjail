//! End-to-end wire test: boot the same stack as `main.rs` in-process,
//! create a session via the control plane, and hit the phantom proxy with
//! the returned token. Proves that ctl → phantom → upstream wiring works.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agentjail_ctl::{
    AuditStoreSink, ControlPlane, ControlPlaneConfig, InMemoryAuditStore, InMemoryCredentialStore,
    InMemorySessionStore,
};
use agentjail_phantom::providers::OpenAiProvider;
use agentjail_phantom::{InMemoryKeyStore, InMemoryTokenStore, PhantomProxy};
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::any;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

#[derive(Default, Clone)]
struct Last(Arc<Mutex<Option<HeaderMap>>>);

async fn echo(State(last): State<Last>, req: Request<Body>) -> impl IntoResponse {
    *last.0.lock().unwrap() = Some(req.headers().clone());
    (
        StatusCode::OK,
        [("content-type", "application/json")],
        r#"{"ok":true}"#,
    )
}

#[tokio::test]
async fn server_stack_forwards_phantom_to_real_upstream() {
    // 1. Mock upstream.
    let last = Last::default();
    let app = Router::new()
        .route("/v1/chat/completions", any(echo))
        .with_state(last.clone());
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let (upstream_stop, upstream_stop_rx) = oneshot::channel::<()>();
    let upstream_task = tokio::spawn(async move {
        axum::serve(upstream, app)
            .with_graceful_shutdown(async move {
                let _ = upstream_stop_rx.await;
            })
            .await
            .unwrap();
    });

    // 2. Shared stores.
    let tokens = Arc::new(InMemoryTokenStore::new());
    let keys = Arc::new(InMemoryKeyStore::new());
    keys.set(
        agentjail_phantom::ServiceId::OpenAi,
        agentjail_phantom::SecretString::new("sk-real"),
    );
    let audit = Arc::new(InMemoryAuditStore::new());

    // 3. Phantom proxy (points at the mock upstream).
    let proxy = PhantomProxy::builder()
        .provider(Arc::new(OpenAiProvider::with_base(format!(
            "http://{upstream_addr}"
        ))))
        .unwrap()
        .tokens(tokens.clone())
        .keys(keys.clone())
        .audit(Arc::new(AuditStoreSink::new(audit.clone())))
        .build()
        .unwrap();
    let (proxy_addr_tx, proxy_addr_rx) = oneshot::channel::<SocketAddr>();
    let (proxy_stop, proxy_stop_rx) = oneshot::channel::<()>();
    let proxy_task = tokio::spawn({
        let p = proxy.clone();
        async move {
            p.serve_with_bound_addr("127.0.0.1:0".parse().unwrap(), proxy_addr_tx, async move {
                let _ = proxy_stop_rx.await;
            })
            .await
            .unwrap();
        }
    });
    let proxy_addr = proxy_addr_rx.await.unwrap();

    // 4. Control plane, same stores.
    let ctl = ControlPlane::with_stores(
        ControlPlaneConfig {
            tokens: tokens.clone(),
            keys: keys.clone(),
            proxy_base_url: format!("http://{proxy_addr}"),
            api_keys: vec!["aj_k".into()],
        },
        Arc::new(InMemorySessionStore::new()),
        Arc::new(InMemoryCredentialStore::new()),
        audit.clone(),
    );
    let ctl_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ctl_addr = ctl_listener.local_addr().unwrap();
    let (ctl_stop, ctl_stop_rx) = oneshot::channel::<()>();
    let router = ctl.router();
    let ctl_task = tokio::spawn(async move {
        axum::serve(ctl_listener, router)
            .with_graceful_shutdown(async move {
                let _ = ctl_stop_rx.await;
            })
            .await
            .unwrap();
    });

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    // 5. Create a session via the control plane.
    let session: Value = http
        .post(format!("http://{ctl_addr}/v1/sessions"))
        .bearer_auth("aj_k")
        .json(&json!({ "services": ["openai"] }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let env = session["env"].as_object().unwrap();
    let phantom = env["OPENAI_API_KEY"].as_str().unwrap();
    let base = env["OPENAI_BASE_URL"].as_str().unwrap();
    assert!(phantom.starts_with("phm_"), "got {phantom}");
    // The SDK / raw consumer calls <base>/chat/completions.
    let url = format!("{base}/chat/completions");

    // 6. Hit the proxy like a sandbox would.
    let resp = http
        .post(url)
        .bearer_auth(phantom)
        .json(&json!({"model": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 7. Upstream saw the real key, not the phantom.
    let h = last.0.lock().unwrap().clone().expect("upstream hit");
    let auth = h.get("authorization").unwrap().to_str().unwrap();
    assert_eq!(auth, "Bearer sk-real");
    assert!(!auth.contains("phm_"));

    // 8. The control plane's audit feed recorded the request.
    let audit_resp: Value = http
        .get(format!("http://{ctl_addr}/v1/audit"))
        .bearer_auth("aj_k")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(audit_resp["total"].as_u64().unwrap(), 1);
    let rows = audit_resp["rows"].as_array().unwrap();
    assert_eq!(rows[0]["service"].as_str().unwrap(), "openai");
    assert_eq!(rows[0]["status"].as_u64().unwrap(), 200);

    // Cleanup.
    let _ = ctl_stop.send(());
    let _ = proxy_stop.send(());
    let _ = upstream_stop.send(());
    let _ = ctl_task.await;
    let _ = proxy_task.await;
    let _ = upstream_task.await;
}
