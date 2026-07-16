//! Tokio Unix-domain-socket IPC server.
//!
//! Binds the socket from the resolved `socket_path`, restricts it to
//! `0600`, verifies each peer's UID matches our own (SPEC §5), then reads
//! length-prefixed JSON `Request` frames and writes `Response` frames.

use crate::frame::{read_frame, write_frame};
use crate::handlers;
use crate::state::AppState;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, info, warn};

pub struct Server {
    listener: UnixListener,
    socket_path: PathBuf,
}

impl Server {
    /// Bind the socket, replacing any stale socket file. Sets `0600` perms.
    pub fn bind(socket_path: &Path) -> Result<Self> {
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating socket dir {}", parent.display()))?;
        }
        // A leftover socket file from a previous run would make bind fail; the
        // single-instance lock already guarantees no live daemon is using it.
        if socket_path.exists() {
            std::fs::remove_file(socket_path)
                .with_context(|| format!("removing stale socket {}", socket_path.display()))?;
        }

        let listener = UnixListener::bind(socket_path)
            .with_context(|| format!("binding socket {}", socket_path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))
                .context("chmod 0600 on socket")?;
        }

        info!(socket = %socket_path.display(), "IPC server listening");
        Ok(Self {
            listener,
            socket_path: socket_path.to_path_buf(),
        })
    }

    /// Accept loop. Runs until `shutdown` resolves, then removes the socket.
    pub async fn run(self, state: Arc<AppState>, shutdown: impl std::future::Future<Output = ()>) {
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                accepted = self.listener.accept() => match accepted {
                    Ok((stream, _addr)) => {
                        let state = Arc::clone(&state);
                        tokio::spawn(async move {
                            if let Err(e) = handle_conn(stream, state).await {
                                debug!(error = %e, "connection closed with error");
                            }
                        });
                    }
                    Err(e) => {
                        error!(error = %e, "accept failed");
                    }
                },
                _ = &mut shutdown => {
                    info!("shutdown signal received; stopping IPC server");
                    break;
                }
            }
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

async fn handle_conn(mut stream: UnixStream, state: Arc<AppState>) -> Result<()> {
    if !peer_is_self(&stream) {
        warn!("rejecting connection from a different uid");
        return Ok(());
    }

    let (read_half, write_half) = stream.split();
    let mut reader = tokio::io::BufReader::new(read_half);
    let mut writer = write_half;

    while let Some(body) = read_frame(&mut reader).await? {
        let response = match serde_json::from_slice::<clipboard_core::ipc::Request>(&body) {
            Ok(req) => handlers::dispatch(&state, req),
            Err(e) => clipboard_core::ipc::Response::error(
                "bad_request",
                format!("malformed request: {e}"),
            ),
        };
        let bytes = serde_json::to_vec(&response)?;
        write_frame(&mut writer, &bytes).await?;
    }
    Ok(())
}

/// Verify the connecting peer's UID equals ours via `SO_PEERCRED`.
#[cfg(target_os = "linux")]
fn peer_is_self(stream: &UnixStream) -> bool {
    use std::os::unix::io::AsRawFd;
    let fd = stream.as_raw_fd();
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: fd is a valid connected socket; cred/len are correctly sized.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        warn!("SO_PEERCRED failed; rejecting peer");
        return false;
    }
    // SAFETY: geteuid is always safe.
    let self_uid = unsafe { libc::geteuid() };
    cred.uid == self_uid
}

/// Non-Linux Unix fallback: the socket already lives in the user runtime dir
/// with `0600` perms, so same-user access is enforced by the filesystem.
#[cfg(all(unix, not(target_os = "linux")))]
fn peer_is_self(_stream: &UnixStream) -> bool {
    true
}
