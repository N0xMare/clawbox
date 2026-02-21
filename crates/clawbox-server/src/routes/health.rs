//! Health check and component status endpoint.

use crate::AppState;
use axum::{Json, Router, extract::State, routing::get};
use clawbox_types::{ComponentHealth, HealthComponents, HealthResponse};
use std::sync::Arc;

/// Build the health route handler.
pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/health", get(health_check))
}

async fn health_check(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let (docker_available, active_containers, agents_active) = state.docker_status().await;
    let tools_loaded = state.tools.read().await.len();

    let mut wasm_health = ComponentHealth::ok();
    wasm_health.status = "healthy".into();
    wasm_health.detail = Some(serde_json::json!({ "tools_loaded": tools_loaded }));

    let mut docker_health = ComponentHealth::ok();
    docker_health.status = if docker_available {
        "healthy"
    } else {
        "unavailable"
    }
    .to_string();
    docker_health.detail = Some(serde_json::json!({ "active_containers": active_containers }));

    let mut agents_health = ComponentHealth::ok();
    agents_health.status = "healthy".into();
    agents_health.detail = Some(serde_json::json!({ "active_agents": agents_active }));

    let components = HealthComponents::new(wasm_health, docker_health, agents_health);
    let overall_status = if docker_available { "ok" } else { "degraded" };

    let mut resp = HealthResponse::healthy(
        env!("CARGO_PKG_VERSION"),
        state.start_time.elapsed().as_secs(),
    );
    resp.status = overall_status.to_string();
    resp.docker_available = docker_available;
    resp.active_containers = active_containers;
    resp.components = Some(components);

    Json(resp)
}
