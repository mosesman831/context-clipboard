use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    env,
    error::Error,
    fmt,
    path::{Path, PathBuf},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::UnixStream,
};

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_RECENT_LIMIT: u32 = 5;

const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const SOCKET_ENV: &str = "CONTEXT_CLIPBOARD_SOCKET";

#[derive(Debug)]
pub enum ClientError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Protocol(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Json(error) => write!(f, "{error}"),
            Self::Protocol(message) => write!(f, "{message}"),
        }
    }
}

impl Error for ClientError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Protocol(_) => None,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum Request {
    Ping {
        v: u32,
    },
    Search {
        query: String,
        limit: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        category: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        app: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        since: Option<String>,
    },
    Recent {
        limit: u32,
    },
    Get {
        id: String,
    },
    Delete {
        id: String,
    },
    Clear {},
    SetPaused {
        paused: bool,
    },
    GetStatus {},
}

impl Request {
    pub fn ping() -> Self {
        Self::Ping {
            v: PROTOCOL_VERSION,
        }
    }

    pub fn search(
        query: impl Into<String>,
        limit: u32,
        category: Option<String>,
        app: Option<String>,
        since: Option<String>,
    ) -> Self {
        Self::Search {
            query: query.into(),
            limit,
            category,
            app,
            since,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum Response {
    Ok {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    Err {
        code: String,
        message: String,
    },
    Status {
        #[serde(default)]
        paused: bool,
        #[serde(default)]
        count: u64,
        #[serde(default)]
        version: String,
        #[serde(default)]
        platform_caps: BTreeMap<String, Value>,
    },
    SearchResults {
        #[serde(default)]
        items: Vec<ClipSummary>,
    },
    ClipDetail {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preview: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        category: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_app: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_window_title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        created_at: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_seen_at: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipSummary {
    pub id: String,
    #[serde(default)]
    pub preview: String,
    #[serde(default)]
    pub category: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_window_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl ClipSummary {
    pub fn source_label(&self) -> Option<&str> {
        self.source_app
            .as_deref()
            .or(self.app.as_deref())
            .or(self.source_window_title.as_deref())
    }
}

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

    pub fn for_default_socket() -> Self {
        Self::new(default_socket_path())
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

pub fn default_socket_path() -> PathBuf {
    if let Some(path) = env::var_os(SOCKET_ENV) {
        return PathBuf::from(path);
    }

    if let Some(runtime_dir) = env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir)
            .join("context-clipboard")
            .join("context-clipboard.sock");
    }

    let user = env::var("UID").unwrap_or_else(|_| "user".to_owned());
    env::temp_dir().join(format!("context-clipboard-{user}.sock"))
}

pub async fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let payload = serde_json::to_vec(value)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(ClientError::Protocol(format!(
            "frame is {} bytes, max is {MAX_FRAME_BYTES}",
            payload.len()
        )));
    }

    writer
        .write_all(&(payload.len() as u32).to_be_bytes())
        .await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_frame<R, T>(reader: &mut R) -> Result<T>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut len = [0_u8; 4];
    reader.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len) as usize;

    if len > MAX_FRAME_BYTES {
        return Err(ClientError::Protocol(format!(
            "frame is {len} bytes, max is {MAX_FRAME_BYTES}"
        )));
    }

    let mut payload = vec![0_u8; len];
    reader.read_exact(&mut payload).await?;
    Ok(serde_json::from_slice(&payload)?)
}

fn connect_error(error: std::io::Error, socket_path: &Path) -> ClientError {
    ClientError::Protocol(format!(
        "could not connect to daemon socket at {}: {error}",
        socket_path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_search_request_for_daemon() {
        let request = Request::search(
            "meeting notes",
            10,
            Some("text".to_owned()),
            Some("Slack".to_owned()),
            Some("2026-07-01T00:00:00Z".to_owned()),
        );

        let encoded = serde_json::to_value(request).expect("search request encodes");

        assert_eq!(encoded["type"], "Search");
        assert_eq!(encoded["query"], "meeting notes");
        assert_eq!(encoded["limit"], 10);
        assert_eq!(encoded["category"], "text");
        assert_eq!(encoded["app"], "Slack");
        assert_eq!(encoded["since"], "2026-07-01T00:00:00Z");
    }

    #[tokio::test]
    async fn round_trips_length_prefixed_request() {
        let (mut writer, mut reader) = tokio::io::duplex(1024);
        let request = Request::Recent {
            limit: DEFAULT_RECENT_LIMIT,
        };
        let expected = request.clone();

        let write_task = tokio::spawn(async move { write_frame(&mut writer, &request).await });
        let decoded: Request = read_frame(&mut reader).await.expect("frame decodes");

        assert_eq!(decoded, expected);
        write_task
            .await
            .expect("writer task completes")
            .expect("write ok");
    }
}
