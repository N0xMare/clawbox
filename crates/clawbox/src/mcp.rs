//! MCP (Model Context Protocol) stdio server — bridges JSON-RPC over stdin/stdout
//! to a running clawbox HTTP server.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl JsonRpcResponse {
    fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

fn load_image_templates() -> HashMap<String, clawbox_server::config::ImageTemplate> {
    let path = super::config_path();
    if let Ok(content) = std::fs::read_to_string(&path)
        && let Ok(config) = toml::from_str::<clawbox_server::ClawboxConfig>(&content)
    {
        return config.images.templates;
    }
    HashMap::new()
}

fn synthetic_container_tools() -> Vec<Value> {
    vec![
        serde_json::json!({
            "name": "clawbox_spawn_container",
            "description": "Spawn a container from a named image template",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "template": {"type": "string", "description": "Name of the image template to use"},
                    "task": {"type": "string", "description": "Task description for the container"}
                },
                "required": ["template"]
            }
        }),
        serde_json::json!({
            "name": "clawbox_kill_container",
            "description": "Kill a running container and collect its output",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "container_id": {"type": "string", "description": "ID of the container to kill"}
                },
                "required": ["container_id"]
            }
        }),
        serde_json::json!({
            "name": "clawbox_list_containers",
            "description": "List all running containers",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
    ]
}

fn handle_initialize(id: Value) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": "clawbox",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
}

fn handle_ping(id: Value) -> JsonRpcResponse {
    JsonRpcResponse::success(id, serde_json::json!({}))
}

