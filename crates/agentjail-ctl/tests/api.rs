//! End-to-end HTTP integration tests for the control plane.

use std::net::SocketAddr;
use std::sync::Arc;

use agentjail_ctl::{ControlPlane, ControlPlaneConfig};
use agentjail_phantom::{InMemoryKeyStore, InMemoryTokenStore};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

struct Harness {
    base: String,
    shutdown: Option<oneshot::Sender<()>>,
    join: tokio::task::JoinHandle<()>,
}

impl Harness {
    async fn start(api_keys: Vec<String>) -> Self {
        let tokens = Arc::new(InMemoryTokenStore::new());
        let keys = Arc::new(InMemoryKeyStore::new());
        let ctl = ControlPlane::new(ControlPlaneConfig {
            tokens,
            keys,
            proxy_base_url: "http://10.0.0.1:8443".into(),
            api_keys,
        });
        let router = ctl.router();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        let (tx, rx) = oneshot::channel::<()>();
        let join = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = rx.await;
                })
                .await
                .unwrap();
        });
        Self {
            base: format!("http://{addr}"),
            shutdown: Some(tx),
            join,
        }
    }

    async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = self.join.await;
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().build().unwrap()
}

#[tokio::test]
async fn healthz_is_public() {
    let h = Harness::start(vec!["aj_test".into()]).await;
    let r = reqwest::get(format!("{}/healthz", h.base)).await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.text().await.unwrap(), "ok");
    h.stop().await;
}

#[tokio::test]
async fn guarded_routes_require_api_key() {
    let h = Harness::start(vec!["aj_test".into()]).await;
    let r = client()
        .get(format!("{}/v1/credentials", h.base))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    let r = client()
        .get(format!("{}/v1/credentials", h.base))
        .bearer_auth("aj_wrong")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    h.stop().await;
}

