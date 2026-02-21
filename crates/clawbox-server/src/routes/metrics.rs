//! Prometheus metrics endpoint.

use crate::AppState;
use axum::{Router, extract::State, response::IntoResponse, routing::get};
use std::sync::Arc;

/// Build the metrics route handler.
pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/metrics", get(metrics_handler))
}

async fn metrics_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.metrics_handle.render()
}
