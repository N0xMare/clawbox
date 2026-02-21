//! Core WASM execution engine.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use thiserror::Error;
use tracing::{info, warn};
use wasmtime::{Caller, Config, Engine, Linker, Module, ResourceLimiter, Store};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};

use crate::host_functions::{HostCallHandler, HostState, LogEntry, NoOpHandler};
use crate::resource_limits::SandboxConfig;

/// Errors from the sandbox engine.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SandboxError {
    #[error("tool \'{0}\' not found")]
    ToolNotFound(String),
    #[error("module load error: {0}")]
    ModuleLoad(String),
    #[error("execution timed out")]
    Timeout,
    #[error("fuel exhausted")]
    FuelExhausted,
    #[error("engine initialization error: {0}")]
    Init(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Output from a tool execution.
#[derive(Debug, serde::Serialize)]
#[must_use]
#[non_exhaustive]
pub struct ToolOutput {
    /// Parsed JSON output from the tool's stdout.
    pub output: serde_json::Value,
    /// Log entries captured during execution.
    pub logs: Vec<LogEntry>,
    /// Wall-clock execution time in milliseconds.
    pub execution_time_ms: u64,
    /// Fuel consumed during execution.
    pub fuel_consumed: u64,
}

/// Enforces memory and table growth limits on WASM modules.
struct WasmResourceLimiter {
    max_memory: usize,
    max_table_elements: usize,
}

impl ResourceLimiter for WasmResourceLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> anyhow::Result<bool> {
        let max = maximum
            .map(|m| m.min(self.max_memory))
            .unwrap_or(self.max_memory);
        Ok(desired <= max)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> anyhow::Result<bool> {
        let max = maximum
            .map(|m| m.min(self.max_table_elements))
            .unwrap_or(self.max_table_elements);
        Ok(desired <= max)
    }
}

/// Store state that holds both WASI context and our host state.
struct SandboxState {
    wasi: WasiP1Ctx,
    host: HostState,
    limiter: WasmResourceLimiter,
}

/// A cached WASM module with its file modification time.
#[derive(Clone)]
struct CachedModule {
    module: Module,
    modified: Option<SystemTime>,
}

/// The WASM sandbox engine — manages wasmtime instances for tool execution.
#[non_exhaustive]
pub struct SandboxEngine {
    engine: Engine,
    config: SandboxConfig,
    modules: Mutex<HashMap<String, CachedModule>>,
    shutdown: Arc<AtomicBool>,
    /// Handle for the epoch ticker thread, joined on shutdown.
    epoch_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl SandboxEngine {
    /// Create a new sandbox engine with the given configuration.
    pub fn new(config: SandboxConfig) -> std::result::Result<Self, SandboxError> {
        let mut wasm_config = Config::new();
        wasm_config.consume_fuel(true);
        wasm_config.epoch_interruption(true);
        wasm_config.wasm_bulk_memory(true);
        wasm_config.wasm_multi_value(true);
        wasm_config.wasm_simd(true);
        wasm_config.wasm_reference_types(true);
        wasm_config.wasm_threads(false);

        let engine = Engine::new(&wasm_config).map_err(|e| SandboxError::Init(format!("{e:#}")))?;

        // Spawn epoch ticker thread for timeout enforcement
        let engine_clone = engine.clone();
        let interval_ms = config.epoch_interval_ms;
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = Arc::clone(&shutdown);
        let epoch_handle = std::thread::Builder::new()
            .name("clawbox-epoch-ticker".to_string())
            .spawn(move || {
                while !shutdown_clone.load(AtomicOrdering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(interval_ms));
                    engine_clone.increment_epoch();
                }
            })?;

        info!("WASM sandbox engine initialized");

        Ok(Self {
            engine,
            config,
            modules: Mutex::new(HashMap::new()),
            shutdown,
            epoch_thread: Mutex::new(Some(epoch_handle)),
        })
    }

    /// Check if a module with the given name is loaded.
    pub fn has_module(&self, name: &str) -> bool {
        self.modules
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(name)
    }

    /// Remove a loaded module by name. Returns true if it existed.
    pub fn unload_module(&self, name: &str) -> bool {
        self.modules
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name)
            .is_some()
    }

    /// List names of all loaded modules.
    pub fn list_modules(&self) -> Vec<String> {
        self.modules
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    /// Signal the epoch ticker thread to stop and wait for it to finish.
    pub fn shutdown(&self) {
        self.shutdown.store(true, AtomicOrdering::Relaxed);
        if let Some(handle) = self
            .epoch_thread
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = handle.join();
        }
    }

    /// Get a reference to the wasmtime engine.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Load and cache a WASM module by name from a file path.
    pub fn load_module(&self, name: &str, path: &Path) -> std::result::Result<(), SandboxError> {
        let module = Module::from_file(&self.engine, path).map_err(|e| {
            SandboxError::ModuleLoad(format!(
                "failed to load module '{name}' from {}: {e:#}",
                path.display()
            ))
        })?;
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let mut modules = self.modules.lock().unwrap_or_else(|e| e.into_inner());
        modules.insert(name.to_string(), CachedModule { module, modified });
        info!(name, "loaded WASM tool module");
        Ok(())
    }

    /// Recompile and replace a single cached module.
    pub fn reload_module(&self, name: &str, path: &Path) -> std::result::Result<(), SandboxError> {
        let module = Module::from_file(&self.engine, path).map_err(|e| {
            SandboxError::ModuleLoad(format!(
                "failed to reload module '{name}' from {}: {e:#}",
                path.display()
            ))
        })?;
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let mut modules = self.modules.lock().unwrap_or_else(|e| e.into_inner());
        modules.insert(name.to_string(), CachedModule { module, modified });
        info!(name, "reloaded WASM tool module");
        Ok(())
    }

    /// Rescan tool_dir and reload all modules.
    pub fn reload_all_modules(&self) -> std::result::Result<usize, SandboxError> {
        self.load_all_modules()
    }

    /// Load all .wasm files from the configured tool directory.
    /// Skips modules whose file modification time hasn't changed since last load.
    pub fn load_all_modules(&self) -> std::result::Result<usize, SandboxError> {
        let dir = &self.config.tool_dir;
        if !dir.exists() {
            warn!(?dir, "tool directory does not exist");
            return Ok(0);
        }
        let mut count = 0;
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("wasm")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                // Check if mtime changed
                let current_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                let needs_reload = {
                    let modules = self.modules.lock().unwrap_or_else(|e| e.into_inner());
                    match modules.get(stem) {
                        Some(cached) => cached.modified != current_mtime,
                        None => true,
                    }
                };
                if needs_reload {
                    self.load_module(stem, &path)?;
                }
                count += 1;
            }
        }
        info!(count, "loaded all WASM tool modules");
        Ok(count)
    }

    /// Execute a tool by name with JSON parameters.
    pub fn execute_tool(
        &self,
        tool_name: &str,
        params_json: &serde_json::Value,
        handler: Option<Arc<dyn HostCallHandler>>,
    ) -> std::result::Result<ToolOutput, SandboxError> {
        let start = Instant::now();

        // Look up the module
        let module = {
            let modules = self.modules.lock().unwrap_or_else(|e| e.into_inner());
            modules
                .get(tool_name)
                .map(|c| c.module.clone())
                .ok_or_else(|| SandboxError::ToolNotFound(tool_name.to_string()))?
        };

        let handler = handler.unwrap_or_else(|| Arc::new(NoOpHandler));
        let host_state = HostState::with_limit(handler, self.config.max_host_calls);
        let logs = Arc::clone(&host_state.logs);

        // Set up stdin with params JSON, stdout capture
        let stdin_bytes = serde_json::to_vec(params_json).unwrap_or_default();
        let stdin_pipe = MemoryInputPipe::new(stdin_bytes);
        // NOTE: MemoryOutputPipe::new(cap) sets initial capacity, NOT a hard cap.
        // wasmtime-wasi will grow the buffer beyond this if the guest writes more.
        // We truncate after execution below to enforce the 1MB limit.
        let stdout_pipe = MemoryOutputPipe::new(1024 * 1024);

        let stderr_pipe = MemoryOutputPipe::new(64 * 1024);
        let wasi = WasiCtxBuilder::new()
            .stdin(stdin_pipe)
            .stdout(stdout_pipe.clone())
            .stderr(stderr_pipe)
            .build_p1();

        let limiter = WasmResourceLimiter {
            max_memory: self.config.max_memory_bytes,
            max_table_elements: self.config.max_table_elements,
        };
        let state = SandboxState {
            wasi,
            host: host_state,
            limiter,
        };

        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| &mut state.limiter);
        store
            .set_fuel(self.config.fuel_limit)
            .map_err(|e| SandboxError::Runtime(format!("fuel setup: {e:#}")))?;
        store.set_epoch_deadline(self.config.epoch_deadline);

        // Create linker with WASI + host functions
        let mut linker: Linker<SandboxState> = Linker::new(&self.engine);
        p1::add_to_linker_sync(&mut linker, |state: &mut SandboxState| &mut state.wasi)
            .map_err(|e| SandboxError::Runtime(format!("linker setup: {e:#}")))?;

        // Register clawbox::host_call
        linker
            .func_wrap(
                "clawbox",
                "host_call",
                |mut caller: Caller<'_, SandboxState>,
                 request_ptr: i32,
                 request_len: i32,
                 response_ptr: i32,
                 response_cap: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let data = memory.data(&caller);
                    let req_start = match usize::try_from(request_ptr) {
                        Ok(v) => v,
                        Err(_) => return -1,
                    };
                    let req_len = match usize::try_from(request_len) {
                        Ok(v) => v,
                        Err(_) => return -1,
                    };
                    let req_end = match req_start.checked_add(req_len) {
                        Some(v) => v,
                        None => return -1,
                    };
                    if req_end > data.len() {
                        return -1;
                    }
                    let request_bytes = data[req_start..req_end].to_vec();

                    let request_str = match std::str::from_utf8(&request_bytes) {
                        Ok(s) => s.to_string(),
                        Err(_) => return -1,
                    };

                    let response_bytes = caller.data().host.dispatch(&request_str);

                    let resp_start = match usize::try_from(response_ptr) {
                        Ok(v) => v,
                        Err(_) => return -1,
                    };
                    let resp_cap = match usize::try_from(response_cap) {
                        Ok(v) => v,
                        Err(_) => return -1,
                    };
                    let resp_cap = resp_cap.min(i32::MAX as usize);
                    let write_len = response_bytes.len().min(resp_cap);

                    let data_mut = memory.data_mut(&mut caller);
                    let resp_end = match resp_start.checked_add(write_len) {
                        Some(v) => v,
                        None => return -1,
                    };
                    if resp_end > data_mut.len() {
                        return -1;
                    }
                    data_mut[resp_start..resp_start + write_len]
                        .copy_from_slice(&response_bytes[..write_len]);

                    i32::try_from(write_len).unwrap_or(i32::MAX)
                },
            )
            .map_err(|e| SandboxError::Runtime(format!("linker setup: {e:#}")))?;

        // Instantiate and run
        let instance = linker.instantiate(&mut store, &module).map_err(|e| {
            let msg = format!("{e:#}");
            if msg.contains("fuel") {
                SandboxError::FuelExhausted
            } else if msg.contains("epoch") {
                SandboxError::Timeout
            } else {
                SandboxError::ModuleLoad(format!("{e:#}"))
            }
        })?;

        let start_func = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .map_err(|e| SandboxError::ModuleLoad(format!("{e:#}")))?;

        if let Err(e) = start_func.call(&mut store, ()) {
            let msg = format!("{e:#}");
            if msg.contains("fuel") || msg.contains("all fuel consumed") {
                return Err(SandboxError::FuelExhausted);
            } else if msg.contains("epoch") || msg.contains("interrupt") {
                return Err(SandboxError::Timeout);
            }
            // WASI proc_exit(0) is normal
            let is_normal_exit = e
                .downcast_ref::<wasmtime_wasi::I32Exit>()
                .is_some_and(|exit| exit.0 == 0);
            if !is_normal_exit {
                return Err(SandboxError::ModuleLoad(format!("{e:#}")));
            }
        }

        let fuel_remaining = store.get_fuel().unwrap_or(0);
        let fuel_consumed = self.config.fuel_limit.saturating_sub(fuel_remaining);
        let elapsed = start.elapsed().as_millis() as u64;

        // Read stdout
        let stdout_bytes = stdout_pipe.contents();
        const MAX_STDOUT_BYTES: usize = 1024 * 1024;
        let stdout_bytes = if stdout_bytes.len() > MAX_STDOUT_BYTES {
            warn!(
                tool = tool_name,
                actual_bytes = stdout_bytes.len(),
                max_bytes = MAX_STDOUT_BYTES,
                "Tool stdout exceeded maximum size, truncating"
            );
            stdout_bytes[..MAX_STDOUT_BYTES].to_vec().into()
        } else {
            stdout_bytes
        };

        let stdout_str = String::from_utf8_lossy(&stdout_bytes);
        let output: serde_json::Value = serde_json::from_str(&stdout_str)
            .unwrap_or_else(|_| serde_json::Value::String(stdout_str.into_owned()));

        let captured_logs = logs.lock().unwrap_or_else(|e| e.into_inner()).clone();

        Ok(ToolOutput {
            output,
            logs: captured_logs,
            execution_time_ms: elapsed,
            fuel_consumed,
        })
    }
}

