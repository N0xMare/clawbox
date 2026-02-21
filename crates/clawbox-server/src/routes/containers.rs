//! Container lifecycle endpoints — spawn, inspect, kill.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use clawbox_containers::ContainerBackend;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::AppState;
use crate::container_proxy::ContainerProxy;
use crate::state::{proxy_socket_dir, proxy_socket_path};
use clawbox_proxy::{CredentialInjector, LeakDetector, ProxyConfig};
use clawbox_types::{ApiError, ContainerSpawnRequest};

/// Build the containers route handler.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/containers", get(list_containers))
        .route("/containers/spawn", post(spawn_container))
        .route("/containers/{id}", get(get_container))
        .route("/containers/{id}", delete(kill_container))
}

async fn spawn_container(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ContainerSpawnRequest>,
) -> impl IntoResponse {
    // Enforce allowed sandbox policies
    let policy_str = req.policy.to_string();
    if !state
        .config
        .container_policy
        .allowed_policies
        .iter()
        .any(|p| p.eq_ignore_ascii_case(&policy_str))
    {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                format!(
                    "Sandbox policy '{}' is not allowed; permitted: {:?}",
                    policy_str, state.config.container_policy.allowed_policies
                ),
                "policy_denied",
            )),
        )
            .into_response();
    }

    // Enforce max_containers limit
    let current = state.docker.container_manager.list().await.len();
    if current >= state.config.containers.max_containers {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(ApiError::new(
                format!(
                    "container limit reached ({}/{})",
                    current, state.config.containers.max_containers
                ),
                "resource_exhausted",
            )),
        )
            .into_response();
    }

    // Pre-generate container ID so we can pass it to the proxy for auth validation
    let (container_id, proxy_token) = state.docker.container_manager.pre_generate_id();

    // 1. Create socket directory for this container
    let socket_dir = proxy_socket_dir(&container_id);
    let socket_path = proxy_socket_path(&container_id);
    if let Err(e) = std::fs::create_dir_all(&socket_dir) {
        error!(
            "Failed to create proxy socket dir {}: {e}",
            socket_dir.display()
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(
                format!("Failed to create proxy socket dir: {e}"),
                "proxy_error",
            )),
        )
            .into_response();
    }
    // Set directory permissions so container can access the socket
    if let Err(e) = std::fs::set_permissions(&socket_dir, std::fs::Permissions::from_mode(0o770)) {
        error!("Failed to set socket dir permissions: {e}");
        let _ = std::fs::remove_dir_all(&socket_dir);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(
                format!("Failed to set socket dir permissions: {e}"),
                "proxy_error",
            )),
        )
            .into_response();
    }
    let token_store = state.docker.container_manager.token_store().clone();

    // SECURITY FIX 1: Use server-side allowlist, NOT client-requested allowlist
    let server_allowlist = state.config.container_policy.network_allowlist.clone();
    if !req.capabilities.network.allowlist.is_empty() {
        warn!(
            container_id = %container_id,
            client_allowlist = ?req.capabilities.network.allowlist,
            "Client-requested network allowlist was IGNORED; using server-side policy"
        );
    }

    // SECURITY FIX 2: Filter credentials against server-side allowed list
    let allowed_creds = &state.config.container_policy.allowed_credentials;
    let filtered_creds: Vec<String> = req
        .capabilities
        .credentials
        .iter()
        .filter(|name| allowed_creds.contains(name))
        .cloned()
        .collect();
    let denied_creds: Vec<&String> = req
        .capabilities
        .credentials
        .iter()
        .filter(|name| !allowed_creds.contains(name))
        .collect();
    if !denied_creds.is_empty() {
        warn!(
            container_id = %container_id,
            denied = ?denied_creds,
            "Client-requested credentials were DENIED by server policy"
        );
    }

    // 2. Build credential injector from credential store for ALLOWED credentials only
    let injector = match &state.credential_store {
        Some(store) => store.build_injector(&filtered_creds),
        None => CredentialInjector::new(),
    };

    // 3. Build leak detector with known secrets
    let mut leak_detector = LeakDetector::new();
    if let Some(store) = &state.credential_store {
        for secret in store.secret_values() {
            // Note: creates a non-zeroized copy for leak detection.
            // The LeakDetector's lifetime is bounded to this container proxy's scope.
            leak_detector.add_known_secret(secret.as_str());
        }
    }

    // 4. Spawn the per-container proxy with token validation
    let base_proxy_config = ProxyConfig::new(
        vec![],
        state.config.proxy.max_response_bytes,
        state.config.proxy.default_timeout_ms,
    );

    let container_proxy = match ContainerProxy::spawn(
        socket_path.clone(),
        server_allowlist, // SECURITY: server-side allowlist
        injector,
        leak_detector,
        &base_proxy_config,
        token_store,
        container_id.clone(),
        Some(Arc::clone(&state.rate_limiter)),
    )
    .await
    {
        Ok(proxy) => proxy,
        Err(e) => {
            error!(
                "Failed to spawn container proxy at {}: {e}",
                socket_path.display()
            );
            let _ = std::fs::remove_dir_all(&socket_dir);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    format!("Failed to spawn container proxy: {e}"),
                    "proxy_error",
                )),
            )
                .into_response();
        }
    };

    // 5. Spawn the container via DockerBackend with pre-generated ID
    match state
        .docker
        .container_manager
        .spawn(req, &socket_path, Some((container_id.clone(), proxy_token)))
        .await
    {
        Ok(response) => {
            // Track the proxy
            state
                .docker
                .container_proxies
                .write()
                .await
                .insert(response.container_id.clone(), container_proxy);

            crate::metrics::record_container_event("spawned");
            crate::metrics::set_containers_active(
                state.docker.container_manager.list().await.len(),
            );

            info!(
                container_id = %response.container_id,
                proxy_socket = %socket_path.display(),
                "Container spawned"
            );

            (StatusCode::CREATED, Json(response)).into_response()
        }
        Err(e) => {
            error!("Failed to spawn container: {e}");
            // H4: Clean up pre-generated token on spawn failure
            state
                .docker
                .container_manager
                .token_store()
                .remove(&container_id);
            // Cleanup proxy and socket dir on container spawn failure
            let mut proxy = container_proxy;
            proxy.shutdown();
            let _ = std::fs::remove_dir_all(&socket_dir);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    format!("Failed to spawn container: {e}"),
                    "container_error",
                )),
            )
                .into_response()
        }
    }
}

