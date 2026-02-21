//! Resource limit enforcement for WASM execution.

use serde::{Deserialize, Serialize};

/// Fuel budget for WASM execution (maps roughly to instruction count).
pub const DEFAULT_FUEL: u64 = 100_000_000; // ~100M instructions

/// Epoch interval for interruption checks (milliseconds).
pub const EPOCH_INTERVAL_MS: u64 = 100;

/// Default execution timeout in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000; // 30 seconds

/// Default epoch deadline (number of epoch ticks before interruption).
pub const DEFAULT_EPOCH_DEADLINE: u64 = 300; // 300 * 100ms = 30s

/// Default maximum WASM linear memory (bytes).
pub const DEFAULT_MAX_MEMORY_BYTES: usize = 64 * 1024 * 1024; // 64MB

/// Default maximum number of WASM table elements.
pub const DEFAULT_MAX_TABLE_ELEMENTS: usize = 10_000;

/// Configuration for sandbox resource limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[must_use]
pub struct SandboxConfig {
    /// Directory containing pre-compiled WASM tool modules.
    pub tool_dir: std::path::PathBuf,
    /// Fuel budget per execution.
    #[serde(default = "default_fuel")]
    pub fuel_limit: u64,
    /// Epoch deadline (number of ticks before timeout).
    #[serde(default = "default_epoch_deadline")]
    pub epoch_deadline: u64,
    /// Epoch tick interval in milliseconds.
    #[serde(default = "default_epoch_interval")]
    pub epoch_interval_ms: u64,
    /// Maximum host_call invocations per execution.
    #[serde(default = "default_max_host_calls")]
    pub max_host_calls: u32,
    /// Maximum WASM linear memory in bytes.
    #[serde(default = "default_max_memory")]
    pub max_memory_bytes: usize,
    /// Maximum WASM table elements.
    #[serde(default = "default_max_table_elements")]
    pub max_table_elements: usize,
}

fn default_max_host_calls() -> u32 {
    100
}
fn default_max_memory() -> usize {
    DEFAULT_MAX_MEMORY_BYTES
}
fn default_max_table_elements() -> usize {
    DEFAULT_MAX_TABLE_ELEMENTS
}

fn default_fuel() -> u64 {
    DEFAULT_FUEL
}
fn default_epoch_deadline() -> u64 {
    DEFAULT_EPOCH_DEADLINE
}
fn default_epoch_interval() -> u64 {
    EPOCH_INTERVAL_MS
}

impl SandboxConfig {
    pub fn new(tool_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            tool_dir: tool_dir.into(),
            fuel_limit: DEFAULT_FUEL,
            epoch_deadline: DEFAULT_EPOCH_DEADLINE,
            epoch_interval_ms: EPOCH_INTERVAL_MS,
            max_host_calls: 100,
            max_memory_bytes: DEFAULT_MAX_MEMORY_BYTES,
            max_table_elements: DEFAULT_MAX_TABLE_ELEMENTS,
        }
    }
}

impl SandboxConfig {
    /// Set the fuel limit.
    pub fn with_fuel_limit(mut self, fuel: u64) -> Self {
        self.fuel_limit = fuel;
        self
    }

    /// Set the epoch deadline.
    pub fn with_epoch_deadline(mut self, deadline: u64) -> Self {
        self.epoch_deadline = deadline;
        self
    }

    /// Set the epoch interval in milliseconds.
    pub fn with_epoch_interval_ms(mut self, ms: u64) -> Self {
        self.epoch_interval_ms = ms;
        self
    }

    /// Set the maximum host calls per execution.
    pub fn with_max_host_calls(mut self, max: u32) -> Self {
        self.max_host_calls = max;
        self
    }

    /// Set the maximum WASM memory in bytes.
    pub fn with_max_memory_bytes(mut self, bytes: usize) -> Self {
        self.max_memory_bytes = bytes;
        self
    }

    /// Set the maximum WASM table elements.
    pub fn with_max_table_elements(mut self, elements: usize) -> Self {
        self.max_table_elements = elements;
        self
    }
}
