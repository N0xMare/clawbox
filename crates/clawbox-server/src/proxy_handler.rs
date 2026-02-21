//! Bridge between WASM host_call (sync) and ProxyService (async).
//!
//! Implements `HostCallHandler` for the proxy, allowing WASM tools
//! to make HTTP requests through the proxy pipeline.

use std::collections::HashMap;

use clawbox_proxy::ProxyService;
use clawbox_types::HostCallHandler;
use tokio::runtime::Handle;

/// Handler that bridges WASM host calls to the async ProxyService.
#[non_exhaustive]
pub struct ProxyHandler {
    proxy: ProxyService,
    handle: Handle,
}

impl ProxyHandler {
    pub fn new(proxy: ProxyService, handle: Handle) -> Self {
        Self { proxy, handle }
    }
}

impl HostCallHandler for ProxyHandler {
    /// Dispatch a host call to the proxy service.
    ///
    /// # Warning
    /// This method uses `block_on` internally and **must** be called from
    /// `spawn_blocking`, never directly from an async context. Calling it
    /// from an async runtime thread will panic.
    fn handle(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        match method {
            "http_request" => {
                let url = params
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or("missing 'url' parameter")?
                    .to_string();

                let method_str = params
                    .get("method")
                    .and_then(|v| v.as_str())
                    .unwrap_or("GET")
                    .to_string();

                let headers: HashMap<String, String> = params
                    .get("headers")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();

                let body = params
                    .get("body")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Bridge async ProxyService into sync HostCallHandler
                let result = tokio::task::block_in_place(|| {
                    self.handle.block_on(self.proxy.forward_request(
                        &url,
                        &method_str,
                        headers,
                        body,
                    ))
                });

                match result {
                    Ok(response) => Ok(serde_json::json!({
                        "status": response.status,
                        "headers": response.headers,
                        "body": response.body
                    })),
                    Err(e) => Err(e.to_string()),
                }
            }
            other => Err(format!("unknown host call method: {other}")),
        }
    }
}
