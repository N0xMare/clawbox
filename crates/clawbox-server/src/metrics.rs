//! Prometheus metrics for clawbox.

use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

/// Initialize the metrics recorder and return a handle for rendering.
///
/// If a global recorder has already been installed (e.g. in tests),
/// this creates a standalone recorder without installing it globally.
pub fn init_metrics() -> PrometheusHandle {
    let recorder = PrometheusBuilder::new().build_recorder();
    let handle = recorder.handle();
    // Try to install globally; if it fails (already installed), that's fine —
    // we still return a valid handle for rendering.
    let _ = metrics::set_global_recorder(recorder);
    handle
}

/// Record a WASM tool execution.
pub fn record_wasm_execution(tool: &str, status: &str, duration_ms: f64, fuel: u64) {
    counter!("clawbox_wasm_executions_total", "tool" => tool.to_owned(), "status" => status.to_owned())
        .increment(1);
    histogram!("clawbox_wasm_execution_ms", "tool" => tool.to_owned()).record(duration_ms);
    histogram!("clawbox_wasm_fuel_consumed", "tool" => tool.to_owned()).record(fuel as f64);
}

/// Set the active container count gauge.
pub fn set_containers_active(count: usize) {
    gauge!("clawbox_containers_active").set(count as f64);
}

/// Record a container lifecycle event.
pub fn record_container_event(status: &str) {
    counter!("clawbox_containers_total", "status" => status.to_owned()).increment(1);
}

/// Record a sanitization issue detected.
pub fn record_sanitization_issue() {
    counter!("clawbox_sanitization_issues_total").increment(1);
}

/// Set the number of loaded tools.
pub fn set_tools_loaded(count: usize) {
    gauge!("clawbox_tools_loaded").set(count as f64);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_recording_functions_do_not_panic() {
        // These should not panic even without a global recorder
        // (metrics crate uses a no-op recorder by default)
        record_wasm_execution("echo", "ok", 10.0, 5000);
        set_containers_active(3);
        record_container_event("spawned");
        record_sanitization_issue();
        set_tools_loaded(5);
    }
}
