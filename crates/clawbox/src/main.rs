//! clawbox — Sandboxed agent execution service.
mod build;
mod helpers;
mod mcp;
mod scaffold;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

use serde_json::Value;
use toml_edit::DocumentMut;

#[derive(Parser)]
#[command(name = "clawbox", about = "Sandboxed agent execution service", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize clawbox — creates config, generates auth token, sets up directories.
    Init {
        /// Directory to initialize (defaults to ~/.clawbox).
        #[arg(long)]
        dir: Option<String>,
    },
    /// Start the clawbox HTTP server.
    Serve {
        /// Path to config file. Defaults to ~/.clawbox/config/clawbox.toml if it exists, otherwise config/default.toml.
        #[arg(long)]
        config: Option<String>,
        /// Allow insecure defaults (e.g. 'changeme' auth token) for development.
        #[arg(long, default_value_t = false)]
        insecure: bool,
    },
    /// Check service health.
    Health {
        /// Host to query.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to query.
        #[arg(long, default_value_t = 9800)]
        port: u16,
    },
    /// Execute a tool on a running clawbox server.
    Run {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 9800)]
        port: u16,
        #[arg(long)]
        token: Option<String>,
        /// Tool name to execute.
        tool: String,
        /// JSON parameters (or "-" for stdin).
        params: Option<String>,
    },
    /// Manage tools on a running server.
    Tools {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 9800)]
        port: u16,
        #[arg(long)]
        token: Option<String>,
        #[command(subcommand)]
        action: ToolsAction,
    },
    /// Show server status.
    Status {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 9800)]
        port: u16,
    },
    /// Read audit logs.
    Logs {
        /// Path to audit log file.
        #[arg(long, default_value = "./audit/proxy.jsonl")]
        path: String,
        /// Follow the log (like tail -f).
        #[arg(long)]
        follow: bool,
        /// Show last N lines.
        #[arg(long, default_value_t = 20)]
        tail: usize,
    },
    /// Start an MCP (Model Context Protocol) server for tool integration with AI assistants.
    Mcp {
        /// clawbox server host.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// clawbox server port.
        #[arg(long, default_value_t = 9800)]
        port: u16,
        /// Bearer token for authentication. If not provided, reads from CLAWBOX_AUTH_TOKEN env var.
        #[arg(long)]
        token: Option<String>,
    },
    /// Add a tool or image template.
    Add {
        #[command(subcommand)]
        action: AddAction,
    },
    /// Remove a tool or image template.
    Remove {
        #[command(subcommand)]
        action: RemoveAction,
    },
    /// List tools, images, or containers.
    List {
        #[command(subcommand)]
        action: ListAction,
    },
    /// Spawn a container from a named image template.
    Spawn {
        /// Template name from config.
        template: String,
        /// Task description for the container.
        #[arg(long)]
        task: Option<String>,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 9800)]
        port: u16,
        #[arg(long)]
        token: Option<String>,
    },
    /// Kill a running container.
    Kill {
        /// Container ID.
        container_id: String,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 9800)]
        port: u16,
        #[arg(long)]
        token: Option<String>,
    },
    /// Scaffold a new WASM tool project (hidden alias for `add tool`).
    #[command(name = "new-tool", hide = true)]
    NewTool {
        /// Tool name (e.g., "my-tool").
        name: String,
        /// Language: rust, js, or ts.
        #[arg(long, default_value = "rust")]
        lang: String,
        /// Output directory (default: current directory).
        #[arg(long)]
        dir: Option<String>,
    },
    /// Build WASM tools and copy to the tools directory.
    Build {
        /// Specific tool to build (builds all if omitted).
        tool: Option<String>,
        /// Tools source directory.
        #[arg(long, default_value = "tools/examples")]
        source: String,
        /// Output directory for compiled .wasm files.
        #[arg(long, default_value = "tools/wasm")]
        output: String,
    },
    Creds {
        #[arg(long, default_value = "config/default.toml")]
        config: String,
        #[command(subcommand)]
        action: CredsAction,
    },
}

