//! Bearer token authentication middleware.

use crate::AppState;
use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use clawbox_types::ApiError;
use std::sync::Arc;
use subtle::ConstantTimeEq;

/// Auth middleware — checks Bearer token against config.
/// Health endpoint is exempt (handled by route ordering in lib.rs).
pub async fn require_auth(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let expected = &state.config.server.auth_token;

    let authorized = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|token| {
            // Constant-time comparison to prevent timing attacks
            bool::from(token.as_bytes().ct_eq(expected.as_bytes()))
        });

    if authorized {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new("unauthorized", "auth_required")),
        )
            .into_response()
    }
}
