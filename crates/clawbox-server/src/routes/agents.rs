//! Agent management routes.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use std::sync::Arc;

use crate::AppState;
use clawbox_types::ApiError;
use clawbox_types::agent::AgentConfig;

/// Build the agents route handler.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/agents", post(register_agent))
        .route("/agents", get(list_agents))
        .route("/agents/{id}", get(get_agent))
        .route("/agents/{id}/start", post(start_agent))
        .route("/agents/{id}/stop", post(stop_agent))
        .route("/agents/{id}", delete(remove_agent))
}

async fn register_agent(
    State(state): State<Arc<AppState>>,
    Json(config): Json<AgentConfig>,
) -> impl IntoResponse {
    match state.docker.agent_orchestrator.register_agent(config).await {
        Ok(info) => (StatusCode::CREATED, Json(info)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(e.to_string(), "invalid_request")),
        )
            .into_response(),
    }
}

async fn list_agents(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let agents = state.docker.agent_orchestrator.list_agents().await;
    Json(agents)
}

async fn get_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.docker.agent_orchestrator.get_agent(&id).await {
        Some(info) => (StatusCode::OK, Json(info)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(format!("agent not found: {id}"), "not_found")),
        )
            .into_response(),
    }
}

async fn start_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.docker.agent_orchestrator.start_agent(&id).await {
        Ok(info) => (StatusCode::OK, Json(info)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(e.to_string(), "invalid_request")),
        )
            .into_response(),
    }
}

async fn stop_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.docker.agent_orchestrator.stop_agent(&id).await {
        Ok(info) => (StatusCode::OK, Json(info)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(e.to_string(), "invalid_request")),
        )
            .into_response(),
    }
}

async fn remove_agent(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.docker.agent_orchestrator.remove_agent(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(e.to_string(), "not_found")),
        )
            .into_response(),
    }
}
