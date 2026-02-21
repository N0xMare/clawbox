//! Auth middleware edge-case tests.

async fn start_server() -> (u16, String) {
    let config = clawbox_server::ClawboxConfig::default_config();
    let token = config.server.auth_token.clone();
    let state = std::sync::Arc::new(clawbox_server::AppState::new(config).await.unwrap());
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
async fn test_auth_no_header() {
    let (port, _) = start_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{port}/tools"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_auth_empty_bearer() {
    let (port, _) = start_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{port}/tools"))
        .header("Authorization", "Bearer ")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_auth_wrong_scheme() {
    let (port, _) = start_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{port}/tools"))
        .header("Authorization", "Basic xxx")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_auth_wrong_token() {
    let (port, _) = start_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{port}/tools"))
        .bearer_auth("wrong-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_auth_valid_token() {
    let (port, token) = start_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{port}/tools"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}
