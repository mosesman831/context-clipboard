//! IPC client for `context-clipboardd` using `clipboard_core` wire types.

use std::{
    env,
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

use clipboard_core::ipc::{
    ClipDetail, ClipSummary, Request, Response, IPC_VERSION, MAX_FRAME_BYTES,
};
use clipboard_core::paths;
use serde::Serialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

// Re-export wire types for the binary and downstream callers.
pub use clipboard_core::ipc::{
    ClipDetail as Detail, ClipSummary as Summary, Request as IpcRequest, Response as IpcResponse,
};

pub const PROTOCOL_VERSION: u32 = IPC_VERSION;
pub const DEFAULT_RECENT_LIMIT: u32 = 5;

const SOCKET_ENV: &str = "CONTEXT_CLIPBOARD_SOCKET";

#[derive(Debug)]
pub enum ClientError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Protocol(String),
    Paths(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "{error}"),
            Self::Protocol(message) | Self::Paths(message) => write!(f, "{message}"),
        }
    }
}

impl Error for ClientError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Protocol(_) | Self::Paths(_) => None,
        }
    }
}

impl From<std::io::Error> for ClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub type Result<T> = std::result::Result<T, ClientError>;

#[derive(Debug, Clone)]
pub struct IpcClient {
    socket_path: PathBuf,
}

impl IpcClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub fn for_default_socket() -> Result<Self> {
        Ok(Self::new(default_socket_path()?))
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub async fn send(&self, request: &Request) -> Result<Response> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .map_err(|error| connect_error(error, &self.socket_path))?;

        write_frame(&mut stream, request).await?;
        read_frame(&mut stream).await
    }
}

/// Resolve the daemon socket the same way the daemon does (with env override).
pub fn default_socket_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os(SOCKET_ENV) {
        return Ok(PathBuf::from(path));
    }
    paths::socket_path().map_err(|e| ClientError::Paths(e.to_string()))
}

pub async fn write_frame(stream: &mut UnixStream, value: &impl Serialize) -> Result<()> {
    let payload = serde_json::to_vec(value)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(ClientError::Protocol(format!(
            "frame is {} bytes, max is {MAX_FRAME_BYTES}",
            payload.len()
        )));
    }
    stream
        .write_all(&(payload.len() as u32).to_be_bytes())
        .await?;
    stream.write_all(&payload).await?;
    stream.flush().await?;
    Ok(())
}

pub async fn read_frame(stream: &mut UnixStream) -> Result<Response> {
    let mut len = [0_u8; 4];
    stream.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(ClientError::Protocol(format!(
            "frame is {len} bytes, max is {MAX_FRAME_BYTES}"
        )));
    }
    let mut payload = vec![0_u8; len];
    stream.read_exact(&mut payload).await?;
    Ok(serde_json::from_slice(&payload)?)
}

fn connect_error(error: std::io::Error, socket_path: &Path) -> ClientError {
    ClientError::Protocol(format!(
        "could not connect to daemon socket at {}: {error}",
        socket_path.display()
    ))
}

/// Helpers for building common requests.
pub fn request_ping() -> Request {
    Request::Ping {
        v: PROTOCOL_VERSION,
    }
}

pub fn request_search(
    query: impl Into<String>,
    limit: u32,
    category: Option<String>,
    app: Option<String>,
    since: Option<String>,
) -> Request {
    Request::Search {
        query: query.into(),
        limit: Some(limit),
        category,
        app,
        since,
    }
}

pub fn request_recent(limit: u32) -> Request {
    Request::Recent { limit: Some(limit) }
}

pub fn source_label(item: &ClipSummary) -> Option<&str> {
    item.source_app
        .as_deref()
        .or(item.source_window_title.as_deref())
}

pub fn clip_text(detail: &ClipDetail) -> Option<&str> {
    detail.text.as_deref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_search_request_for_daemon() {
        let request = request_search(
            "meeting notes",
            10,
            Some("text".to_owned()),
            Some("Slack".to_owned()),
            Some("2026-07-01T00:00:00Z".to_owned()),
        );

        let encoded = serde_json::to_value(&request).expect("search request encodes");

        assert_eq!(encoded["op"], "search");
        assert_eq!(encoded["query"], "meeting notes");
        assert_eq!(encoded["limit"], 10);
        assert_eq!(encoded["category"], "text");
        assert_eq!(encoded["app"], "Slack");
        assert_eq!(encoded["since"], "2026-07-01T00:00:00Z");
    }

    #[test]
    fn encodes_ping_with_snake_case_op() {
        let encoded = serde_json::to_value(request_ping()).expect("ping encodes");
        assert_eq!(encoded["op"], "ping");
        assert_eq!(encoded["v"], PROTOCOL_VERSION);
    }
}
