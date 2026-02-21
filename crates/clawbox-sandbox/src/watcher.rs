//! Filesystem watcher for hot-reloading WASM tool modules.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;
use tracing::{debug, error, info, warn};

use crate::SandboxEngine;

/// Errors from the tool watcher.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WatcherError {
    /// Filesystem notification error.
    #[error("notify error: {0}")]
    Notify(#[from] notify::Error),
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Handle to a running tool directory watcher. Call `shutdown()` to stop.
#[non_exhaustive]
pub struct ToolWatcherHandle {
    shutdown: Arc<AtomicBool>,
    _watcher: RecommendedWatcher,
}

impl ToolWatcherHandle {
    /// Signal the watcher to stop processing events.
    pub fn shutdown(self) {
        self.shutdown.store(true, Ordering::Relaxed);
        info!("tool watcher shut down");
    }
}

/// Debounce state per file path.
struct DebounceState {
    last_event: HashMap<PathBuf, Instant>,
}

impl DebounceState {
    fn new() -> Self {
        Self {
            last_event: HashMap::new(),
        }
    }

    /// Returns true if the event should be processed (not debounced).
    fn should_process(&mut self, path: &PathBuf) -> bool {
        let now = Instant::now();
        if let Some(last) = self.last_event.get(path)
            && now.duration_since(*last).as_millis() < 500
        {
            return false;
        }
        self.last_event.insert(path.clone(), now);
        true
    }
}

/// Start watching a tool directory for .wasm file changes.
///
/// On create/modify: reloads the module.
/// On remove: unloads the module.
pub fn start_watching(
    engine: Arc<SandboxEngine>,
    tool_dir: PathBuf,
) -> Result<ToolWatcherHandle, WatcherError> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);
    let debounce = Arc::new(Mutex::new(DebounceState::new()));

    let watcher_engine = Arc::clone(&engine);
    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if shutdown_clone.load(Ordering::Relaxed) {
            return;
        }

        let event = match res {
            Ok(e) => e,
            Err(e) => {
                warn!("filesystem watch error: {e}");
                return;
            }
        };

        for path in &event.paths {
            // Only care about .wasm files
            if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
                continue;
            }

            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };

            let mut db = debounce.lock().unwrap_or_else(|e| e.into_inner());
            if !db.should_process(&path.to_path_buf()) {
                debug!(name = stem, "debounced wasm file event");
                continue;
            }
            drop(db);

            let name = stem.to_string();

            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    info!(name, path = %path.display(), "wasm file changed, reloading");
                    if let Err(e) = watcher_engine.reload_module(&name, path) {
                        error!(name, error = %e, "failed to reload module");
                    }
                }
                EventKind::Remove(_) => {
                    info!(name, "wasm file removed, unloading");
                    watcher_engine.unload_module(&name);
                }
                _ => {
                    debug!(name, kind = ?event.kind, "ignoring wasm file event");
                }
            }
        }
    })?;

    watcher.watch(&tool_dir, RecursiveMode::NonRecursive)?;
    info!(dir = %tool_dir.display(), "started watching tool directory");

    Ok(ToolWatcherHandle {
        shutdown,
        _watcher: watcher,
    })
}
