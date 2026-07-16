//! Single-instance lock using an advisory `flock` on a lock file.
//!
//! `flock` (rather than `O_EXCL` create) means a crashed daemon that left its
//! lock file behind does not permanently block the next start: the lock is
//! released automatically when the holding process dies.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct SingleInstance {
    path: PathBuf,
    #[cfg(unix)]
    _file: std::fs::File,
}

impl SingleInstance {
    /// Acquire the exclusive lock at `path`, or fail if another daemon holds it.
    #[cfg(unix)]
    pub fn acquire(path: &Path) -> Result<Self> {
        use std::io::Write;
        use std::os::unix::io::AsRawFd;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating lock dir {}", parent.display()))?;
        }

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("opening lock file {}", path.display()))?;

        // SAFETY: flock on a valid fd owned by `file`.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
                anyhow::bail!(
                    "another context-clipboardd instance is already running (lock held on {})",
                    path.display()
                );
            }
            return Err(err).context("flock on lock file");
        }

        // Record our pid for debugging; ignore write errors (non-critical).
        let _ = file.set_len(0);
        let _ = writeln!(file, "{}", std::process::id());
        let _ = file.flush();

        Ok(Self {
            path: path.to_path_buf(),
            _file: file,
        })
    }

    #[cfg(not(unix))]
    pub fn acquire(_path: &Path) -> Result<Self> {
        anyhow::bail!("context-clipboardd requires a Unix platform");
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // The advisory lock is released when the fd closes; remove the file too.
        let _ = std::fs::remove_file(&self.path);
    }
}
