//! Host functions exposed to WASM guest modules.
//!
//! These are the ONLY capabilities a WASM tool has access to.
//! The `host_call` import is the single extensible RPC mechanism.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub use clawbox_types::HostCallHandler;

/// A log entry captured from a WASM tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LogEntry {
    pub level: String,
    pub message: String,
}

/// A no-op handler that rejects all host calls (useful for testing).
#[non_exhaustive]
pub struct NoOpHandler;

impl HostCallHandler for NoOpHandler {
    fn handle(
        &self,
        method: &str,
        _params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err(format!(
            "host call '{method}' not available in this context"
        ))
    }
}

/// State held per-execution for host function dispatch.
pub(crate) struct HostState {
    pub handler: Arc<dyn HostCallHandler>,
    pub logs: Arc<Mutex<Vec<LogEntry>>>,
    pub call_count: AtomicU32,
    pub max_host_calls: u32,
}

impl HostState {
    pub fn with_limit(handler: Arc<dyn HostCallHandler>, max_host_calls: u32) -> Self {
        Self {
            handler,
            logs: Arc::new(Mutex::new(Vec::new())),
            call_count: AtomicU32::new(0),
            max_host_calls,
        }
    }

    /// Dispatch a host_call request. Returns JSON response bytes.
    pub fn dispatch(&self, request_json: &str) -> Vec<u8> {
        let count = self.call_count.fetch_add(1, Ordering::Relaxed);
        if count >= self.max_host_calls {
            let response = serde_json::json!({ "error": format!("host_call limit exceeded (max {})", self.max_host_calls) });
            return serde_json::to_vec(&response)
                .unwrap_or_else(|_| b"{\"error\":\"serialization failed\"}".to_vec());
        }
        let response = match self.dispatch_inner(request_json) {
            Ok(value) => serde_json::json!({ "ok": value }),
            Err(e) => serde_json::json!({ "error": e }),
        };
        serde_json::to_vec(&response)
            .unwrap_or_else(|_| b"{\"error\":\"serialization failed\"}".to_vec())
    }

    fn dispatch_inner(&self, request_json: &str) -> Result<serde_json::Value, String> {
        #[derive(Deserialize)]
        struct Request {
            method: String,
            #[serde(default)]
            params: serde_json::Value,
        }

        let req: Request = serde_json::from_str(request_json)
            .map_err(|e| format!("invalid host_call request: {e}"))?;

        match req.method.as_str() {
            "log" => {
                let level = req
                    .params
                    .get("level")
                    .and_then(|v| v.as_str())
                    .unwrap_or("info")
                    .to_string();
                let message = req
                    .params
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if let Ok(mut logs) = self.logs.lock() {
                    logs.push(LogEntry { level, message });
                }
                Ok(serde_json::json!(null))
            }
            method => self.handler.handle(method, &req.params),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(max: u32) -> HostState {
        HostState::with_limit(Arc::new(NoOpHandler), max)
    }

    #[test]
    fn test_dispatch_log() {
        let state = make_state(100);
        let resp =
            state.dispatch(r#"{"method":"log","params":{"level":"info","message":"hello"}}"#);
        let v: serde_json::Value = serde_json::from_slice(&resp).unwrap();
        assert!(v.get("ok").is_some());
        let logs = state.logs.lock().unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].message, "hello");
    }

    #[test]
    fn test_dispatch_unknown_method() {
        let state = make_state(100);
        let resp = state.dispatch(r#"{"method":"unknown_thing","params":{}}"#);
        let v: serde_json::Value = serde_json::from_slice(&resp).unwrap();
        assert!(v.get("error").is_some());
    }

    #[test]
    fn test_dispatch_malformed_json() {
        let state = make_state(100);
        let resp = state.dispatch("not json at all");
        let v: serde_json::Value = serde_json::from_slice(&resp).unwrap();
        assert!(v.get("error").is_some());
    }

    #[test]
    fn test_host_call_counter_limit() {
        let state = make_state(3);
        for i in 0..3 {
            let resp = state.dispatch(r#"{"method":"log","params":{"message":"ok"}}"#);
            let v: serde_json::Value = serde_json::from_slice(&resp).unwrap();
            assert!(v.get("ok").is_some(), "call {i} should succeed");
        }
        // 4th call should be rejected
        let resp = state.dispatch(r#"{"method":"log","params":{"message":"over"}}"#);
        let v: serde_json::Value = serde_json::from_slice(&resp).unwrap();
        let err = v.get("error").unwrap().as_str().unwrap();
        assert!(err.contains("limit exceeded"));
    }
}