fn handle_tools_list(
    id: Value,
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
) -> JsonRpcResponse {
    let url = format!("{base_url}/tools");
    match client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
    {
        Ok(resp) => {
            match resp.json::<Value>() {
                Ok(tools_value) => {
                    let tools = if let Some(arr) = tools_value.as_array() {
                        arr.iter()
                            .map(|t| {
                                let name = t
                                    .get("tool")
                                    .and_then(|tool| tool.get("name"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                let desc = t
                                    .get("tool")
                                    .and_then(|tool| tool.get("description"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let mut schema = t
                                    .get("input_schema")
                                    .or_else(|| t.get("inputSchema"))
                                    .cloned()
                                    .unwrap_or_else(
                                        || serde_json::json!({"type": "object", "properties": {}}),
                                    );
                                // Ensure type field exists
                                if let Some(obj) = schema.as_object_mut() {
                                    obj.entry("type")
                                        .or_insert_with(|| Value::String("object".into()));
                                }
                                serde_json::json!({
                                    "name": name,
                                    "description": desc,
                                    "inputSchema": schema
                                })
                            })
                            .collect::<Vec<_>>()
                    } else {
                        vec![]
                    };
                    let mut tools = tools;
                    tools.extend(synthetic_container_tools());
                    JsonRpcResponse::success(id, serde_json::json!({ "tools": tools }))
                }
                Err(e) => {
                    eprintln!("mcp: failed to parse /tools response: {e}");
                    JsonRpcResponse::error(
                        id,
                        -32603,
                        format!("failed to parse tools response: {e}"),
                    )
                }
            }
        }
        Err(e) => {
            eprintln!("mcp: failed to reach server at {url}: {e}");
            JsonRpcResponse::error(id, -32603, format!("failed to reach clawbox server: {e}"))
        }
    }
}

fn handle_tools_call(
    id: Value,
    params: Option<Value>,
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
) -> JsonRpcResponse {
    let (tool_name, arguments) = match params {
        Some(ref p) => {
            let name = p
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args = p
                .get("arguments")
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::new()));
            (name, args)
        }
        None => {
            return JsonRpcResponse::success(
                id,
                serde_json::json!({
                    "content": [{"type": "text", "text": "Error: missing tool name"}],
                    "isError": true
                }),
            );
        }
    };

    if tool_name.is_empty() {
        return JsonRpcResponse::success(
            id,
            serde_json::json!({
                "content": [{"type": "text", "text": "Error: missing tool name"}],
                "isError": true
            }),
        );
    }

    // Handle synthetic container tools
    const CONTAINER_TOOLS: &[&str] = &[
        "clawbox_spawn_container",
        "clawbox_kill_container",
        "clawbox_list_containers",
    ];
    if CONTAINER_TOOLS.contains(&tool_name.as_str()) {
        return handle_container_tool(id, &tool_name, &arguments, client, base_url, token);
    }

    let url = format!("{base_url}/execute");
    let body = serde_json::json!({ "tool": tool_name, "params": arguments });

    match client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&body)
        .send()
    {
        Ok(resp) => {
            let status = resp.status();
            match resp.text() {
                Ok(text) => {
                    let is_error = !status.is_success();
                    JsonRpcResponse::success(
                        id,
                        serde_json::json!({
                            "content": [{"type": "text", "text": text}],
                            "isError": is_error
                        }),
                    )
                }
                Err(e) => JsonRpcResponse::success(
                    id,
                    serde_json::json!({
                        "content": [{"type": "text", "text": format!("Error reading response: {e}")}],
                        "isError": true
                    }),
                ),
            }
        }
        Err(e) => {
            eprintln!("mcp: failed to reach server for tool call: {e}");
            JsonRpcResponse::success(
                id,
                serde_json::json!({
                    "content": [{"type": "text", "text": format!("Error: failed to reach clawbox server: {e}")}],
                    "isError": true
                }),
            )
        }
    }
}

fn handle_container_tool(
    id: Value,
    tool_name: &str,
    arguments: &Value,
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
) -> JsonRpcResponse {
    let mcp_result = |text: String, is_error: bool| {
        JsonRpcResponse::success(
            id.clone(),
            serde_json::json!({
                "content": [{"type": "text", "text": text}],
                "isError": is_error
            }),
        )
    };

    match tool_name {
        "clawbox_spawn_container" => {
            let template_name = match arguments.get("template").and_then(|v| v.as_str()) {
                Some(n) => n.to_string(),
                None => {
                    return mcp_result("Error: missing required parameter 'template'".into(), true);
                }
            };
            let task = arguments
                .get("task")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let templates = load_image_templates();
            let template = match templates.get(&template_name) {
                Some(t) => t,
                None => {
                    let available: Vec<_> = templates.keys().collect();
                    return mcp_result(
                        format!(
                            "Error: template '{}' not found. Available templates: {:?}",
                            template_name, available
                        ),
                        true,
                    );
                }
            };

            let body = super::helpers::build_spawn_body(template, &task);

            let url = format!("{base_url}/containers/spawn");
            match client
                .post(&url)
                .header("Authorization", format!("Bearer {token}"))
                .json(&body)
                .send()
            {
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().unwrap_or_default();
                    mcp_result(text, !status.is_success())
                }
                Err(e) => mcp_result(format!("Error: {e}"), true),
            }
        }
        "clawbox_kill_container" => {
            let container_id = match arguments.get("container_id").and_then(|v| v.as_str()) {
                Some(cid) => cid,
                None => {
                    return mcp_result(
                        "Error: missing required parameter 'container_id'".into(),
                        true,
                    );
                }
            };
            let url = format!("{base_url}/containers/{container_id}");
            match client
                .delete(&url)
                .header("Authorization", format!("Bearer {token}"))
                .send()
            {
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().unwrap_or_default();
                    mcp_result(text, !status.is_success())
                }
                Err(e) => mcp_result(format!("Error: {e}"), true),
            }
        }
        "clawbox_list_containers" => {
            let url = format!("{base_url}/containers");
            match client
                .get(&url)
                .header("Authorization", format!("Bearer {token}"))
                .send()
            {
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().unwrap_or_default();
                    mcp_result(text, !status.is_success())
                }
                Err(e) => mcp_result(format!("Error: {e}"), true),
            }
        }
        _ => mcp_result(format!("Error: unknown container tool '{tool_name}'"), true),
    }
}