#[derive(Subcommand)]
enum CredsAction {
    /// Add a credential (value is read from stdin for security).
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        domain: String,
        #[arg(long, default_value = "Authorization")]
        header: String,
        #[arg(long, default_value = "Bearer ")]
        prefix: String,
    },
    /// Remove a credential.
    Remove {
        #[arg(long)]
        name: String,
    },
    /// List stored credentials.
    List,
}

#[derive(Subcommand)]
enum ToolsAction {
    /// List registered tools.
    List,
    /// Register a tool from a JSON manifest file.
    Register {
        /// Path to the manifest JSON file.
        manifest: String,
    },
    /// Trigger hot-reload of WASM tools.
    Reload,
}

#[derive(Subcommand)]
enum AddAction {
    /// Scaffold a new WASM tool project.
    Tool {
        /// Tool name (e.g., "my-tool").
        name: String,
        /// Language: rust, js, or ts. Defaults to config value or "rust".
        #[arg(long)]
        lang: Option<String>,
        /// Output directory (default: current directory).
        #[arg(long)]
        dir: Option<String>,
    },
    /// Add a named Docker image template to the config.
    Image {
        /// Template name.
        name: String,
        /// Docker image name and tag.
        #[arg(long)]
        image: String,
        /// Comma-separated network allowlist domains.
        #[arg(long)]
        network: Option<String>,
        /// Comma-separated credential names.
        #[arg(long)]
        credentials: Option<String>,
        /// Human-readable description.
        #[arg(long)]
        description: Option<String>,
    },
}

#[derive(Subcommand)]
enum RemoveAction {
    /// Delete a compiled WASM tool.
    Tool {
        /// Tool name.
        name: String,
    },
    /// Remove an image template from config.
    Image {
        /// Template name.
        name: String,
    },
}

#[derive(Subcommand)]
enum ListAction {
    /// List registered tools on a running server.
    Tools {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 9800)]
        port: u16,
        #[arg(long)]
        token: Option<String>,
    },
    /// List image templates from config.
    Images,
    /// List running containers on a running server.
    Containers {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 9800)]
        port: u16,
        #[arg(long)]
        token: Option<String>,
    },
}

fn resolve_auth_token(token_flag: &Option<String>) -> Result<String> {
    if let Some(t) = token_flag {
        return Ok(t.clone());
    }
    std::env::var("CLAWBOX_AUTH_TOKEN")
        .map_err(|_| anyhow::anyhow!("No auth token: pass --token or set CLAWBOX_AUTH_TOKEN"))
}

fn format_log_line(line: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(line) {
        let ts = v.get("timestamp").and_then(|v| v.as_str()).unwrap_or("?");
        let method = v.get("method").and_then(|v| v.as_str()).unwrap_or("?");
        let url = v.get("url").and_then(|v| v.as_str()).unwrap_or("?");
        let status = v
            .get("status")
            .and_then(|v| v.as_u64())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "?".into());
        let dur = v
            .get("duration_ms")
            .and_then(|v| v.as_u64())
            .map(|d| format!("{d}ms"))
            .unwrap_or_else(|| "?".into());
        format!("[{ts}] {method} {url} → {status} ({dur})")
    } else {
        line.to_string()
    }
}

fn load_master_key() -> Result<[u8; 32]> {
    let hex = std::env::var("CLAWBOX_MASTER_KEY").map_err(|_| {
        anyhow::anyhow!(
            "CLAWBOX_MASTER_KEY not set.\n\
             Generate one with: openssl rand -hex 32\n\
             Then export it:    export CLAWBOX_MASTER_KEY=<64-char-hex>"
        )
    })?;
    Ok(clawbox_proxy::parse_master_key(&hex)?)
}

fn load_default_language() -> String {
    if let Ok(config) = load_config() {
        return config.tools.default_language;
    }
    "rust".into()
}

