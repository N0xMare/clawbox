//! HTTP request tool — makes proxied HTTP requests via clawbox host_call.
//!
//! Input (stdin JSON):
//! { "url": "https://api.github.com/repos/clawbox/clawbox", "method": "GET", "headers": {}, "body": null }
//!
//! Output (stdout JSON):
//! { "status": 200, "headers": {...}, "body": "..." }

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, Read, Write};

// Host function import — the only way to talk to the outside world
#[link(wasm_import_module = "clawbox")]
extern "C" {
    fn host_call(
        request_ptr: *const u8,
        request_len: i32,
        response_ptr: *mut u8,
        response_cap: i32,
    ) -> i32;
}

/// Make a host call and get the JSON response
fn call_host(method: &str, params: &Value) -> Result<Value, String> {
    let request = serde_json::json!({
        "method": method,
        "params": params
    });
    let request_bytes = serde_json::to_vec(&request).map_err(|e| e.to_string())?;

    // 1MB response buffer
    let mut response_buf = vec![0u8; 1024 * 1024];

    let bytes_written = unsafe {
        host_call(
            request_bytes.as_ptr(),
            request_bytes.len() as i32,
            response_buf.as_mut_ptr(),
            response_buf.len() as i32,
        )
    };

    if bytes_written < 0 {
        return Err("host_call failed".to_string());
    }

    let response_str = std::str::from_utf8(&response_buf[..bytes_written as usize])
        .map_err(|e| e.to_string())?;

    let response: Value = serde_json::from_str(response_str).map_err(|e| e.to_string())?;

    if let Some(ok) = response.get("ok") {
        Ok(ok.clone())
    } else if let Some(err) = response.get("error") {
        Err(err.as_str().unwrap_or("unknown error").to_string())
    } else {
        Ok(response)
    }
}

/// Log a message via host_call
fn log(level: &str, message: &str) {
    let _ = call_host(
        "log",
        &serde_json::json!({ "level": level, "message": message }),
    );
}

#[derive(Deserialize)]
struct HttpParams {
    url: String,
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    headers: Value,
    #[serde(default)]
    body: Option<String>,
}

fn default_method() -> String {
    "GET".to_string()
}

#[derive(Serialize)]
struct HttpResponse {
    status: u16,
    headers: Value,
    body: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

fn main() {
    // Read params from stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap_or_default();

    let params: HttpParams = match serde_json::from_str(&input) {
        Ok(p) => p,
        Err(e) => {
            let error = ErrorResponse {
                error: format!("invalid input: {e}"),
            };
            let out = serde_json::to_string(&error).unwrap();
            io::stdout().write_all(out.as_bytes()).unwrap();
            return;
        }
    };

    log("info", &format!("HTTP {} {}", params.method, params.url));

    // Make the HTTP request via host_call
    let host_params = serde_json::json!({
        "url": params.url,
        "method": params.method,
        "headers": params.headers,
        "body": params.body
    });

    let output = match call_host("http_request", &host_params) {
        Ok(response) => serde_json::to_string(&response).unwrap(),
        Err(e) => {
            log("error", &format!("HTTP request failed: {e}"));
            let error = ErrorResponse { error: e };
            serde_json::to_string(&error).unwrap()
        }
    };

    io::stdout().write_all(output.as_bytes()).unwrap();
}
