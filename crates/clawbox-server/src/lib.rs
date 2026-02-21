#![doc = include_str!("../README.md")]

pub mod auth;
pub mod config;
#[cfg(feature = "docker")]
pub mod container_proxy;
pub mod metrics;
pub mod proxy_handler;
pub mod routes;
pub mod state;

pub use config::{ClawboxConfig, ImageTemplate, ImagesConfig, ToolsConfig};
pub use state::AppState;

use axum::{Router, extract::DefaultBodyLimit, middleware};
use std::sync::Arc;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::trace::TraceLayer;

/// Build the axum router with all routes.
pub fn build_router(state: Arc<AppState>) -> Router {
    // Health and metrics are public (no auth)
    let public = Router::new()
        .merge(routes::health::router())
        .merge(routes::metrics::router());

    // Protected routes require auth, with concurrency limit
    let protected = {
        let r = Router::new()
            .merge(routes::execute::router())
            .merge(routes::tools::router());

        #[cfg(feature = "docker")]
        let r = r
            .merge(routes::containers::router())
            .merge(routes::agents::router());

        r
    };

    let protected = protected
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ))
        .layer(ConcurrencyLimitLayer::new(
            state.config.server.max_concurrent_executions,
        ));

    public
        .merge(protected)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

/// Spawn a Unix domain socket listener serving the same router.
#[cfg(unix)]
pub async fn spawn_unix_listener(socket_path: &str, app: Router) -> std::io::Result<()> {
    use tokio::net::UnixListener;

    let _ = std::fs::remove_file(socket_path);
    let uds = UnixListener::bind(socket_path)?;

    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))?;
    }

    tracing::info!(path = socket_path, "Unix socket listener started");

    tokio::spawn(async move {
        axum::serve(uds, app).await.ok();
    });

    Ok(())
}