impl Drop for SandboxEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource_limits::SandboxConfig;

    #[test]
    fn test_engine_creation() {
        let config = SandboxConfig::new("/tmp/nonexistent-tools");
        let engine = SandboxEngine::new(config);
        assert!(engine.is_ok());
    }

    #[test]
    fn test_load_module_nonexistent() {
        let config = SandboxConfig::new("/tmp/nonexistent-tools");
        let engine = SandboxEngine::new(config).unwrap();
        let result = engine.load_module("nope", Path::new("/tmp/does-not-exist.wasm"));
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_tool_not_found() {
        let config = SandboxConfig::new("/tmp/nonexistent-tools");
        let engine = SandboxEngine::new(config).unwrap();
        let result = engine.execute_tool("missing", &serde_json::json!({}), None);
        assert!(matches!(result, Err(SandboxError::ToolNotFound(_))));
    }

    #[test]
    fn test_engine_epoch_ticker_runs() {
        let config = SandboxConfig::new("/tmp/nonexistent-tools");
        let engine = SandboxEngine::new(config).unwrap();
        // Sleep enough for at least one epoch tick (interval=100ms)
        std::thread::sleep(std::time::Duration::from_millis(250));
        // If epoch ticker is running, engine should still be usable (no panic)
        assert!(engine.load_module("x", Path::new("/tmp/no.wasm")).is_err());
    }

    #[test]
    fn test_load_all_modules_empty_dir() {
        let dir = std::env::temp_dir().join("clawbox-test-empty");
        let _ = std::fs::create_dir_all(&dir);
        let config = SandboxConfig::new(&dir);
        let engine = SandboxEngine::new(config).unwrap();
        let count = engine.load_all_modules().unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_fuel_exhaustion() {
        let mut config = SandboxConfig::new("/tmp/nonexistent-tools");
        config.fuel_limit = 1000;
        config.epoch_deadline = 1000; // high so fuel runs out first
        let sandbox = SandboxEngine::new(config).unwrap();

        let wat = r#"(module (func (export "_start") (loop (br 0))))"#;
        let module = Module::new(&sandbox.engine, wat).unwrap();
        {
            let mut modules = sandbox.modules.lock().unwrap();
            modules.insert(
                "fuel_test".to_string(),
                CachedModule {
                    module,
                    modified: None,
                },
            );
        }

        let result = sandbox.execute_tool("fuel_test", &serde_json::json!({}), None);
        assert!(
            matches!(result, Err(SandboxError::FuelExhausted)),
            "expected FuelExhausted, got: {result:?}"
        );
    }

    #[test]
    fn test_bad_host_call_pointers() {
        let config = SandboxConfig::new("/tmp/nonexistent-tools");
        let sandbox = SandboxEngine::new(config).unwrap();

        // Module with 1 page of memory, calls host_call with out-of-bounds pointers
        let wat = r#"
            (module
                (import "clawbox" "host_call" (func $host_call (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "_start")
                    ;; request_ptr=99999, request_len=100, response_ptr=0, response_cap=0
                    ;; 99999+100 > 65536 (1 page), so should return -1
                    (drop (call $host_call (i32.const 99999) (i32.const 100) (i32.const 0) (i32.const 0)))
                )
            )
        "#;
        let module = Module::new(&sandbox.engine, wat).unwrap();
        {
            let mut modules = sandbox.modules.lock().unwrap();
            modules.insert(
                "bad_ptr_test".to_string(),
                CachedModule {
                    module,
                    modified: None,
                },
            );
        }

        // Should succeed (the host_call returns -1, but the module exits normally)
        let result = sandbox.execute_tool("bad_ptr_test", &serde_json::json!({}), None);
        assert!(
            result.is_ok(),
            "bad pointers should not crash host: {result:?}"
        );
    }

    #[test]
    fn test_large_stdout() {
        let config = SandboxConfig::new("/tmp/nonexistent-tools");
        let sandbox = SandboxEngine::new(config).unwrap();

        // Module that writes ~2MB to stdout via fd_write
        // We write 64KB at a time, 32 times = 2MB
        let wat = r#"
            (module
                (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 2)
                (func (export "_start")
                    (local $i i32)
                    ;; Set up iov at offset 0: ptr=16, len=65536
                    (i32.store (i32.const 0) (i32.const 16))
                    (i32.store (i32.const 4) (i32.const 65536))
                    (local.set $i (i32.const 0))
                    (block $done
                        (loop $loop
                            ;; fd=1 (stdout), iovs=0, iovs_len=1, nwritten=12
                            (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 12)))
                            (local.set $i (i32.add (local.get $i) (i32.const 1)))
                            (br_if $done (i32.ge_u (local.get $i) (i32.const 32)))
                            (br $loop)
                        )
                    )
                )
            )
        "#;
        let module = Module::new(&sandbox.engine, wat).unwrap();
        {
            let mut modules = sandbox.modules.lock().unwrap();
            modules.insert(
                "large_stdout_test".to_string(),
                CachedModule {
                    module,
                    modified: None,
                },
            );
        }

        let result = sandbox.execute_tool("large_stdout_test", &serde_json::json!({}), None);
        // Should not OOM — either succeeds with truncated output or errors gracefully
        assert!(
            result.is_ok() || matches!(result, Err(SandboxError::FuelExhausted)),
            "large stdout should be handled gracefully: {result:?}"
        );
    }

    #[test]
    fn test_memory_limit_enforced() {
        let mut config = SandboxConfig::new("/tmp/nonexistent-tools");
        // Set a very small memory limit: 1 page = 64KB
        config.max_memory_bytes = 64 * 1024; // 64KB = 1 WASM page
        let sandbox = SandboxEngine::new(config).unwrap();

        // Module starts with 1 page and tries to grow by 10 more (needs 704KB > 64KB limit)
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "_start")
                    ;; Try to grow memory by 10 pages (640KB). Should fail and return -1.
                    (drop (memory.grow (i32.const 10)))
                )
            )
        "#;
        let module = Module::new(&sandbox.engine, wat).unwrap();
        {
            let mut modules = sandbox.modules.lock().unwrap();
            modules.insert(
                "mem_limit_test".to_string(),
                CachedModule {
                    module,
                    modified: None,
                },
            );
        }

        // The module should execute successfully — memory.grow returns -1 but doesn't trap
        let result = sandbox.execute_tool("mem_limit_test", &serde_json::json!({}), None);
        assert!(
            result.is_ok(),
            "memory limit should deny growth gracefully: {result:?}"
        );
    }

    #[test]
    fn test_memory_limit_allows_within_bounds() {
        let mut config = SandboxConfig::new("/tmp/nonexistent-tools");
        config.max_memory_bytes = 256 * 1024; // 256KB = 4 pages
        let sandbox = SandboxEngine::new(config).unwrap();

        // Module starts with 1 page, grows by 2 more (total 3 pages = 192KB, within 256KB)
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "_start")
                    (drop (memory.grow (i32.const 2)))
                )
            )
        "#;
        let module = Module::new(&sandbox.engine, wat).unwrap();
        {
            let mut modules = sandbox.modules.lock().unwrap();
            modules.insert(
                "mem_allow_test".to_string(),
                CachedModule {
                    module,
                    modified: None,
                },
            );
        }

        let result = sandbox.execute_tool("mem_allow_test", &serde_json::json!({}), None);
        assert!(
            result.is_ok(),
            "memory growth within limits should succeed: {result:?}"
        );
    }

    #[test]
    fn test_epoch_timeout() {
        let mut config = SandboxConfig::new("/tmp/nonexistent-tools");
        config.epoch_interval_ms = 50;
        config.epoch_deadline = 2; // 2 * 50ms = 100ms
        config.fuel_limit = u64::MAX / 2; // very high fuel so timeout hits first
        let sandbox = SandboxEngine::new(config).unwrap();

        let wat = r#"(module (func (export "_start") (loop (br 0))))"#;
        let module = Module::new(&sandbox.engine, wat).unwrap();
        {
            let mut modules = sandbox.modules.lock().unwrap();
            modules.insert(
                "timeout_test".to_string(),
                CachedModule {
                    module,
                    modified: None,
                },
            );
        }

        let start = Instant::now();
        let result = sandbox.execute_tool("timeout_test", &serde_json::json!({}), None);
        let elapsed = start.elapsed();

        assert!(
            matches!(result, Err(SandboxError::Timeout)),
            "expected Timeout, got: {result:?}"
        );
        assert!(
            elapsed.as_secs() < 5,
            "timeout should fire quickly, took {elapsed:?}"
        );
    }

    #[test]
    fn test_unload_module_missing() {
        let config = SandboxConfig::new("/tmp/nonexistent-tools");
        let engine = SandboxEngine::new(config).unwrap();
        assert!(!engine.unload_module("nonexistent"));
    }

    #[test]
    fn test_unload_module_exists() {
        let config = SandboxConfig::new("/tmp/nonexistent-tools");
        let engine = SandboxEngine::new(config).unwrap();
        let wat = r#"(module (func (export "_start")))"#;
        let module = Module::new(&engine.engine, wat).unwrap();
        {
            let mut modules = engine.modules.lock().unwrap();
            modules.insert(
                "to_unload".to_string(),
                CachedModule {
                    module,
                    modified: None,
                },
            );
        }
        assert!(engine.has_module("to_unload"));
        assert!(engine.unload_module("to_unload"));
        assert!(!engine.has_module("to_unload"));
    }

    #[test]
    fn test_list_modules_empty() {
        let config = SandboxConfig::new("/tmp/nonexistent-tools");
        let engine = SandboxEngine::new(config).unwrap();
        assert!(engine.list_modules().is_empty());
    }

    #[test]
    fn test_list_modules_after_load() {
        let config = SandboxConfig::new("/tmp/nonexistent-tools");
        let engine = SandboxEngine::new(config).unwrap();
        let wat = r#"(module (func (export "_start")))"#;
        let module_a = Module::new(&engine.engine, wat).unwrap();
        let module_b = Module::new(&engine.engine, wat).unwrap();
        {
            let mut modules = engine.modules.lock().unwrap();
            modules.insert(
                "alpha".to_string(),
                CachedModule {
                    module: module_a,
                    modified: None,
                },
            );
            modules.insert(
                "beta".to_string(),
                CachedModule {
                    module: module_b,
                    modified: None,
                },
            );
        }
        let mut names = engine.list_modules();
        names.sort();
        assert_eq!(names, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn test_shutdown_joins_epoch_thread() {
        let config = SandboxConfig::new("/tmp/nonexistent-tools");
        let engine = SandboxEngine::new(config).unwrap();
        engine.shutdown();
        let handle = engine.epoch_thread.lock().unwrap();
        assert!(
            handle.is_none(),
            "epoch thread handle should be taken after shutdown"
        );
    }

    #[test]
    fn test_double_shutdown_is_safe() {
        let config = SandboxConfig::new("/tmp/nonexistent-tools");
        let engine = SandboxEngine::new(config).unwrap();
        engine.shutdown();
        engine.shutdown(); // Should not panic
    }
}
