//! Shared helpers for CLI and MCP server.

use clawbox_server::config::ImageTemplate;

/// Build the JSON body for spawning a container from a template.
pub fn build_spawn_body(template: &ImageTemplate, task: &str) -> serde_json::Value {
    let mut body = serde_json::json!({
        "task": task,
        "image": template.image,
        "capabilities": {
            "network_allowlist": template.network_allowlist,
            "credentials": template.credentials,
        }
    });
    if let Some(ref cmd) = template.command {
        body["command"] = serde_json::json!(cmd);
    }
    if let Some(ms) = template.max_lifetime_ms {
        body["max_lifetime_ms"] = serde_json::json!(ms);
    }
    body
}
