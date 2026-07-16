//! Length-prefixed JSON framing over an async byte stream (SPEC §5, §9).
//!
//! Wire format: a 4-byte big-endian `u32` length prefix followed by that many
//! bytes of UTF-8 JSON. This mirrors the sync `read_frame`/`write_frame`
//! helpers in `clipboard-core` for sync clients, adapted to Tokio streams.

use anyhow::{bail, Result};
use clipboard_core::ipc::MAX_FRAME_BYTES;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Read one frame. Returns `None` on a clean EOF at a frame boundary.
pub async fn read_frame<R>(reader: &mut R) -> Result<Option<Vec<u8>>>
where
    R: AsyncReadExt + Unpin,
{
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        bail!("frame too large: {len} bytes (max {MAX_FRAME_BYTES})");
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    Ok(Some(body))
}

/// Write one frame (length prefix + body).
pub async fn write_frame<W>(writer: &mut W, body: &[u8]) -> Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    if body.len() > MAX_FRAME_BYTES {
        bail!("frame too large to send: {} bytes", body.len());
    }
    let len = (body.len() as u32).to_be_bytes();
    writer.write_all(&len).await?;
    writer.write_all(body).await?;
    writer.flush().await?;
    Ok(())
}
