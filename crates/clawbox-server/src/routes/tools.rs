//! Tool management routes.

use crate::AppState;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use clawbox_types::{ApiError, ToolManifest};
use std::sync::Arc;

/// Build the tools route handler.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tools", get(list_tools))
        .route("/tools/register", post(register_tool))
        .route("/tools/reload", post(reload_tools))
        .route("/tools/{name}", get(get_tool))
}

/// Validate tool name: only `[a-zA-Z0-9_-]`, max 64 chars.
fn is_valid_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

async fn get_tool(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let tools = state.tools.read().await;
    match tools.get(&name) {
        Some(manifest) => (StatusCode::OK, Json(manifest.clone())).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                format!("tool '{}' not found", name),
                "tool_not_found",
            )),
        )
            .into_response(),
    }
}

async fn list_tools(State(state): State<Arc<AppState>>) -> Json<Vec<ToolManifest>> {
    let tools = state.tools.read().await;
    Json(tools.values().cloned().collect())
}

async fn register_tool(
    State(state): State<Arc<AppState>>,
    Json(manifest): Json<ToolManifest>,
) -> impl IntoResponse {
    let name = manifest.tool.name.clone();

    // Validate tool name
    if !is_valid_tool_name(&name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "invalid tool name: must be 1-64 chars, [a-zA-Z0-9_-] only",
                "invalid_request",
            )),
        )
            .into_response();
    }

    // Only allow registering manifests for loaded modules
    if !state.sandbox_engine.has_module(&name) {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                format!("tool '{}' not loaded — register only loaded tools", name),
                "tool_not_found",
            )),
        )
            .into_response();
    }

    let mut tools = state.tools.write().await;
    let existed = tools.contains_key(&name);
    tools.insert(name.clone(), manifest);

    crate::metrics::set_tools_loaded(tools.len());
    let status = if existed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    (
        status,
        Json(serde_json::json!({ "status": "registered", "tool": name })),
    )
        .into_response()
}

async fn reload_tools(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.sandbox_engine.reload_all_modules() {
        Ok(count) => {
            let modules = state.sandbox_engine.list_modules();
            crate::metrics::set_tools_loaded(modules.len());
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "reloaded": count,
                    "tools": modules
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(
                format!("reload failed: {e}"),
                "internal_error",
            )),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_name_path_traversal() {
        assert!(!is_valid_tool_name("../etc/passwd"));
    }

    #[test]
    fn test_tool_name_null_bytes() {
        assert!(!is_valid_tool_name("tool\0name"));
    }

    #[test]
    fn test_tool_name_valid() {
        assert!(is_valid_tool_name("echo"));
        assert!(is_valid_tool_name("http-request"));
        assert!(is_valid_tool_name("my_tool_v2"));
    }

    #[test]
    fn test_tool_name_empty() {
        assert!(!is_valid_tool_name(""));
    }
}