fn config_path() -> String {
    std::env::var("HOME")
        .map(|h| format!("{h}/.clawbox/config/clawbox.toml"))
        .unwrap_or_else(|_| "/root/.clawbox/config/clawbox.toml".into())
}

fn load_config() -> Result<clawbox_server::ClawboxConfig> {
    let path = config_path();
    let content = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Cannot read config at {path}: {e}"))?;
    toml::from_str(&content).map_err(|e| anyhow::anyhow!("Cannot parse config: {e}"))
}

fn load_config_raw() -> Result<DocumentMut> {
    let path = config_path();
    let content = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Cannot read config at {path}: {e}"))?;
    content
        .parse::<DocumentMut>()
        .map_err(|e| anyhow::anyhow!("Cannot parse config: {e}"))
}

fn save_config_raw(doc: &DocumentMut) -> Result<()> {
    let path = config_path();
    let content = doc.to_string();
    let tmp_path = format!("{}.tmp", path);
    std::fs::write(&tmp_path, &content)?;
    std::fs::rename(&tmp_path, &path)?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,clawbox=debug"));
    // D3: Use text format when stderr is a TTY, JSON otherwise
    if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        fmt().with_env_filter(env_filter).init();
    } else {
        fmt().with_env_filter(env_filter).json().init();
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Init { dir } => {
            let base = dir.unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
                format!("{home}/.clawbox")
            });
            let base = std::path::Path::new(&base);

            // Create directories
            let dirs = ["config", "tools", "audit"];
            for d in &dirs {
                std::fs::create_dir_all(base.join(d))?;
            }

            // Generate secure auth token
            let mut token_bytes = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::rng(), &mut token_bytes);
            let token: String = token_bytes.iter().map(|b| format!("{b:02x}")).collect();

            // Write config
            let config_path = base.join("config/clawbox.toml");
            if !config_path.exists() {
                let config_content = format!(
                    r#"[server]
host = "127.0.0.1"
port = 9800
auth_token = "{token}"

[sandbox]
tool_dir = "{tools_dir}"
default_fuel = 100000000
default_timeout_ms = 30000

[proxy]
max_response_bytes = 1048576
default_timeout_ms = 30000

[credentials]
store_path = "{creds_path}"

[logging]
format = "json"
level = "info"
audit_dir = "{audit_dir}"

[tools]
default_language = "rust"

# [images.templates.researcher]
# image = "ghcr.io/your-org/agent:latest"
# description = "Research agent"
# network_allowlist = ["api.github.com", "*.wikipedia.org"]
# credentials = ["GITHUB_TOKEN"]

[containers]
max_containers = 10
default_image = "ghcr.io/n0xmare/clawbox-agent:latest"
"#,
                    tools_dir = base.join("tools").display(),
                    creds_path = base.join("credentials.enc").display(),
                    audit_dir = base.join("audit").display(),
                );
                std::fs::write(&config_path, config_content)?;
            }

            eprintln!("✓ Initialized clawbox at {}", base.display());
            eprintln!("  Config:    {}", config_path.display());
            eprintln!("  Tools dir: {}", base.join("tools").display());
            eprintln!("  Auth token: {token}");

            // Write token to a separate file for easy retrieval
            let token_path = base.join("token");
            if token_path.exists() {
                eprintln!(
                    "  Token file: {} (already exists, not overwritten)",
                    token_path.display()
                );
            } else {
                std::fs::write(&token_path, &token)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600))?;
                }
                eprintln!("  Token file: {}", token_path.display());
            }
            eprintln!();
            eprintln!("Start the server:");
            eprintln!("  clawbox serve --config {}", config_path.display());
        }
        Commands::Serve {
            config: config_path,
            insecure,
        } => {
            let config_path = match config_path {
                Some(p) => p,
                None => {
                    let home_config = std::env::var("HOME")
                        .ok()
                        .map(|h| {
                            std::path::PathBuf::from(h)
                                .join(".clawbox")
                                .join("config")
                                .join("clawbox.toml")
                        })
                        .filter(|p| p.exists());
                    match home_config {
                        Some(p) => {
                            eprintln!("Using config: {}", p.display());
                            p.to_string_lossy().into_owned()
                        }
                        None => "config/default.toml".to_string(),
                    }
                }
            };
            let mut config = clawbox_server::ClawboxConfig::load(&config_path)?;
            config.apply_env_overrides();
            config.expand_paths();
            config.validate()?;

            if config.server.auth_token == "changeme" && !insecure {
                eprintln!(
                    "ERROR: auth_token is 'changeme'. Set a real token in config or use --insecure"
                );
                std::process::exit(1);
            }

            let addr = format!("{}:{}", config.server.host, config.server.port);
            let state = Arc::new(clawbox_server::AppState::new(config).await?);
            let state_for_shutdown = Arc::clone(&state);
            let app = clawbox_server::build_router(state);

            let listener = tokio::net::TcpListener::bind(&addr).await?;
            info!("clawbox listening on {addr}");

            // Spawn Unix socket listener if configured
            if let Some(ref socket_path) = state_for_shutdown.config.server.unix_socket {
                clawbox_server::spawn_unix_listener(socket_path, app.clone()).await?;
                info!("clawbox Unix socket at {socket_path}");
            }

            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal(state_for_shutdown))
                .await?;
        }
        Commands::Health { host, port } => {
            let url = format!("http://{host}:{port}/health");
            let resp = reqwest::get(&url).await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                eprintln!("Error: server returned {status}: {body}");
                std::process::exit(1);
            }
            let body: serde_json::Value = resp.json().await?;
            println!("{}", serde_json::to_string_pretty(&body)?);
        }
        Commands::Run {
            host,
            port,
            token,
            tool,
            params,
        } => {
            let auth = resolve_auth_token(&token)?;
            let params_value: Value = match params.as_deref() {
                Some("-") => {
                    let mut buf = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
                    serde_json::from_str(&buf)?
                }
                Some(json_str) => serde_json::from_str(json_str)?,
                None => Value::Object(serde_json::Map::new()),
            };
            let url = format!("http://{host}:{port}/execute");
            let body = serde_json::json!({ "tool": tool, "params": params_value });
            let client = reqwest::Client::new();
            let resp = client
                .post(&url)
                .header("Authorization", format!("Bearer {auth}"))
                .json(&body)
                .send()
                .await;
            match resp {
                Ok(r) => {
                    let status = r.status();
                    let body: Value = r.json().await.unwrap_or(Value::Null);
                    println!("{}", serde_json::to_string_pretty(&body)?);
                    if !status.is_success() {
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!(
                        "Error: Could not connect to clawbox server. Is it running? Start with: clawbox serve\n\nDetails: {e}"
                    );
                    std::process::exit(1);
                }
            }
        }
        Commands::Tools {
            host,
            port,
            token,
            action: tools_action,
        } => {
            let auth = resolve_auth_token(&token)?;
            let client = reqwest::Client::new();
            let base = format!("http://{host}:{port}");
            match tools_action {
                ToolsAction::List => {
                    let resp = client
                        .get(format!("{base}/tools"))
                        .header("Authorization", format!("Bearer {auth}"))
                        .send()
                        .await;
                    match resp {
                        Ok(r) => {
                            if !r.status().is_success() {
                                let status = r.status();
                                let body = r.text().await.unwrap_or_default();
                                eprintln!("Error: server returned {status}: {body}");
                                std::process::exit(1);
                            }
                            let body: Value = r.json().await.unwrap_or(Value::Null);
                            if let Some(tools) = body.as_array() {
                                println!("{:<24} {:<10} DESCRIPTION", "NAME", "VERSION");
                                println!("{}", "-".repeat(60));
                                for t in tools {
                                    let tool = t.get("tool");
                                    let name = tool
                                        .and_then(|t| t.get("name"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("?");
                                    let ver = tool
                                        .and_then(|t| t.get("version"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("?");
                                    let desc = tool
                                        .and_then(|t| t.get("description"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    println!("{name:<24} {ver:<10} {desc}");
                                }
                            } else {
                                println!("{}", serde_json::to_string_pretty(&body)?);
                            }
                        }
                        Err(e) => {
                            eprintln!("Error: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                ToolsAction::Register { manifest } => {
                    let content = std::fs::read_to_string(&manifest)?;
                    let body: Value = serde_json::from_str(&content)?;
                    let resp = client
                        .post(format!("{base}/tools/register"))
                        .header("Authorization", format!("Bearer {auth}"))
                        .json(&body)
                        .send()
                        .await;
                    match resp {
                        Ok(r) => {
                            if !r.status().is_success() {
                                let status = r.status();
                                let body = r.text().await.unwrap_or_default();
                                eprintln!("Error: server returned {status}: {body}");
                                std::process::exit(1);
                            }
                            let body: Value = r.json().await.unwrap_or(Value::Null);
                            println!("{}", serde_json::to_string_pretty(&body)?);
                        }
                        Err(e) => {
                            eprintln!("Error: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                ToolsAction::Reload => {
                    let resp = client
                        .post(format!("{base}/tools/reload"))
                        .header("Authorization", format!("Bearer {auth}"))
                        .send()
                        .await;
                    match resp {
                        Ok(r) => {
                            if !r.status().is_success() {
                                let status = r.status();
                                let body = r.text().await.unwrap_or_default();
                                eprintln!("Error: server returned {status}: {body}");
                                std::process::exit(1);
                            }
                            let body: Value = r.json().await.unwrap_or(Value::Null);
                            println!("{}", serde_json::to_string_pretty(&body)?);
                        }
                        Err(e) => {
                            eprintln!("Error: {e}");
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
        Commands::Status { host, port } => {
            let url = format!("http://{host}:{port}/health");
            match reqwest::get(&url).await {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        eprintln!("Error: server returned {status}: {body}");
                        std::process::exit(1);
                    }
                    let body: Value = resp.json().await.unwrap_or(Value::Null);
                    println!("=== clawbox status ===\n");
                    if let Some(obj) = body.as_object() {
                        if let Some(v) = obj.get("version").and_then(|v| v.as_str()) {
                            println!("  Version: {v}");
                        }
                        if let Some(u) = obj.get("uptime_seconds").and_then(|v| v.as_u64()) {
                            println!("  Uptime:  {u}s");
                        }
                        if let Some(s) = obj.get("status").and_then(|v| v.as_str()) {
                            println!("  Status:  {s}");
                        }
                        if let Some(wasm) = obj.get("wasm") {
                            println!("\n  WASM Engine:");
                            if let Some(count) = wasm.get("tools_loaded").and_then(|v| v.as_u64()) {
                                println!("    Tools loaded: {count}");
                            }
                            if let Some(s) = wasm.get("status").and_then(|v| v.as_str()) {
                                println!("    Status: {s}");
                            }
                        }
                        if let Some(docker) = obj.get("docker") {
                            println!("\n  Docker:");
                            if let Some(c) =
                                docker.get("active_containers").and_then(|v| v.as_u64())
                            {
                                println!("    Active containers: {c}");
                            }
                        }
                        // Print any remaining fields
                        println!();
                    } else {
                        println!("{}", serde_json::to_string_pretty(&body)?);
                    }
                }
                Err(e) => {
                    eprintln!("Error: could not connect to server: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Logs { path, follow, tail } => {
            let log_path = std::path::Path::new(&path);
            if !log_path.exists() {
                eprintln!("Log file not found: {path}");
                std::process::exit(1);
            }
            let content = std::fs::read_to_string(log_path)?;
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(tail);
            for line in &lines[start..] {
                println!("{}", format_log_line(line));
            }
            if follow {
                use std::io::{BufRead, Seek};
                let mut file = std::fs::File::open(log_path)?;
                file.seek(std::io::SeekFrom::End(0))?;
                let mut reader = std::io::BufReader::new(file);
                loop {
                    let mut line = String::new();
                    let bytes = reader.read_line(&mut line)?;
                    if bytes > 0 {
                        print!("{}", format_log_line(line.trim_end()));
                        println!();
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                }
            }
        }
        Commands::Add { action } => match action {
            AddAction::Tool { name, lang, dir } => {
                let lang = lang.unwrap_or_else(load_default_language);
                scaffold::scaffold(&name, &lang, dir.as_deref())?;
            }
            AddAction::Image {
                name,
                image,
                network,
                credentials,
                description,
            } => {
                let mut doc = load_config_raw()?;
                doc["images"]["templates"][&name]["image"] = toml_edit::value(&image);
                if let Some(desc) = description {
                    doc["images"]["templates"][&name]["description"] = toml_edit::value(&desc);
                }
                if let Some(net) = network {
                    let mut arr = toml_edit::Array::new();
                    for s in net.split(',') {
                        arr.push(s.trim());
                    }
                    doc["images"]["templates"][&name]["network_allowlist"] = toml_edit::value(arr);
                }
                if let Some(creds) = credentials {
                    let mut arr = toml_edit::Array::new();
                    for s in creds.split(',') {
                        arr.push(s.trim());
                    }
                    doc["images"]["templates"][&name]["credentials"] = toml_edit::value(arr);
                }
                save_config_raw(&doc)?;
                eprintln!("✓ Image template '{name}' added to config");
            }
        },
        Commands::Remove { action } => match action {
            RemoveAction::Tool { name } => {
                if name.contains('/') || name.contains('\\') || name.contains("..") {
                    anyhow::bail!("Invalid tool name: must not contain path separators");
                }
                let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
                let wasm_path = format!("{home}/.clawbox/tools/{name}.wasm");
                if std::path::Path::new(&wasm_path).exists() {
                    std::fs::remove_file(&wasm_path)?;
                    eprintln!("✓ Removed {wasm_path}");
                } else {
                    eprintln!("Tool wasm not found: {wasm_path}");
                    std::process::exit(1);
                }
            }
            RemoveAction::Image { name } => {
                let mut doc = load_config_raw()?;
                let removed = doc
                    .get("images")
                    .and_then(|i| i.get("templates"))
                    .and_then(|t| t.get(&name))
                    .is_some();
                if removed {
                    doc["images"]["templates"]
                        .as_table_like_mut()
                        .ok_or_else(|| anyhow::anyhow!("[images.templates] is not a table"))?
                        .remove(&name);
                    save_config_raw(&doc)?;
                    eprintln!("✓ Image template '{name}' removed from config");
                } else {
                    eprintln!("Image template '{name}' not found in config");
                    std::process::exit(1);
                }
            }
        },
        Commands::List { action } => match action {
            ListAction::Tools { host, port, token } => {
                let auth = resolve_auth_token(&token)?;
                let client = reqwest::Client::new();
                let resp = client
                    .get(format!("http://{host}:{port}/tools"))
                    .header("Authorization", format!("Bearer {auth}"))
                    .send()
                    .await;
                match resp {
                    Ok(r) => {
                        if !r.status().is_success() {
                            let status = r.status();
                            let body = r.text().await.unwrap_or_default();
                            eprintln!("Error: server returned {status}: {body}");
                            std::process::exit(1);
                        }
                        let body: Value = r.json().await.unwrap_or(Value::Null);
                        if let Some(tools) = body.as_array() {
                            println!("{:<24} {:<10} DESCRIPTION", "NAME", "VERSION");
                            println!("{}", "-".repeat(60));
                            for t in tools {
                                let tool = t.get("tool");
                                let name = tool
                                    .and_then(|t| t.get("name"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("?");
                                let ver = tool
                                    .and_then(|t| t.get("version"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("?");
                                let desc = tool
                                    .and_then(|t| t.get("description"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                println!("{name:<24} {ver:<10} {desc}");
                            }
                        } else {
                            println!("{}", serde_json::to_string_pretty(&body)?);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
            ListAction::Images => {
                let config = load_config()?;
                if config.images.templates.is_empty() {
                    println!("No image templates configured.");
                } else {
                    println!("{:<20} {:<40} DESCRIPTION", "NAME", "IMAGE");
                    println!("{}", "-".repeat(80));
                    for (name, tmpl) in &config.images.templates {
                        println!("{:<20} {:<40} {}", name, tmpl.image, tmpl.description);
                    }
                }
            }
            ListAction::Containers { host, port, token } => {
                let auth = resolve_auth_token(&token)?;
                let client = reqwest::Client::new();
                let resp = client
                    .get(format!("http://{host}:{port}/containers"))
                    .header("Authorization", format!("Bearer {auth}"))
                    .send()
                    .await;
                match resp {
                    Ok(r) => {
                        let status = r.status();
                        if !status.is_success() {
                            let text = r.text().await.unwrap_or_default();
                            eprintln!("Error (HTTP {status}): {text}");
                            std::process::exit(1);
                        }
                        let body: Value = r.json().await.unwrap_or(Value::Null);
                        println!("{}", serde_json::to_string_pretty(&body)?);
                    }
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                }
            }
        },
        Commands::Spawn {
            template,
            task,
            host,
            port,
            token,
        } => {
            let config = load_config()?;
            let tmpl =
                config.images.templates.get(&template).ok_or_else(|| {
                    anyhow::anyhow!("Image template {template} not found in config")
                })?;
            let auth = resolve_auth_token(&token)?;
            let body = helpers::build_spawn_body(tmpl, &task.unwrap_or_default());
            let client = reqwest::Client::new();
            let resp = client
                .post(format!("http://{host}:{port}/containers/spawn"))
                .header("Authorization", format!("Bearer {auth}"))
                .json(&body)
                .send()
                .await;
            match resp {
                Ok(r) => {
                    let status = r.status();
                    let body: Value = r.json().await.unwrap_or(Value::Null);
                    println!("{}", serde_json::to_string_pretty(&body)?);
                    if !status.is_success() {
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Kill {
            container_id,
            host,
            port,
            token,
        } => {
            let auth = resolve_auth_token(&token)?;
            let client = reqwest::Client::new();
            let resp = client
                .delete(format!("http://{host}:{port}/containers/{container_id}"))
                .header("Authorization", format!("Bearer {auth}"))
                .send()
                .await;
            match resp {
                Ok(r) => {
                    let status = r.status();
                    let body: Value = r.json().await.unwrap_or(Value::Null);
                    println!("{}", serde_json::to_string_pretty(&body)?);
                    if !status.is_success() {
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::NewTool { name, lang, dir } => {
            scaffold::scaffold(&name, &lang, dir.as_deref())?;
        }
        Commands::Build {
            tool,
            source,
            output,
        } => {
            build::build(tool.as_deref(), &source, &output)?;
        }
        Commands::Mcp { host, port, token } => {
            let token = token
                .or_else(|| std::env::var("CLAWBOX_AUTH_TOKEN").ok())
                .or_else(|| {
                    let home = std::env::var("HOME").ok()?;
                    std::fs::read_to_string(std::path::Path::new(&home).join(".clawbox/token"))
                        .ok()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                });
            let token = match token {
                Some(t) => t,
                None => {
                    eprintln!(
                        "No auth token found. Pass --token, set CLAWBOX_AUTH_TOKEN, or run `clawbox init` to create ~/.clawbox/token"
                    );
                    std::process::exit(1);
                }
            };
            tokio::task::block_in_place(|| mcp::run(&host, port, &token))?;
        }
        Commands::Creds {
            config: config_path,
            action,
        } => {
            let config = clawbox_server::ClawboxConfig::load(&config_path)?;
            let store_path = if config.credentials.store_path.starts_with("~/") {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
                config.credentials.store_path.replacen("~", &home, 1)
            } else {
                config.credentials.store_path.clone()
            };

            match action {
                CredsAction::Add {
                    name,
                    domain,
                    header,
                    prefix,
                } => {
                    eprint!("Enter credential value for '{name}': ");
                    let mut value = String::new();
                    std::io::stdin().read_line(&mut value)?;
                    let value = value.trim();
                    if value.is_empty() {
                        anyhow::bail!("credential value cannot be empty");
                    }

                    let key = load_master_key()?;
                    let mut store = clawbox_proxy::CredentialStore::load(&store_path, key)?;
                    store.add(&name, value, &domain, &header, &prefix);
                    store.save()?;
                    eprintln!("Credential '{name}' stored for domain '{domain}'");
                }
                CredsAction::Remove { name } => {
                    let key = load_master_key()?;
                    let mut store = clawbox_proxy::CredentialStore::load(&store_path, key)?;
                    store.remove(&name);
                    store.save()?;
                    eprintln!("Credential '{name}' removed");
                }
                CredsAction::List => {
                    let key = load_master_key()?;
                    let store = clawbox_proxy::CredentialStore::load(&store_path, key)?;
                    let names = store.list_names();
                    if names.is_empty() {
                        println!("No credentials stored.");
                    } else {
                        for name in names {
                            println!("  {name}");
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

async fn shutdown_signal(state: Arc<clawbox_server::AppState>) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received, cleaning up...");
    state.shutdown().await;
    info!("Cleanup complete");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn test_format_log_line_valid_json() {
        let line = r#"{"timestamp":"2025-01-01T00:00:00Z","method":"GET","url":"/health","status":200,"duration_ms":5}"#;
        let result = format_log_line(line);
        assert_eq!(result, "[2025-01-01T00:00:00Z] GET /health → 200 (5ms)");
    }

    #[test]
    fn test_format_log_line_invalid_json() {
        let line = "not json at all";
        assert_eq!(format_log_line(line), "not json at all");
    }

    #[test]
    fn test_format_log_line_partial_fields() {
        let line = r#"{"timestamp":"2025-01-01T00:00:00Z","method":"POST"}"#;
        let result = format_log_line(line);
        assert_eq!(result, "[2025-01-01T00:00:00Z] POST ? → ? (?)");
    }

    #[test]
    fn test_resolve_auth_token_from_flag() {
        let token = resolve_auth_token(&Some("my-token".into())).unwrap();
        assert_eq!(token, "my-token");
    }

    #[test]
    #[serial]
    fn test_resolve_auth_token_from_env() {
        unsafe { std::env::set_var("CLAWBOX_AUTH_TOKEN", "env-token") };
        let token = resolve_auth_token(&None).unwrap();
        assert_eq!(token, "env-token");
        unsafe { std::env::remove_var("CLAWBOX_AUTH_TOKEN") };
    }

    #[test]
    #[serial]
    fn test_resolve_auth_token_flag_over_env() {
        unsafe { std::env::set_var("CLAWBOX_AUTH_TOKEN", "env-token") };
        let token = resolve_auth_token(&Some("flag-token".into())).unwrap();
        assert_eq!(token, "flag-token");
        unsafe { std::env::remove_var("CLAWBOX_AUTH_TOKEN") };
    }

    #[test]
    #[serial]
    fn test_resolve_auth_token_missing() {
        unsafe { std::env::remove_var("CLAWBOX_AUTH_TOKEN") };
        let result = resolve_auth_token(&None);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_json_params() {
        let input = r#"{"key": "value"}"#;
        let v: Value = serde_json::from_str(input).unwrap();
        assert_eq!(v["key"], "value");
    }
}
