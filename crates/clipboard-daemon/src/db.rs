//! Store operations that compose `clipboard_core::schema` helpers with crypto.
//!
//! The daemon is the sole SQLite writer (SPEC §3). Plain CRUD lives in
//! `clipboard_core::schema`; this module adds the daemon-only concerns:
//! encryption/decryption, FTS index maintenance tied to capture/delete, and
//! mapping rows into the IPC summary/detail shapes.

use anyhow::Result;
use clipboard_core::crypto::{self, Key};
use clipboard_core::ipc::{ClipDetail, ClipSummary};
use clipboard_core::schema::{self as cs, ClipRow, FtsFields};
use rusqlite::{params, Connection, OptionalExtension};

/// A newly captured clip, ready to persist. `text` is the full plaintext when
/// it fits under the size cap; otherwise it is `None` and only preview/metadata
/// are stored (SPEC §4 caps).
pub struct CapturedClip {
    pub content_hash: String,
    pub mime: String,
    pub category: String,
    pub text: Option<String>,
    pub preview: String,
    pub source_app: Option<String>,
    pub source_bundle_id: Option<String>,
    pub source_window_title: Option<String>,
    pub source_url: Option<String>,
    pub byte_size: i64,
}

/// A newly captured image clip. Thumbnail bytes are encrypted at rest; full
/// raster is not stored in the DB (hash + dims + encrypted thumb only).
pub struct CapturedImage {
    pub content_hash: String,
    pub mime: String,
    pub preview: String,
    pub thumb_bytes: Vec<u8>,
    pub thumb_mime: String,
    pub width: i64,
    pub height: i64,
    pub source_app: Option<String>,
    pub source_bundle_id: Option<String>,
    pub source_window_title: Option<String>,
    pub source_url: Option<String>,
    pub byte_size: i64,
}

fn summary_from_row(row: &ClipRow) -> ClipSummary {
    ClipSummary {
        id: row.id.clone(),
        preview: row.preview_plaintext.clone().unwrap_or_default(),
        category: row.category.clone(),
        mime: row.mime.clone(),
        source_app: row.source_app.clone(),
        source_window_title: row.source_window_title.clone(),
        source_url: row.source_url.clone(),
        byte_size: row.byte_size,
        created_at: row.created_at.clone(),
        last_seen_at: row.last_seen_at.clone(),
        is_favorite: row.is_favorite,
    }
}

/// Decrypt a clip's stored text, if any.
fn decrypt_text(key: &Key, row: &ClipRow) -> Result<Option<String>> {
    match (&row.text_ciphertext, &row.text_nonce) {
        (Some(ct), Some(nonce)) => {
            let plain = crypto::decrypt(key, nonce, ct)
                .map_err(|e| anyhow::anyhow!("decrypt clip {}: {e}", row.id))?;
            Ok(Some(String::from_utf8_lossy(&plain).into_owned()))
        }
        _ => Ok(None),
    }
}

fn detail_from_row(key: &Key, row: &ClipRow) -> Result<ClipDetail> {
    Ok(ClipDetail {
        id: row.id.clone(),
        category: row.category.clone(),
        mime: row.mime.clone(),
        text: decrypt_text(key, row)?,
        source_app: row.source_app.clone(),
        source_bundle_id: row.source_bundle_id.clone(),
        source_window_title: row.source_window_title.clone(),
        source_url: row.source_url.clone(),
        byte_size: row.byte_size,
        created_at: row.created_at.clone(),
        last_seen_at: row.last_seen_at.clone(),
        updated_at: row.updated_at.clone(),
        is_favorite: row.is_favorite,
    })
}

/// The plaintext used for the FTS index: the full body when stored, else the
/// preview. Must match between insert and delete so the contentless index can
/// be reversed.
fn searchable_text(key: &Key, row: &ClipRow) -> Result<Option<String>> {
    match decrypt_text(key, row)? {
        Some(t) => Ok(Some(t)),
        None => Ok(row.preview_plaintext.clone()),
    }
}

pub fn count(conn: &Connection) -> Result<i64> {
    Ok(cs::count_clips(conn)?)
}

pub fn recent(conn: &Connection, limit: u32) -> Result<Vec<ClipSummary>> {
    Ok(cs::recent_clips(conn, limit)?
        .iter()
        .map(summary_from_row)
        .collect())
}

/// FTS5-backed search with optional category/app/time filters. Falls back to
/// recent items when the query has no usable terms.
pub fn search(
    conn: &Connection,
    query: &str,
    limit: u32,
    category: Option<&str>,
    app: Option<&str>,
    since: Option<&str>,
) -> Result<Vec<ClipSummary>> {
    let has_filters = category.is_some() || app.is_some() || since.is_some();
    let match_query = build_fts_query(query);

    let rows: Vec<ClipRow> = if match_query.is_empty() {
        // Over-fetch when filtering so post-filtering can still fill `limit`.
        let fetch = if has_filters { 1000 } else { limit };
        cs::recent_clips(conn, fetch)?
    } else {
        let fetch = if has_filters { 1000 } else { limit };
        let ids = cs::fts_search(conn, &match_query, fetch)?;
        let mut rows = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(row) = cs::get_clip(conn, &id)? {
                rows.push(row);
            }
        }
        rows
    };

    let filtered = rows
        .into_iter()
        .filter(|r| category.is_none_or(|c| r.category == c))
        .filter(|r| app.is_none_or(|a| r.source_app.as_deref() == Some(a)))
        .filter(|r| since.is_none_or(|s| r.last_seen_at.as_str() >= s))
        .take(limit as usize)
        .map(|r| summary_from_row(&r))
        .collect();
    Ok(filtered)
}

