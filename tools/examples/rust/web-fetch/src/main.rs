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

fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let chars: Vec<char> = html.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '<' {
            // Check for script/style blocks
            if i + 7 < len {
                let tag: String = chars[i..std::cmp::min(i + 8, len)].iter().collect();
                let tag_lower = tag.to_lowercase();
                if tag_lower.starts_with("<script") || tag_lower.starts_with("<style") {
                    let close_tag = if tag_lower.starts_with("<script") {
                        "</script>"
                    } else {
                        "</style>"
                    };
                    let rest: String = chars[i..].iter().collect();
                    if let Some(pos) = rest.to_lowercase().find(close_tag) {
                        i += pos + close_tag.len();
                        continue;
                    }
                }
            }
            // Skip tag
            while i < len && chars[i] != '>' {
                i += 1;
            }
            i += 1;
            result.push(' ');
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    // Collapse whitespace
    let mut collapsed = String::with_capacity(result.len());
    let mut prev_space = false;
    for ch in result.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                collapsed.push(' ');
            }
            prev_space = true;
        } else {
            collapsed.push(ch);
            prev_space = false;
        }
    }
    collapsed.trim().to_string()
}

fn run() -> Result<(), String> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).map_err(|e| e.to_string())?;
    let params: Value = serde_json::from_str(&input).map_err(|e| e.to_string())?;

    let url = params["url"].as_str().ok_or("missing url")?;
    let max_length = params
        .get("max_length")
        .and_then(|v| v.as_u64())
        .unwrap_or(50000) as usize;

    log("debug", &format!("Fetching URL: {}", url));

    let response = call_host(
        "http_request",
        &json!({
            "url": url,
            "method": "GET"
        }),
    )?;

    let body = response.get("body").and_then(|v| v.as_str()).unwrap_or("");
    let content_type = response
        .get("headers")
        .and_then(|h| h.get("content-type"))
        .and_then(|v| v.as_str())
        .unwrap_or("text/plain")
        .to_string();

    let is_html = content_type.contains("text/html");
    let mut content = if is_html {
        strip_html(body)
    } else {
        body.to_string()
    };

    let truncated = content.len() > max_length;
    if truncated {
        content.truncate(max_length);
    }
    let length = content.len();

    let output = json!({
        "url": url,
        "content_type": content_type,
        "content": content,
        "length": length,
        "truncated": truncated,
    });
    print!(
        "{}",
        serde_json::to_string(&output).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        let output = json!({ "error": e });
        print!(
            "{}",
            serde_json::to_string(&output)
                .unwrap_or_else(|_| format!("{{\"error\":\"{}\"}}", e))
        );
    }
}
