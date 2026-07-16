//! Versioned, length-prefixed JSON IPC protocol (SPEC §9).
//!
//! Wire format: a 4-byte big-endian unsigned length prefix followed by that
//! many bytes of UTF-8 JSON. [`write_frame`]/[`read_frame`] implement this over
//! any [`Write`]/[`Read`]. The protocol is additive: new request/response
//! variants can be introduced without breaking v1 clients.

use std::io::{Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Protocol version. Sent in [`Request::Ping`] and reported in
/// [`Response::Status`].
pub const IPC_VERSION: u32 = 1;

/// Maximum accepted frame size (64 MiB) to bound memory on malformed input.
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// Client → server requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Ping {
        v: u32,
    },
    Search {
        query: String,
        #[serde(default)]
        limit: Option<u32>,
        #[serde(default)]
        category: Option<String>,
        #[serde(default)]
        app: Option<String>,
        #[serde(default)]
        since: Option<String>,
    },
    Recent {
        #[serde(default)]
        limit: Option<u32>,
    },
    Get {
        id: String,
    },
    Delete {
        id: String,
    },
    Clear,
    SetPaused {
        paused: bool,
    },
    GetStatus,
    UpdateSettings {
        settings: serde_json::Value,
    },
    ListExcludedApps,
    AddExcludedApp {
        match_type: String,
        match_value: String,
    },
    RemoveExcludedApp {
        id: i64,
    },
}

/// Server → client responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Err {
        code: String,
        message: String,
    },
    Status {
        paused: bool,
        count: u64,
        version: u32,
        platform_caps: Vec<String>,
    },
    SearchResults {
        items: Vec<ClipSummary>,
    },
    ClipDetail {
        detail: Box<ClipDetail>,
    },
    ExcludedApps {
        apps: Vec<ExcludedAppInfo>,
    },
    Settings {
        settings: serde_json::Value,
    },
}

impl Response {
    /// Convenience constructor for error responses.
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Response::Err {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// A list-view summary of a clip. Never carries ciphertext or full secrets;
/// `preview` is the short, exclusion-respecting preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipSummary {
    pub id: String,
    pub preview: String,
    pub category: String,
    pub mime: String,
    pub source_app: Option<String>,
    pub source_window_title: Option<String>,
    pub source_url: Option<String>,
    pub byte_size: i64,
    pub created_at: String,
    pub last_seen_at: String,
    pub is_favorite: bool,
}

/// Full detail for a single clip, including decrypted text for paste/reveal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipDetail {
    pub id: String,
    pub category: String,
    pub mime: String,
    /// Decrypted text, present only when the clip stored text.
    pub text: Option<String>,
    pub source_app: Option<String>,
    pub source_bundle_id: Option<String>,
    pub source_window_title: Option<String>,
    pub source_url: Option<String>,
    pub byte_size: i64,
    pub created_at: String,
    pub last_seen_at: String,
    pub updated_at: String,
    pub is_favorite: bool,
}

/// An excluded-app rule as exposed over IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExcludedAppInfo {
    pub id: i64,
    pub match_type: String,
    pub match_value: String,
}

/// Write a length-prefixed JSON frame.
pub fn write_frame<W: Write, T: Serialize>(w: &mut W, msg: &T) -> Result<()> {
    let body = serde_json::to_vec(msg)?;
    let len = u32::try_from(body.len())
        .map_err(|_| Error::Ipc("frame body exceeds u32 length".to_string()))?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(Error::Ipc(format!(
            "frame body {} exceeds max {}",
            body.len(),
            MAX_FRAME_BYTES
        )));
    }
    w.write_all(&len.to_be_bytes())?;
    w.write_all(&body)?;
    w.flush()?;
    Ok(())
}

/// Read a length-prefixed JSON frame.
pub fn read_frame<R: Read, T: DeserializeOwned>(r: &mut R) -> Result<T> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(Error::Ipc(format!(
            "declared frame length {len} exceeds max {MAX_FRAME_BYTES}"
        )));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn ping_roundtrip() {
        let req = Request::Ping { v: IPC_VERSION };
        let mut buf = Vec::new();
        write_frame(&mut buf, &req).expect("write");

        // Verify the 4-byte big-endian length prefix.
        let declared = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        assert_eq!(declared, buf.len() - 4);

        let mut cursor = Cursor::new(buf);
        let decoded: Request = read_frame(&mut cursor).expect("read");
        assert_eq!(decoded, req);
    }

    #[test]
    fn status_roundtrip() {
        let resp = Response::Status {
            paused: true,
            count: 42,
            version: IPC_VERSION,
            platform_caps: vec!["hotkey".into(), "window_title".into()],
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &resp).expect("write");
        let mut cursor = Cursor::new(buf);
        let decoded: Response = read_frame(&mut cursor).expect("read");
        assert_eq!(decoded, resp);
    }

    #[test]
    fn multiple_frames_stream() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &Request::GetStatus).expect("w1");
        write_frame(&mut buf, &Request::Recent { limit: Some(5) }).expect("w2");
        let mut cursor = Cursor::new(buf);
        let a: Request = read_frame(&mut cursor).expect("r1");
        let b: Request = read_frame(&mut cursor).expect("r2");
        assert_eq!(a, Request::GetStatus);
        assert_eq!(b, Request::Recent { limit: Some(5) });
    }

    #[test]
    fn oversized_declared_length_rejected() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&u32::MAX.to_be_bytes());
        let mut cursor = Cursor::new(buf);
        let r: Result<Request> = read_frame(&mut cursor);
        assert!(r.is_err());
    }

    #[test]
    fn error_response_shape() {
        let resp = Response::error("not_found", "no such clip");
        let mut buf = Vec::new();
        write_frame(&mut buf, &resp).expect("write");
        let mut cursor = Cursor::new(buf);
        let decoded: Response = read_frame(&mut cursor).expect("read");
        assert_eq!(decoded, resp);
    }
}