pub fn get_detail(conn: &Connection, key: &Key, id: &str) -> Result<Option<ClipDetail>> {
    match cs::get_clip(conn, id)? {
        Some(row) => Ok(Some(detail_from_row(key, &row)?)),
        None => Ok(None),
    }
}

/// Delete a clip, first reversing its FTS index entry.
pub fn delete(conn: &Connection, key: &Key, id: &str) -> Result<bool> {
    let Some(row) = cs::get_clip(conn, id)? else {
        return Ok(false);
    };
    let searchable = searchable_text(key, &row)?;
    cs::fts_remove(
        conn,
        id,
        FtsFields {
            text: searchable.as_deref(),
            source_app: row.source_app.as_deref(),
            source_window_title: row.source_window_title.as_deref(),
            source_url: row.source_url.as_deref(),
        },
    )?;
    Ok(cs::delete_clip(conn, id)?)
}

/// Insert a captured clip (encrypting the body) or dedup by content hash.
/// Returns true if a new row was stored.
pub fn store_capture(conn: &Connection, key: &Key, clip: CapturedClip) -> Result<bool> {
    let is_new = cs::get_clip_by_hash(conn, &clip.content_hash)?.is_none();

    let mut row = ClipRow::new(clip.content_hash, clip.mime, clip.category, clip.byte_size);
    row.preview_plaintext = Some(clip.preview);
    row.source_app = clip.source_app;
    row.source_bundle_id = clip.source_bundle_id;
    row.source_window_title = clip.source_window_title;
    row.source_url = clip.source_url;

    if let Some(text) = &clip.text {
        let (nonce, ciphertext) = crypto::encrypt(key, text.as_bytes())
            .map_err(|e| anyhow::anyhow!("encrypt clip: {e}"))?;
        row.text_ciphertext = Some(ciphertext);
        row.text_nonce = Some(nonce);
    }

    let id = cs::insert_or_touch(conn, &row)?;

    if is_new {
        let searchable = clip.text.as_deref().or(row.preview_plaintext.as_deref());
        cs::fts_add(
            conn,
            &id,
            FtsFields {
                text: searchable,
                source_app: row.source_app.as_deref(),
                source_window_title: row.source_window_title.as_deref(),
                source_url: row.source_url.as_deref(),
            },
        )?;
    }
    Ok(is_new)
}

/// Insert an image clip (encrypting the thumbnail) or dedup by content hash.
/// Returns true if a new row was stored.
pub fn store_image(conn: &Connection, key: &Key, image: CapturedImage) -> Result<bool> {
    let is_new = cs::get_clip_by_hash(conn, &image.content_hash)?.is_none();

    let mut row = ClipRow::new(
        image.content_hash,
        image.mime,
        "image".to_string(),
        image.byte_size,
    );
    row.preview_plaintext = Some(image.preview);
    row.source_app = image.source_app;
    row.source_bundle_id = image.source_bundle_id;
    row.source_window_title = image.source_window_title;
    row.source_url = image.source_url;
    row.width = Some(image.width);
    row.height = Some(image.height);
    row.thumb_mime = Some(image.thumb_mime);

    let (nonce, ciphertext) = crypto::encrypt(key, &image.thumb_bytes)
        .map_err(|e| anyhow::anyhow!("encrypt image thumb: {e}"))?;
    row.thumb_ciphertext = Some(ciphertext);
    row.thumb_nonce = Some(nonce);

    let id = cs::insert_or_touch(conn, &row)?;

    if is_new {
        // Index the preview string so "[image WxH]" is searchable.
        cs::fts_add(
            conn,
            &id,
            FtsFields {
                text: row.preview_plaintext.as_deref(),
                source_app: row.source_app.as_deref(),
                source_window_title: row.source_window_title.as_deref(),
                source_url: row.source_url.as_deref(),
            },
        )?;
    }
    Ok(is_new)
}

/// True if a source app matches any exclusion rule (SPEC §5.2 denylist).
pub fn is_app_excluded(
    conn: &Connection,
    app: Option<&str>,
    bundle_id: Option<&str>,
) -> Result<bool> {
    if app.is_none() && bundle_id.is_none() {
        return Ok(false);
    }
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM apps_excluded WHERE \
                 (match_type = 'app_name' AND match_value = ?1) OR \
                 (match_type = 'bundle_id' AND match_value = ?2) LIMIT 1",
            params![app, bundle_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

/// Turn free-text into a safe FTS5 MATCH expression: each whitespace token is
/// wrapped as a quoted string (quotes doubled) and AND-ed together. This avoids
/// FTS5 syntax errors from user input containing operators.
fn build_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}