async fn list_containers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let containers = state.docker.container_manager.list().await;
    (StatusCode::OK, Json(containers)).into_response()
}

async fn get_container(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.docker.container_manager.get(&id).await {
        Some(info) => (StatusCode::OK, Json(info)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new("Container not found", "not_found")),
        )
            .into_response(),
    }
}

async fn kill_container(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // FIX 4: Collect output BEFORE killing the container
    let output = state
        .docker
        .container_manager
        .collect_output(&id)
        .await
        .ok();

    // Kill the container
    if let Err(e) = state.docker.container_manager.kill(&id).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(
                format!("Failed to kill container: {e}"),
                "internal_error",
            )),
        )
            .into_response();
    }

    // Shut down the per-container proxy and clean up socket dir
    if let Some(mut proxy) = state.docker.container_proxies.write().await.remove(&id) {
        let socket_dir = proxy.socket_path.parent().map(|p| p.to_path_buf());
        proxy.shutdown();
        if let Some(dir) = socket_dir {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    crate::metrics::record_container_event("killed");
    crate::metrics::set_containers_active(state.docker.container_manager.list().await.len());

    info!(container_id = %id, "Container killed and output collected");
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "container_id": id,
            "status": "killed",
            "output": output,
        })),
    )
        .into_response()
}
