//! Echo tool — reads JSON from stdin, echoes it back with metadata.
//! This is a pure-computation WASM tool for testing the clawbox pipeline.

use serde_json::Value;
use std::io::{self, Read, Write};

fn main() {
    // Read input from stdin (clawbox passes params as JSON)
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap_or_default();

    let params: Value = serde_json::from_str(&input).unwrap_or(Value::Null);

    // Build response
    let response = serde_json::json!({
        "tool": "echo",
        "version": "0.1.0",
        "echo": params,
        "message": "Hello from clawbox WASM sandbox!"
    });

    // Write JSON output to stdout
    let output = serde_json::to_string(&response).unwrap();
    io::stdout().write_all(output.as_bytes()).unwrap();
}
