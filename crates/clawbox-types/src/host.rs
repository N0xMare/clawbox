//! Host call handler trait for WASM-host RPC dispatch.

/// Trait for handling host calls from WASM modules.
///
/// This trait lives in `clawbox-types` rather than `clawbox-sandbox` to allow
/// downstream crates to implement custom host call handlers without depending
/// on the full sandbox engine.
///
/// Implementations receive a method name and JSON parameters, and return
/// a JSON result or an error string.
pub trait HostCallHandler: Send + Sync {
    /// Dispatch a host call from a WASM module.
    ///
    /// # Arguments
    /// * `method` — The host function name (e.g., `"http_request"`, `"credential_get"`).
    /// * `params` — JSON parameters from the WASM module.
    ///
    /// # Returns
    /// A JSON value on success, or a human-readable error string on failure.
    fn handle(&self, method: &str, params: &serde_json::Value)
    -> Result<serde_json::Value, String>;
}
