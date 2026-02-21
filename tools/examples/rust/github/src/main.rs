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

fn error_json(msg: &str) -> String {
    json!({"error": msg}).to_string()
}

fn require<'a>(input: &'a Value, field: &str) -> Result<&'a str, String> {
    input[field].as_str().ok_or_else(|| format!("missing required field: {}", field))
}

fn main() {
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        println!("{}", error_json("failed to read stdin"));
        return;
    }

    let input: Value = match serde_json::from_str(&buf) {
        Ok(v) => v,
        Err(e) => { println!("{}", error_json(&e.to_string())); return; }
    };

    let result = run(&input);
    match result {
        Ok(v) => println!("{}", v),
        Err(e) => println!("{}", error_json(&e)),
    }
}

fn run(input: &Value) -> Result<String, String> {
    let action = require(input, "action")?;
    let state = input["state"].as_str().unwrap_or("open");
    let per_page = input["per_page"].as_u64().unwrap_or(10);

    let url = match action {
        "get_repo" => {
            let owner = require(input, "owner")?;
            let repo = require(input, "repo")?;
            format!("https://api.github.com/repos/{}/{}", owner, repo)
        }
        "list_issues" => {
            let owner = require(input, "owner")?;
            let repo = require(input, "repo")?;
            format!("https://api.github.com/repos/{}/{}/issues?state={}&per_page={}", owner, repo, state, per_page)
        }
        "get_issue" => {
            let owner = require(input, "owner")?;
            let repo = require(input, "repo")?;
            let number = input["number"].as_u64().ok_or("missing required field: number")?;
            format!("https://api.github.com/repos/{}/{}/issues/{}", owner, repo, number)
        }
        "list_pulls" => {
            let owner = require(input, "owner")?;
            let repo = require(input, "repo")?;
            format!("https://api.github.com/repos/{}/{}/pulls?state={}&per_page={}", owner, repo, state, per_page)
        }
        "get_pull" => {
            let owner = require(input, "owner")?;
            let repo = require(input, "repo")?;
            let number = input["number"].as_u64().ok_or("missing required field: number")?;
            format!("https://api.github.com/repos/{}/{}/pulls/{}", owner, repo, number)
        }
        "search_code" => {
            let query = require(input, "query")?;
            format!("https://api.github.com/search/code?q={}&per_page={}", query, per_page)
        }
        "search_repos" => {
            let query = require(input, "query")?;
            format!("https://api.github.com/search/repositories?q={}&per_page={}", query, per_page)
        }
        "list_releases" => {
            let owner = require(input, "owner")?;
            let repo = require(input, "repo")?;
            format!("https://api.github.com/repos/{}/{}/releases?per_page={}", owner, repo, per_page)
        }
        "get_contents" => {
            let owner = require(input, "owner")?;
            let repo = require(input, "repo")?;
            let path = require(input, "path")?;
            format!("https://api.github.com/repos/{}/{}/contents/{}", owner, repo, path)
        }
        _ => return Err(format!("unknown action: {}", action)),
    };

    let params = json!({
        "method": "GET",
        "url": url,
        "headers": {
            "Accept": "application/vnd.github+json",
            "User-Agent": "clawbox-github-tool/0.1.0",
            "X-GitHub-Api-Version": "2022-11-28"
        }
    });

    let response = call_host("http_request", &params)?;

    // Pass through the body directly
    if let Some(body) = response.get("body") {
        if let Some(s) = body.as_str() {
            Ok(s.to_string())
        } else {
            Ok(body.to_string())
        }
    } else {
        Ok(response.to_string())
    }
}
