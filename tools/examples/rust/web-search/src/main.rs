use serde_json::{json, Value};
use std::io::Read;

#[link(wasm_import_module = "clawbox")]
extern "C" {
    fn host_call(
        request_ptr: *const u8,
        request_len: i32,
        response_ptr: *mut u8,
        response_cap: i32,
    ) -> i32;
}

fn call_host(method: &str, params: &Value) -> Result<Value, String> {
    let request = json!({ "method": method, "params": params });
    let request_bytes = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
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

fn log(level: &str, message: &str) {
    let _ = call_host("log", &json!({ "level": level, "message": message }));
}

fn output_error(msg: &str) {
    let out = json!({ "error": msg });
    print!("{}", out);
}

fn run() -> Result<(), String> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).map_err(|e| e.to_string())?;

    let params: Value = serde_json::from_str(&input).map_err(|e| e.to_string())?;

    let query = params["query"].as_str().ok_or("missing 'query' parameter")?;
    let count = params["count"].as_u64().unwrap_or(5).min(10).max(1);

    let encoded_query = query.replace(' ', "+")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('#', "%23");

    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
        encoded_query, count
    );

    log("debug", &format!("Searching Brave: {}", url));

    let response = call_host("http_request", &json!({
        "method": "GET",
        "url": url,
        "credential": "brave_search"
    }))?;

    // Parse body - could be string or already parsed
    let body: Value = if let Some(body_str) = response.get("body").and_then(|b| b.as_str()) {
        serde_json::from_str(body_str).map_err(|e| format!("failed to parse response body: {}", e))?
    } else if let Some(body_val) = response.get("body") {
        body_val.clone()
    } else {
        return Err("no body in response".to_string());
    };

    let empty_vec = vec![];
    let web_results = body.get("web")
        .and_then(|w| w.get("results"))
        .and_then(|r| r.as_array())
        .unwrap_or(&empty_vec);

    let results: Vec<Value> = web_results.iter().map(|r| {
        json!({
            "title": r.get("title").and_then(|t| t.as_str()).unwrap_or(""),
            "url": r.get("url").and_then(|u| u.as_str()).unwrap_or(""),
            "snippet": r.get("description").and_then(|d| d.as_str()).unwrap_or("")
        })
    }).collect();

    let output = json!({
        "results": results,
        "total_results": results.len()
    });

    print!("{}", output);
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        output_error(&e);
    }
}
