//! Tool execution endpoint — the primary API for running WASM tools.

use crate::AppState;
use crate::proxy_handler::ProxyHandler;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use clawbox_proxy::{AuditEntry, CredentialInjector, LeakDetector, ProxyConfig, ProxyService};
use clawbox_types::{
    ApiError, ExecuteRequest, ExecuteResponse, ExecutionMetadata, SanitizationReport,
};
use std::sync::Arc;
use uuid::Uuid;

/// Build the execute route handler.
pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/execute", post(execute_tool))
}

async fn execute_tool(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExecuteRequest>,
) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let tool_name = req.tool.clone();
    let params = req.params.clone();

    // Look up tool manifest for server-side allowlist (C1: CRITICAL security fix)
    let tools = state.tools.read().await;
    let tool_manifest = tools.get(&tool_name);

    // Allowlist comes from tool manifest, NOT from request
    let allowlist = tool_manifest
        .and_then(|m| m.network.as_ref())
        .map(|n| n.allowlist.clone())
        .unwrap_or_default(); // deny-all if no manifest

    // Credential names come from tool manifest too
    let allowed_creds = tool_manifest
        .and_then(|m| m.credentials.as_ref())
        .map(|c| c.available.clone())
        .unwrap_or_default();

    drop(tools); // release read lock

    // The request can specify WHICH of the allowed creds to use (subset only)
    let cred_names = req
        .capabilities
        .as_ref()
        .map(|c| {
            c.credentials
                .iter()
                .filter(|name| allowed_creds.contains(name))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let proxy_config = ProxyConfig::new(
        allowlist,
        state.config.proxy.max_response_bytes,
        state.config.proxy.default_timeout_ms,
    );

    let mut leak_detector = LeakDetector::new();

    let injector = if !cred_names.is_empty() {
        if let Some(ref store) = state.credential_store {
            // Wire known secrets into leak detector
            for secret in store.secret_values() {
                // Note: creates a non-zeroized copy for leak detection.
                // The LeakDetector's lifetime is bounded to this request scope,
                // so the secret is dropped when the handler returns.
                leak_detector.add_known_secret(secret.to_string());
            }
            store.build_injector(&cred_names)
        } else {
            CredentialInjector::new()
        }
    } else {
        CredentialInjector::new()
    };

    let proxy = match ProxyService::new(proxy_config, injector, leak_detector) {
        Ok(p) => p
            .with_rate_limiter(Arc::clone(&state.rate_limiter))
            .with_rate_limit_key(tool_name.clone()),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    format!("proxy setup failed: {e}"),
                    "internal_error",
                )),
            )
                .into_response();
        }
    };

    let tokio_handle = tokio::runtime::Handle::current();
    let handler = Arc::new(ProxyHandler::new(proxy, tokio_handle));

    // Run WASM execution in blocking task
    let engine = Arc::clone(&state);
    let scanner_ref = Arc::clone(&state);
    let req_id = request_id.clone();
    let tn = tool_name.clone();
    let result = tokio::task::spawn_blocking(move || {
        engine
            .sandbox_engine
            .execute_tool(&tn, &params, Some(handler))
    })
    .await;

    match result {
        Ok(Ok(tool_output)) => {
            let output_str = serde_json::to_string(&tool_output.output).unwrap_or_default();
            let sanitization = scanner_ref.output_scanner.scan(&output_str);

            // Apply output redaction when issues found
            let final_output = if sanitization.issues_found > 0 {
                let redacted = scanner_ref.output_scanner.redact(&output_str);
                serde_json::from_str(&redacted).unwrap_or(tool_output.output.clone())
            } else {
                tool_output.output.clone()
            };

            // Record metrics
            crate::metrics::record_wasm_execution(
                &tool_name,
                "ok",
                tool_output.execution_time_ms as f64,
                tool_output.fuel_consumed,
            );
            if sanitization.issues_found > 0 {
                crate::metrics::record_sanitization_issue();
            }
            // M8: Write server-level audit entry
            let mut server_audit =
                AuditEntry::new(format!("/execute/{}", tool_name), "POST".to_string());
            server_audit.status = 200;
            server_audit.duration_ms = tool_output.execution_time_ms;
            server_audit.leak_detected = sanitization.issues_found > 0;
            if !cred_names.is_empty() {
                server_audit.credential_injected = Some(cred_names.join(","));
            }
            if let Err(e) = state.audit_log.record(&server_audit) {
                tracing::warn!("Failed to write audit entry: {e}");
            }

            let mut metadata =
                ExecutionMetadata::new(tool_output.execution_time_ms, tool_output.fuel_consumed);
            metadata.logs = tool_output
                .logs
                .iter()
                .map(|l| serde_json::to_value(l).unwrap_or_default())
                .collect();
            metadata.sanitization = SanitizationReport::new(
                sanitization.issues_found,
                sanitization.actions_taken.clone(),
            );
            let response = ExecuteResponse::ok(final_output, metadata).with_request_id(req_id);

            (StatusCode::OK, Json(response)).into_response()
        }
        Ok(Err(e)) => {
            crate::metrics::record_wasm_execution(&tool_name, "error", 0.0, 0);
            let (status_code, code) = match &e {
                clawbox_sandbox::SandboxError::ToolNotFound(_) => {
                    (StatusCode::NOT_FOUND, "tool_not_found")
                }
                clawbox_sandbox::SandboxError::Timeout => (StatusCode::REQUEST_TIMEOUT, "timeout"),
                clawbox_sandbox::SandboxError::FuelExhausted => {
                    (StatusCode::REQUEST_TIMEOUT, "fuel_exhausted")
                }
                _ => (StatusCode::INTERNAL_SERVER_ERROR, "execution_error"),
            };

            (status_code, Json(ApiError::new(e.to_string(), code))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(
                format!("execution panicked: {e}"),
                "internal_error",
            )),
        )
            .into_response(),
    }
}
