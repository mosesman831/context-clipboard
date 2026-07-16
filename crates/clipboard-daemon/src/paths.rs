//! Path resolution for the daemon.
//!
//! By default paths come from `clipboard_core::paths`. When `--data-dir` is
//! given, everything (db, config, key, socket, lock, logs) is placed under that
//! directory so the daemon is fully self-contained — useful for tests and for
//! running isolated instances. Core's path functions have no override hook, so
//! this wrapper owns that behavior.

use anyhow::{Context, Result};
use clipboard_core::paths as core_paths;
use std::path::{Path, PathBuf};

/// Key filename under the data dir (dev key; production uses the OS keychain).
pub const KEY_NAME: &str = "db.key";

pub struct Paths {
    override_dir: Option<PathBuf>,
}

impl Paths {
    pub fn new(override_dir: Option<PathBuf>) -> Self {
        Self { override_dir }
    }

    pub fn data_dir(&self) -> Result<PathBuf> {
        match &self.override_dir {
            Some(d) => Ok(d.clone()),
            None => core_paths::data_dir().context("resolving data dir"),
        }
    }

    fn runtime_dir(&self) -> Result<PathBuf> {
        match &self.override_dir {
            Some(d) => Ok(d.clone()),
            None => core_paths::runtime_dir().context("resolving runtime dir"),
        }
    }

    fn log_dir(&self) -> Result<PathBuf> {
        match &self.override_dir {
            Some(d) => Ok(d.join("logs")),
            None => core_paths::log_dir().context("resolving log dir"),
        }
    }

    pub fn db_path(&self) -> Result<PathBuf> {
        match &self.override_dir {
            Some(d) => Ok(d.join(core_paths::DB_NAME)),
            None => core_paths::db_path().context("resolving db path"),
        }
    }

    pub fn config_path(&self) -> Result<PathBuf> {
        match &self.override_dir {
            Some(d) => Ok(d.join(core_paths::CONFIG_NAME)),
            None => core_paths::config_path().context("resolving config path"),
        }
    }

    pub fn socket_path(&self) -> Result<PathBuf> {
        match &self.override_dir {
            Some(d) => Ok(d.join(core_paths::SOCKET_NAME)),
            None => core_paths::socket_path().context("resolving socket path"),
        }
    }

    pub fn lock_path(&self) -> Result<PathBuf> {
        match &self.override_dir {
            Some(d) => Ok(d.join(core_paths::LOCK_NAME)),
            None => core_paths::lock_path().context("resolving lock path"),
        }
    }

    pub fn key_path(&self) -> Result<PathBuf> {
        Ok(self.data_dir()?.join(KEY_NAME))
    }

    /// Create the data, runtime, and log directories.
    pub fn ensure_dirs(&self) -> Result<()> {
        create(&self.data_dir()?)?;
        create(&self.runtime_dir()?)?;
        create(&self.log_dir()?)?;
        Ok(())
    }
}

fn create(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))
}