#[tokio::test]
async fn attach_list_delete_credential() {
    let h = Harness::start(vec!["aj_test".into()]).await;
    let c = client();

    // Attach.
    let r = c
        .post(format!("{}/v1/credentials", h.base))
        .bearer_auth("aj_test")
        .json(&json!({ "service": "openai", "secret": "sk-abc" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["service"], "openai");
    assert!(!body["fingerprint"].as_str().unwrap().is_empty());
    assert!(body["added_at"].as_str().is_some());

    // Rotate: updated_at should change, added_at should not.
    let added_at_1 = body["added_at"].as_str().unwrap().to_string();
    let r = c
        .post(format!("{}/v1/credentials", h.base))
        .bearer_auth("aj_test")
        .json(&json!({ "service": "openai", "secret": "sk-rotated" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body2: Value = r.json().await.unwrap();
    assert_eq!(body2["added_at"].as_str().unwrap(), added_at_1);
    assert_ne!(body2["fingerprint"], body["fingerprint"]);

    // List.
    let r = c
        .get(format!("{}/v1/credentials", h.base))
        .bearer_auth("aj_test")
        .send()
        .await
        .unwrap();
    let list: Vec<Value> = r.json().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["service"], "openai");

    // Delete.
    let r = c
        .delete(format!("{}/v1/credentials/openai", h.base))
        .bearer_auth("aj_test")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);

    let r = c
        .get(format!("{}/v1/credentials", h.base))
        .bearer_auth("aj_test")
        .send()
        .await
        .unwrap();
    let list: Vec<Value> = r.json().await.unwrap();
    assert!(list.is_empty());

    h.stop().await;
}

#[tokio::test]
async fn reject_empty_secret() {
    let h = Harness::start(vec!["aj_test".into()]).await;
    let r = client()
        .post(format!("{}/v1/credentials", h.base))
        .bearer_auth("aj_test")
        .json(&json!({ "service": "openai", "secret": "" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    h.stop().await;
}

#[tokio::test]
async fn create_session_with_no_credential_fails() {
    let h = Harness::start(vec!["aj_test".into()]).await;
    let r = client()
        .post(format!("{}/v1/sessions", h.base))
        .bearer_auth("aj_test")
        .json(&json!({ "services": ["openai"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    h.stop().await;
}

#[tokio::test]
async fn session_lifecycle() {
    let h = Harness::start(vec!["aj_test".into()]).await;
    let c = client();

    // Configure credentials for two services.
    c.post(format!("{}/v1/credentials", h.base))
        .bearer_auth("aj_test")
        .json(&json!({ "service": "openai", "secret": "sk-real" }))
        .send()
        .await
        .unwrap();
    c.post(format!("{}/v1/credentials", h.base))
        .bearer_auth("aj_test")
        .json(&json!({ "service": "anthropic", "secret": "sk-ant" }))
        .send()
        .await
        .unwrap();

    // Create session.
    let r = c
        .post(format!("{}/v1/sessions", h.base))
        .bearer_auth("aj_test")
        .json(&json!({ "services": ["openai", "anthropic"], "ttl_secs": 300 }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let body: Value = r.json().await.unwrap();
    let id = body["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("sess_"));
    assert!(body["expires_at"].as_str().is_some());

    // env must contain phantom tokens and proxy base URLs.
    let env = body["env"].as_object().unwrap();
    let openai_key = env["OPENAI_API_KEY"].as_str().unwrap();
    assert!(openai_key.starts_with("phm_"));
    assert_eq!(openai_key.len(), 4 + 64);
    assert_eq!(
        env["OPENAI_BASE_URL"].as_str().unwrap(),
        "http://10.0.0.1:8443/v1/openai/v1"
    );
    let anth_key = env["ANTHROPIC_API_KEY"].as_str().unwrap();
    assert!(anth_key.starts_with("phm_"));
    assert_ne!(anth_key, openai_key, "tokens must be distinct per service");
    assert_eq!(
        env["ANTHROPIC_BASE_URL"].as_str().unwrap(),
        "http://10.0.0.1:8443/v1/anthropic"
    );

    // List sessions.
    let list: Vec<Value> = c
        .get(format!("{}/v1/sessions", h.base))
        .bearer_auth("aj_test")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"].as_str().unwrap(), id);

    // Get a single session.
    let one: Value = c
        .get(format!("{}/v1/sessions/{id}", h.base))
        .bearer_auth("aj_test")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(one["id"].as_str().unwrap(), id);

    // Delete.
    let r = c
        .delete(format!("{}/v1/sessions/{id}", h.base))
        .bearer_auth("aj_test")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);

    let r = c
        .get(format!("{}/v1/sessions/{id}", h.base))
        .bearer_auth("aj_test")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);

    h.stop().await;
}

#[tokio::test]
async fn unknown_service_4xx() {
    let h = Harness::start(vec!["aj_test".into()]).await;
    let r = client()
        .post(format!("{}/v1/credentials", h.base))
        .bearer_auth("aj_test")
        .json(&json!({ "service": "not-a-real-service", "secret": "sk" }))
        .send()
        .await
        .unwrap();
    // Serde rejects the unknown variant -> 422 from axum's Json extractor.
    assert!(r.status().is_client_error());
    h.stop().await;
}

#[tokio::test]
async fn session_with_scopes_reaches_through_to_token() {
    let h = Harness::start(vec!["aj_test".into()]).await;
    let c = client();
    c.post(format!("{}/v1/credentials", h.base))
        .bearer_auth("aj_test")
        .json(&json!({ "service": "github", "secret": "ghp_x" }))
        .send()
        .await
        .unwrap();

    // Happy path: valid scope.
    let r = c
        .post(format!("{}/v1/sessions", h.base))
        .bearer_auth("aj_test")
        .json(&json!({
            "services": ["github"],
            "scopes": { "github": ["/repos/foo/*"] }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);

    // Scope keyed by a service not in `services` -> 400.
    let r = c
        .post(format!("{}/v1/sessions", h.base))
        .bearer_auth("aj_test")
        .json(&json!({
            "services": ["github"],
            "scopes": { "openai": ["/v1/*"] }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);

    h.stop().await;
}

#[tokio::test]
async fn audit_endpoint_returns_empty_initially() {
    let h = Harness::start(vec!["aj_test".into()]).await;
    let body: Value = client()
        .get(format!("{}/v1/audit", h.base))
        .bearer_auth("aj_test")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["total"], 0);
    assert!(body["rows"].as_array().unwrap().is_empty());
    h.stop().await;
}