fn handle_request(
    req: &JsonRpcRequest,
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
) -> Option<JsonRpcResponse> {
    // Notifications (no id) don't get responses
    let id = match &req.id {
        Some(id) => id.clone(),
        None => {
            eprintln!("mcp: notification received: {}", req.method);
            return None;
        }
    };

    let resp = match req.method.as_str() {
        "initialize" => handle_initialize(id),
        "ping" => handle_ping(id),
        "tools/list" => handle_tools_list(id, client, base_url, token),
        "tools/call" => handle_tools_call(id, req.params.clone(), client, base_url, token),
        "resources/list" => JsonRpcResponse::success(id, serde_json::json!({"resources": []})),
        "prompts/list" => JsonRpcResponse::success(id, serde_json::json!({"prompts": []})),
        other => {
            eprintln!("mcp: unknown method: {other}");
            JsonRpcResponse::error(id, -32601, format!("method not found: {other}"))
        }
    };
    Some(resp)
}

pub fn run(host: &str, port: u16, token: &str) -> Result<()> {
    let base_url = format!("http://{host}:{port}");
    let client = reqwest::blocking::Client::new();

    eprintln!("mcp: clawbox MCP server started (server: {base_url})");

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut reader = BufReader::new(stdin.lock());
    const MAX_LINE_LEN: usize = 10 * 1024 * 1024; // 10 MB

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => {
                eprintln!("mcp: stdin read error: {e}");
                break;
            }
        }

        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        if line.len() > MAX_LINE_LEN {
            eprintln!("mcp: request too large ({} bytes)", line.len());
            let resp = JsonRpcResponse::error(Value::Null, -32700, "Request too large");
            let _ = writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap());
            let _ = stdout.flush();
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("mcp: failed to parse JSON-RPC: {e} — input: {line}");
                // Try to extract an id for error response
                if let Ok(v) = serde_json::from_str::<Value>(&line)
                    && let Some(id) = v.get("id").cloned()
                {
                    let resp = JsonRpcResponse::error(id, -32700, "parse error");
                    let _ = writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap());
                    let _ = stdout.flush();
                }
                continue;
            }
        };

        if let Some(resp) = handle_request(&req, &client, &base_url, token) {
            let json = serde_json::to_string(&resp)?;
            writeln!(stdout, "{json}")?;
            stdout.flush()?;
        }
    }

    eprintln!("mcp: stdin closed, shutting down");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize_response_format() {
        let resp = handle_initialize(Value::Number(1.into()));
        assert_eq!(resp.jsonrpc, "2.0");
        assert_eq!(resp.id, Value::Number(1.into()));
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], "2025-03-26");
        assert_eq!(result["serverInfo"]["name"], "clawbox");
        assert!(result["capabilities"]["tools"]["listChanged"].is_boolean());
    }

    #[test]
    fn test_ping_response() {
        let resp = handle_ping(Value::Number(42.into()));
        assert_eq!(resp.id, Value::Number(42.into()));
        let result = resp.result.unwrap();
        assert_eq!(result, serde_json::json!({}));
    }

    #[test]
    fn test_jsonrpc_parsing_all_methods() {
        let cases = vec![
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"msg":"hi"}}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        ];
        for input in cases {
            let req: JsonRpcRequest = serde_json::from_str(input).unwrap();
            assert_eq!(req.jsonrpc, "2.0");
        }
    }

    #[test]
    fn test_notification_no_response() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: None,
            method: "notifications/initialized".into(),
            params: None,
        };
        let client = reqwest::blocking::Client::new();
        let result = handle_request(&req, &client, "http://localhost:0", "fake");
        assert!(result.is_none());
    }

    #[test]
    fn test_unknown_method_returns_error() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(Value::Number(99.into())),
            method: "bogus/method".into(),
            params: None,
        };
        let client = reqwest::blocking::Client::new();
        let resp = handle_request(&req, &client, "http://localhost:0", "fake").unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[test]
    fn test_tools_call_missing_name() {
        let resp = handle_tools_call(
            Value::Number(1.into()),
            Some(serde_json::json!({})),
            &reqwest::blocking::Client::new(),
            "http://localhost:0",
            "fake",
        );
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], true);
    }
}
