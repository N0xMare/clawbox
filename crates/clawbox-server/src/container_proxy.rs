//! Spawns per-container proxy listeners.
//! Each container gets a dedicated Unix socket proxy that enforces its specific
//! allowlist and credential injection rules. Containers have no network access
//! and can only reach the outside world through this socket.

use axum::{
    Json,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    response::IntoResponse,
};
use clawbox_proxy::{
    CredentialInjector, LeakDetector, ProxyConfig, ProxyError, ProxyService, RateLimiter,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::net::UnixListener;

/// Errors from container proxy operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ContainerProxyError {
    /// Failed to bind the proxy listener.
    #[error("failed to bind proxy socket {path}: {source}")]
    Bind {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Failed to set socket permissions.
    #[error("failed to set socket permissions on {path}: {source}")]
    Permissions {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Proxy service initialization error.
    #[error("proxy service error: {0}")]
    Proxy(#[from] ProxyError),
}
use clawbox_containers::auth::ContainerTokenStore;

/// Shared state for the per-container proxy handler.
struct ContainerProxyState {
    proxy: ProxyService,
    token_store: Arc<ContainerTokenStore>,
    container_id: String,
}

/// A running per-container proxy instance.
#[non_exhaustive]
pub struct ContainerProxy {
    pub socket_path: PathBuf,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

#[derive(Debug, Deserialize)]
struct ProxyRequest {
    url: String,
    method: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    body: Option<String>,
}

impl ContainerProxy {
    /// Spawn a proxy listener for a container.
    /// The proxy enforces the container's allowlist and injects credentials.
    #[allow(clippy::too_many_arguments)]
    pub async fn spawn(
        socket_path: PathBuf,
        allowlist: Vec<String>,
        injector: CredentialInjector,
        leak_detector: LeakDetector,
        base_config: &ProxyConfig,
        token_store: Arc<ContainerTokenStore>,
        container_id: String,
        rate_limiter: Option<Arc<RateLimiter>>,
    ) -> Result<Self, ContainerProxyError> {
        let proxy_config = ProxyConfig::new(
            allowlist,
            base_config.max_response_bytes,
            base_config.timeout_ms,
        );
        let mut proxy = ProxyService::new(proxy_config, injector, leak_detector)?;
        if let Some(limiter) = rate_limiter {
            proxy = proxy
                .with_rate_limiter(limiter)
                .with_rate_limit_key(&container_id);
        }

        let listener =
            UnixListener::bind(&socket_path).map_err(|source| ContainerProxyError::Bind {
                path: socket_path.clone(),
                source,
            })?;

        // Set socket permissions so container can connect
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o660)).map_err(
            |source| ContainerProxyError::Permissions {
                path: socket_path.clone(),
                source,
            },
        )?;
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let state = Arc::new(ContainerProxyState {
            proxy,
            token_store,
            container_id,
        });
        tokio::spawn(async move {
            let app = axum::Router::new()
                .route("/proxy", axum::routing::post(handle_proxy_request))
                .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
                .with_state(state);

            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await
                .ok();
        });

        Ok(Self {
            socket_path,
            shutdown: Some(shutdown_tx),
        })
    }

    /// Shut down this container's proxy and remove the socket file.
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Handler for proxied requests from the container.
async fn handle_proxy_request(
    State(state): State<Arc<ContainerProxyState>>,
    req: axum::http::Request<axum::body::Body>,
) -> impl IntoResponse {
    // Validate bearer token
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match auth_header {
        Some(token) if state.token_store.validate(&state.container_id, token) => {}
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "unauthorized"})),
            )
                .into_response();
        }
    }

    // Parse the JSON body
    let body_bytes = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };
    let proxy_req: ProxyRequest = match serde_json::from_slice(&body_bytes) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    match state
        .proxy
        .forward_request(
            &proxy_req.url,
            &proxy_req.method,
            proxy_req.headers,
            proxy_req.body,
        )
        .await
    {
        Ok(resp) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": resp.status,
                "headers": resp.headers,
                "body": resp.body,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": e.to_string(),
            })),
        )
            .into_response(),
    }
}
