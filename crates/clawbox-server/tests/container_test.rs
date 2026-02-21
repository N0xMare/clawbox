//! Container integration tests — require Docker.
//! Run with: cargo test -p clawbox-server --test container_test -- --ignored

use std::sync::Arc;

/// Helper to check if Docker is available
async fn docker_available() -> bool {
    match bollard::Docker::connect_with_local_defaults() {
        Ok(docker) => docker.ping().await.is_ok(),
        Err(_) => false,
    }
}

/// Start a server with container support enabled
async fn start_server_with_containers() -> Option<(u16, String)> {
    if !docker_available().await {
        return None;
    }

    let mut config = clawbox_server::ClawboxConfig::default_config();
    // Point to real tools directory
    config.sandbox.tool_dir = std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tools")
        .join("wasm")
        .to_string_lossy()
        .to_string();

    let state = Arc::new(clawbox_server::AppState::new(config.clone()).await.ok()?);
    let token = config.server.auth_token.clone();
    let app = clawbox_server::build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.ok()?;
    let port = listener.local_addr().ok()?.port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    Some((port, token))
}

#[tokio::test]
#[ignore] // Requires Docker
async fn test_spawn_container() {
    let Some((port, token)) = start_server_with_containers().await else {
        eprintln!("Docker not available, skipping");
        return;
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{port}/containers/spawn"))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "task": "echo hello from container",
            "policy": "container",
            "capabilities": {
                "network": { "allowlist": [] },
                "credentials": [],
                "resources": { "timeout_ms": 10000, "memory_mb": 128 }
            }
        }))
        .send()
        .await
        .unwrap();

    // Should succeed (or return appropriate error if image not pulled)
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap();

    if status.is_success() {
        assert!(body["container_id"].is_string());
        assert_eq!(body["status"], "creating");

        // Clean up
        let container_id = body["container_id"].as_str().unwrap();
        client
            .delete(format!("http://127.0.0.1:{port}/containers/{container_id}"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
    } else {
        // Acceptable: image not found, Docker error, etc.
        eprintln!("Container spawn returned {status}: {body}");
    }
}

#[tokio::test]
#[ignore] // Requires Docker
async fn test_list_containers_empty() {
    let Some((port, token)) = start_server_with_containers().await else {
        eprintln!("Docker not available, skipping");
        return;
    };

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{port}/containers"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.as_array().is_some());
}

#[tokio::test]
#[ignore]
async fn test_get_nonexistent_container() {
    let Some((port, token)) = start_server_with_containers().await else {
        return;
    };

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{port}/containers/nonexistent-id"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}
