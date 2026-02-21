//! Tests for observability features: metrics endpoint and enhanced health.

use serde_json::Value;
use std::sync::Arc;

async fn start_server() -> (u16, String) {
    let config = clawbox_server::ClawboxConfig::default_config();
    let token = config.server.auth_token.clone();
    let state = Arc::new(clawbox_server::AppState::new(config).await.unwrap());
    let app = clawbox_server::build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (port, token)
}

#[tokio::test]
async fn metrics_endpoint_returns_200_no_auth() {
    let (port, _) = start_server().await;
    let resp = reqwest::get(format!("http://127.0.0.1:{port}/metrics"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn health_includes_components() {
    let (port, _) = start_server().await;
    let resp = reqwest::get(format!("http://127.0.0.1:{port}/health"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();

    assert_eq!(body["status"], "ok");
    assert!(body["uptime_seconds"].is_number());

    let components = &body["components"];
    assert!(components.is_object(), "health should include components");
    assert_eq!(components["wasm_engine"]["status"], "healthy");
    assert!(components["wasm_engine"]["detail"]["tools_loaded"].is_number());
    assert!(components["docker"].is_object());
    assert!(components["agents"]["detail"]["active_agents"].is_number());
}

#[tokio::test]
async fn metrics_endpoint_returns_text_content() {
    let (port, _) = start_server().await;
    let resp = reqwest::get(format!("http://127.0.0.1:{port}/metrics"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    // The response should be text (Prometheus format), not JSON
    let content_type = resp
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap_or("").to_string())
        .unwrap_or_default();
    assert!(
        content_type.contains("text/plain") || content_type.is_empty(),
        "metrics should return text/plain, got: {content_type}"
    );
}
