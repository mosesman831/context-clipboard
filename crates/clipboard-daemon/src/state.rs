//! Process-wide shared state handed to the IPC server and capture loop.

use clipboard_core::config::Config;
use clipboard_core::crypto::Key;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};

/// Shared application state. `Arc<AppState>` is cloned into every connection
/// handler and the capture thread.
pub struct AppState {
    /// The daemon is the only SQLite writer (SPEC §3). A single connection
    /// behind a mutex keeps writes serialized and readers consistent.
    pub db: Mutex<Connection>,
    pub key: Key,
    pub config: RwLock<Config>,
    paused: AtomicBool,
    /// Kept for the forthcoming "reveal data dir" / settings IPC surface.
    #[allow(dead_code)]
    pub data_dir: PathBuf,
}

impl AppState {
    pub fn new(db: Connection, key: Key, config: Config, data_dir: PathBuf) -> Self {
        let paused = AtomicBool::new(config.capture.paused);
        Self {
            db: Mutex::new(db),
            key,
            config: RwLock::new(config),
            paused,
            data_dir,
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
        if let Ok(mut cfg) = self.config.write() {
            cfg.capture.paused = paused;
        }
    }

    pub fn poll_interval_ms(&self) -> u64 {
        self.config
            .read()
            .map(|c| c.capture.poll_interval_ms)
            .unwrap_or(300)
    }

    pub fn max_text_bytes(&self) -> usize {
        self.config
            .read()
            .map(|c| c.capture.max_text_bytes as usize)
            .unwrap_or(1_048_576)
    }

    pub fn recent_count(&self) -> u32 {
        self.config.read().map(|c| c.ui.recent_count).unwrap_or(5)
    }

    pub fn secret_heuristics_enabled(&self) -> bool {
        self.config
            .read()
            .map(|c| c.privacy.secret_heuristics)
            .unwrap_or(true)
    }
}
