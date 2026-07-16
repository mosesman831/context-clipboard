//! Filesystem paths for data, config, socket, and lockfile.
//!
//! Data dir:
//! - macOS: `~/Library/Application Support/ContextClipboard/`
//! - Linux/other: `$XDG_DATA_HOME/context-clipboard/` (default
//!   `~/.local/share/context-clipboard/`)
//!
//! Runtime dir (socket + lockfile): prefers the platform runtime dir
//! (`$XDG_RUNTIME_DIR` on Linux, `$TMPDIR` on macOS) and falls back to a
//! `run/` subdirectory of the data dir.

use std::path::PathBuf;

use directories::BaseDirs;

use crate::error::{Error, Result};

/// Unix domain socket filename.
pub const SOCKET_NAME: &str = "context-clipboardd.sock";
/// Single-instance lockfile name.
pub const LOCK_NAME: &str = "context-clipboardd.lock";
/// SQLite database filename.
pub const DB_NAME: &str = "history.db";
/// Config filename.
pub const CONFIG_NAME: &str = "config.toml";

const APP_DIR_MACOS: &str = "ContextClipboard";
const APP_DIR_XDG: &str = "context-clipboard";

fn home_dir() -> Result<PathBuf> {
    if let Some(bd) = BaseDirs::new() {
        return Ok(bd.home_dir().to_path_buf());
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Config("could not determine home directory".to_string()))
}

/// User data directory (created lazily via [`ensure_dirs`]).
pub fn data_dir() -> Result<PathBuf> {
    if cfg!(target_os = "macos") {
        Ok(home_dir()?
            .join("Library")
            .join("Application Support")
            .join(APP_DIR_MACOS))
    } else {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| {
                // Fallback handled below; use a placeholder that we join with home.
                PathBuf::new()
            });
        if base.as_os_str().is_empty() {
            Ok(home_dir()?.join(".local").join("share").join(APP_DIR_XDG))
        } else {
            Ok(base.join(APP_DIR_XDG))
        }
    }
}

/// Runtime directory for the socket and lockfile.
pub fn runtime_dir() -> Result<PathBuf> {
    // Linux: XDG_RUNTIME_DIR is the correct, per-user, secured location.
    if let Some(bd) = BaseDirs::new() {
        if let Some(rt) = bd.runtime_dir() {
            return Ok(rt.join(APP_DIR_XDG));
        }
    }

    // macOS: prefer the per-user temp dir exposed via $TMPDIR.
    if cfg!(target_os = "macos") {
        if let Some(tmp) = std::env::var_os("TMPDIR").map(PathBuf::from) {
            if !tmp.as_os_str().is_empty() {
                return Ok(tmp.join(APP_DIR_MACOS));
            }
        }
    }

    // Fallback: a run/ dir under the data directory.
    Ok(data_dir()?.join("run"))
}

/// Directory for rotating log files.
pub fn log_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("logs"))
}

/// Path to the SQLite database.
pub fn db_path() -> Result<PathBuf> {
    Ok(data_dir()?.join(DB_NAME))
}

/// Path to the TOML config file.
pub fn config_path() -> Result<PathBuf> {
    Ok(data_dir()?.join(CONFIG_NAME))
}

/// Path to the Unix domain socket.
pub fn socket_path() -> Result<PathBuf> {
    Ok(runtime_dir()?.join(SOCKET_NAME))
}

/// Path to the single-instance lockfile.
pub fn lock_path() -> Result<PathBuf> {
    Ok(runtime_dir()?.join(LOCK_NAME))
}

/// Create the data, runtime, and log directories if they do not exist.
pub fn ensure_dirs() -> Result<()> {
    std::fs::create_dir_all(data_dir()?)?;
    std::fs::create_dir_all(runtime_dir()?)?;
    std::fs::create_dir_all(log_dir()?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_absolute_and_named() {
        // These may depend on environment; assert structure not exact prefix.
        let db = db_path().expect("db path");
        assert!(db.ends_with(DB_NAME));

        let cfg = config_path().expect("config path");
        assert!(cfg.ends_with(CONFIG_NAME));

        let sock = socket_path().expect("socket path");
        assert!(sock.ends_with(SOCKET_NAME));

        let lock = lock_path().expect("lock path");
        assert!(lock.ends_with(LOCK_NAME));
    }

    #[test]
    fn data_dir_uses_app_folder() {
        let d = data_dir().expect("data dir");
        let s = d.to_string_lossy();
        assert!(
            s.contains(APP_DIR_MACOS) || s.contains(APP_DIR_XDG),
            "unexpected data dir: {s}"
        );
    }
}
