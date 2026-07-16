//! SQLite schema, migrations, and row helpers (SPEC §4).
//!
//! The daemon is the only writer; these helpers centralize the SQL so both the
//! daemon and (read-only) UI agree on shapes. Timestamps are RFC3339 UTC
//! strings, which sort lexicographically and so are safe to compare as text.

use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// Current schema version applied by [`migrate`].
pub const SCHEMA_VERSION: i64 = 1;

/// v1 schema DDL (SPEC §4). The `PRAGMA` lines from the spec are applied by
/// [`open`] rather than embedded here, so this batch can run inside a
/// transaction.
const V1_SCHEMA: &str = r#"
CREATE TABLE clips (
  id TEXT PRIMARY KEY,
  content_hash TEXT NOT NULL,
  mime TEXT NOT NULL,
  category TEXT NOT NULL,
  text_ciphertext BLOB,
  text_nonce BLOB,
  preview_plaintext TEXT,
  source_app TEXT,
  source_bundle_id TEXT,
  source_window_title TEXT,
  source_url TEXT,
  byte_size INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  is_favorite INTEGER NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX idx_clips_content_hash ON clips(content_hash);
CREATE INDEX idx_clips_last_seen ON clips(last_seen_at DESC);
CREATE INDEX idx_clips_category ON clips(category);
CREATE INDEX idx_clips_source_app ON clips(source_app);

CREATE VIRTUAL TABLE clips_fts USING fts5(
  text,
  source_app,
  source_window_title,
  source_url,
  content='',
  tokenize='porter unicode61'
);

CREATE TABLE clips_fts_map (
  clip_id TEXT PRIMARY KEY REFERENCES clips(id) ON DELETE CASCADE,
  fts_rowid INTEGER NOT NULL
);

CREATE TABLE apps_excluded (
  id INTEGER PRIMARY KEY,
  match_type TEXT NOT NULL,
  match_value TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(match_type, match_value)
);

CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE clip_vec (
  clip_id TEXT PRIMARY KEY REFERENCES clips(id) ON DELETE CASCADE,
  dim INTEGER NOT NULL CHECK(dim = 384),
  embedding BLOB NOT NULL,
  model_id TEXT NOT NULL,
  embedded_at TEXT NOT NULL
);
"#;

/// Seed denylist: password managers whose clipboard output should never be
/// stored. Seeded as both bundle IDs (macOS) and app names (cross-platform).
const SEED_BUNDLE_IDS: &[&str] = &[
    "com.1password.1password",
    "com.agilebits.onepassword",
    "com.bitwarden.desktop",
    "com.lastpass.LastPass",
    "org.keepassxc.keepassxc",
    "com.apple.keychainaccess",
];

const SEED_APP_NAMES: &[&str] = &[
    "1Password",
    "Bitwarden",
    "LastPass",
    "KeePassXC",
    "Keychain Access",
];

/// Current UTC time as an RFC3339 string.
pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

/// Generate a new time-ordered clip id (UUID v7).
pub fn new_clip_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// SHA-256 hex digest of the given bytes.
pub fn content_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Open (or create) the database at `path` with WAL + foreign keys enabled,
/// then run migrations.
pub fn open(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

/// Open an in-memory database (mainly for tests) with the schema applied.
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    // execute_batch tolerates the result rows PRAGMA statements return.
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    Ok(())
}

/// Apply versioned migrations. Idempotent; safe to call on every open.
pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
         );",
    )?;

    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?;

    if current < 1 {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(V1_SCHEMA)?;
        seed_excluded_apps(&tx)?;
        tx.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (1, ?1)",
            params![now_rfc3339()],
        )?;
        tx.commit()?;
    }

    Ok(())
}

fn seed_excluded_apps(conn: &Connection) -> Result<()> {
    let now = now_rfc3339();
    for bundle in SEED_BUNDLE_IDS {
        conn.execute(
            "INSERT OR IGNORE INTO apps_excluded(match_type, match_value, created_at)
             VALUES ('bundle_id', ?1, ?2)",
            params![bundle, now],
        )?;
    }
    for name in SEED_APP_NAMES {
        conn.execute(
            "INSERT OR IGNORE INTO apps_excluded(match_type, match_value, created_at)
             VALUES ('app_name', ?1, ?2)",
            params![name, now],
        )?;
    }
    Ok(())
}

/// A row in the `clips` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipRow {
    pub id: String,
    pub content_hash: String,
    pub mime: String,
    pub category: String,
    pub text_ciphertext: Option<Vec<u8>>,
    pub text_nonce: Option<Vec<u8>>,
    pub preview_plaintext: Option<String>,
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

impl ClipRow {
    /// Build a new row with sensible defaults (fresh id + timestamps).
    pub fn new(content_hash: String, mime: String, category: String, byte_size: i64) -> Self {
        let now = now_rfc3339();
        Self {
            id: new_clip_id(),
            content_hash,
            mime,
            category,
            text_ciphertext: None,
            text_nonce: None,
            preview_plaintext: None,
            source_app: None,
            source_bundle_id: None,
            source_window_title: None,
            source_url: None,
            byte_size,
            created_at: now.clone(),
            last_seen_at: now.clone(),
            updated_at: now,
            is_favorite: false,
        }
    }
}

fn row_from_sqlite(r: &rusqlite::Row<'_>) -> rusqlite::Result<ClipRow> {
    Ok(ClipRow {
        id: r.get("id")?,
        content_hash: r.get("content_hash")?,
        mime: r.get("mime")?,
        category: r.get("category")?,
        text_ciphertext: r.get("text_ciphertext")?,
        text_nonce: r.get("text_nonce")?,
        preview_plaintext: r.get("preview_plaintext")?,
        source_app: r.get("source_app")?,
        source_bundle_id: r.get("source_bundle_id")?,
        source_window_title: r.get("source_window_title")?,
        source_url: r.get("source_url")?,
        byte_size: r.get("byte_size")?,
        created_at: r.get("created_at")?,
        last_seen_at: r.get("last_seen_at")?,
        updated_at: r.get("updated_at")?,
        is_favorite: r.get("is_favorite")?,
    })
}

const CLIP_COLUMNS: &str = "id, content_hash, mime, category, text_ciphertext, text_nonce, \
     preview_plaintext, source_app, source_bundle_id, source_window_title, source_url, \
     byte_size, created_at, last_seen_at, updated_at, is_favorite";

/// Insert a clip row.
pub fn insert_clip(conn: &Connection, row: &ClipRow) -> Result<()> {
    conn.execute(
        "INSERT INTO clips (
            id, content_hash, mime, category, text_ciphertext, text_nonce,
            preview_plaintext, source_app, source_bundle_id, source_window_title,
            source_url, byte_size, created_at, last_seen_at, updated_at, is_favorite
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
         )",
        params![
            row.id,
            row.content_hash,
            row.mime,
            row.category,
            row.text_ciphertext,
            row.text_nonce,
            row.preview_plaintext,
            row.source_app,
            row.source_bundle_id,
            row.source_window_title,
            row.source_url,
            row.byte_size,
            row.created_at,
            row.last_seen_at,
            row.updated_at,
            row.is_favorite,
        ],
    )?;
    Ok(())
}

/// Insert a clip, or if `content_hash` already exists, touch `last_seen_at`
/// and enrich source metadata (keeping existing values when the new ones are
/// `NULL`). Returns the id of the resulting row.
pub fn insert_or_touch(conn: &Connection, row: &ClipRow) -> Result<String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM clips WHERE content_hash = ?1",
            params![row.content_hash],
            |r| r.get(0),
        )
        .optional()?;

    match existing {
        Some(id) => {
            conn.execute(
                "UPDATE clips SET
                    last_seen_at = ?1,
                    updated_at = ?2,
                    source_app = COALESCE(?3, source_app),
                    source_bundle_id = COALESCE(?4, source_bundle_id),
                    source_window_title = COALESCE(?5, source_window_title),
                    source_url = COALESCE(?6, source_url)
                 WHERE id = ?7",
                params![
                    row.last_seen_at,
                    row.updated_at,
                    row.source_app,
                    row.source_bundle_id,
                    row.source_window_title,
                    row.source_url,
                    id,
                ],
            )?;
            Ok(id)
        }
        None => {
            insert_clip(conn, row)?;
            Ok(row.id.clone())
        }
    }
}

/// Fetch a clip by id.
pub fn get_clip(conn: &Connection, id: &str) -> Result<Option<ClipRow>> {
    let sql = format!("SELECT {CLIP_COLUMNS} FROM clips WHERE id = ?1");
    let row = conn
        .query_row(&sql, params![id], row_from_sqlite)
        .optional()?;
    Ok(row)
}

/// Fetch a clip by content hash.
pub fn get_clip_by_hash(conn: &Connection, hash: &str) -> Result<Option<ClipRow>> {
    let sql = format!("SELECT {CLIP_COLUMNS} FROM clips WHERE content_hash = ?1");
    let row = conn
        .query_row(&sql, params![hash], row_from_sqlite)
        .optional()?;
    Ok(row)
}

/// Most recently seen clips, newest first.
pub fn recent_clips(conn: &Connection, limit: u32) -> Result<Vec<ClipRow>> {
    let sql =
        format!("SELECT {CLIP_COLUMNS} FROM clips ORDER BY last_seen_at DESC, id DESC LIMIT ?1");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit], row_from_sqlite)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Total number of clips.
pub fn count_clips(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM clips", [], |r| r.get(0))?)
}

/// Delete a clip by id. Cascades to `clips_fts_map` and `clip_vec`.
///
/// Note: the contentless `clips_fts` index is not updated here (it has no
/// foreign key). Search still excludes the clip because results join through
/// `clips_fts_map`, whose row is removed by the cascade. Call [`fts_remove`]
/// beforehand when you want to reclaim the index entry.
pub fn delete_clip(conn: &Connection, id: &str) -> Result<bool> {
    let n = conn.execute("DELETE FROM clips WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

/// Delete all clips and their FTS mappings.
pub fn clear_clips(conn: &Connection) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let n = tx.execute("DELETE FROM clips", [])?;
    // Contentless FTS index cannot be truncated by DELETE; rebuild it empty.
    tx.execute_batch("INSERT INTO clips_fts(clips_fts) VALUES('delete-all');")?;
    tx.commit()?;
    Ok(n)
}

/// Toggle or set the favorite flag on a clip.
pub fn set_favorite(conn: &Connection, id: &str, favorite: bool) -> Result<bool> {
    let n = conn.execute(
        "UPDATE clips SET is_favorite = ?1, updated_at = ?2 WHERE id = ?3",
        params![favorite, now_rfc3339(), id],
    )?;
    Ok(n > 0)
}

/// Searchable text fields for a clip; borrowed to avoid copying plaintext.
#[derive(Debug, Default, Clone, Copy)]
pub struct FtsFields<'a> {
    pub text: Option<&'a str>,
    pub source_app: Option<&'a str>,
    pub source_window_title: Option<&'a str>,
    pub source_url: Option<&'a str>,
}

/// Add a clip's searchable fields to the FTS index and record its rowid.
pub fn fts_add(conn: &Connection, clip_id: &str, fields: FtsFields<'_>) -> Result<()> {
    conn.execute(
        "INSERT INTO clips_fts(text, source_app, source_window_title, source_url)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            fields.text,
            fields.source_app,
            fields.source_window_title,
            fields.source_url,
        ],
    )?;
    let rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT OR REPLACE INTO clips_fts_map(clip_id, fts_rowid) VALUES (?1, ?2)",
        params![clip_id, rowid],
    )?;
    Ok(())
}

/// Remove a clip's entry from the FTS index.
///
/// The contentless FTS5 table requires the original column values to delete a
/// row, so the same `fields` used with [`fts_add`] must be supplied.
pub fn fts_remove(conn: &Connection, clip_id: &str, fields: FtsFields<'_>) -> Result<()> {
    let rowid: Option<i64> = conn
        .query_row(
            "SELECT fts_rowid FROM clips_fts_map WHERE clip_id = ?1",
            params![clip_id],
            |r| r.get(0),
        )
        .optional()?;

    let Some(rowid) = rowid else {
        return Ok(());
    };

    conn.execute(
        "INSERT INTO clips_fts(clips_fts, rowid, text, source_app, source_window_title, source_url)
         VALUES ('delete', ?1, ?2, ?3, ?4, ?5)",
        params![
            rowid,
            fields.text,
            fields.source_app,
            fields.source_window_title,
            fields.source_url,
        ],
    )?;
    conn.execute(
        "DELETE FROM clips_fts_map WHERE clip_id = ?1",
        params![clip_id],
    )?;
    Ok(())
}

/// Full-text search returning matching clip ids ordered by relevance.
pub fn fts_search(conn: &Connection, query: &str, limit: u32) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT m.clip_id
         FROM clips_fts f
         JOIN clips_fts_map m ON m.fts_rowid = f.rowid
         WHERE clips_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![query, limit], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// An excluded-app rule row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedApp {
    pub id: i64,
    pub match_type: String,
    pub match_value: String,
    pub created_at: String,
}

/// List all excluded-app rules.
pub fn list_excluded_apps(conn: &Connection) -> Result<Vec<ExcludedApp>> {
    let mut stmt = conn
        .prepare("SELECT id, match_type, match_value, created_at FROM apps_excluded ORDER BY id")?;
    let rows = stmt.query_map([], |r| {
        Ok(ExcludedApp {
            id: r.get(0)?,
            match_type: r.get(1)?,
            match_value: r.get(2)?,
            created_at: r.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Add an excluded-app rule. `match_type` must be one of `bundle_id`,
/// `app_name`, or `path_prefix`. Returns the new row id.
pub fn add_excluded_app(conn: &Connection, match_type: &str, match_value: &str) -> Result<i64> {
    match match_type {
        "bundle_id" | "app_name" | "path_prefix" => {}
        other => return Err(Error::InvalidInput(format!("invalid match_type: {other}"))),
    }
    conn.execute(
        "INSERT OR IGNORE INTO apps_excluded(match_type, match_value, created_at)
         VALUES (?1, ?2, ?3)",
        params![match_type, match_value, now_rfc3339()],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM apps_excluded WHERE match_type = ?1 AND match_value = ?2",
        params![match_type, match_value],
        |r| r.get(0),
    )?;
    Ok(id)
}

/// Remove an excluded-app rule by id.
pub fn remove_excluded_app(conn: &Connection, id: i64) -> Result<bool> {
    let n = conn.execute("DELETE FROM apps_excluded WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

/// Read a settings value.
pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?)
}

/// Write a settings value.
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Retention eviction: delete non-favorite clips older than `max_age_days`,
/// then trim non-favorites beyond `max_items` (oldest by `last_seen_at`).
/// Favorites are always exempt. Returns the number of rows deleted.
pub fn evict(
    conn: &Connection,
    max_age_days: Option<i64>,
    max_items: Option<i64>,
) -> Result<usize> {
    let mut deleted = 0usize;

    if let Some(days) = max_age_days {
        let cutoff = (Utc::now() - Duration::days(days)).to_rfc3339();
        deleted += conn.execute(
            "DELETE FROM clips WHERE is_favorite = 0 AND last_seen_at < ?1",
            params![cutoff],
        )?;
    }

    if let Some(max) = max_items {
        // Keep the newest `max` non-favorites; delete the rest.
        deleted += conn.execute(
            "DELETE FROM clips WHERE id IN (
                SELECT id FROM clips
                WHERE is_favorite = 0
                ORDER BY last_seen_at DESC, id DESC
                LIMIT -1 OFFSET ?1
             )",
            params![max],
        )?;
    }

    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        open_in_memory().expect("open in-memory db")
    }

    #[test]
    fn migrate_is_idempotent_and_seeds() {
        let c = conn();
        migrate(&c).expect("second migrate");
        let version: i64 = c
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .expect("version");
        assert_eq!(version, SCHEMA_VERSION);

        let apps = list_excluded_apps(&c).expect("list");
        assert!(apps.iter().any(|a| a.match_value == "1Password"));
        assert!(apps
            .iter()
            .any(|a| a.match_value == "org.keepassxc.keepassxc"));
        // Re-running migrate must not duplicate seeds.
        let count = apps.len();
        migrate(&c).expect("migrate again");
        assert_eq!(list_excluded_apps(&c).expect("list").len(), count);
    }

    #[test]
    fn content_hash_is_sha256_hex() {
        let h = content_hash(b"hello");
        assert_eq!(
            h,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn insert_get_and_dedup() {
        let c = conn();
        let mut row = ClipRow::new(content_hash(b"data"), "text/plain".into(), "text".into(), 4);
        row.preview_plaintext = Some("data".into());
        row.source_app = Some("Terminal".into());
        let id = insert_or_touch(&c, &row).expect("insert");
        assert_eq!(id, row.id);
        assert_eq!(count_clips(&c).expect("count"), 1);

        let fetched = get_clip(&c, &id).expect("get").expect("some");
        assert_eq!(fetched.source_app.as_deref(), Some("Terminal"));

        // Same hash -> dedup, updates metadata, no new row.
        let mut dup = ClipRow::new(content_hash(b"data"), "text/plain".into(), "text".into(), 4);
        dup.last_seen_at = "2999-01-01T00:00:00+00:00".into();
        dup.source_window_title = Some("bash".into());
        let dup_id = insert_or_touch(&c, &dup).expect("touch");
        assert_eq!(dup_id, id);
        assert_eq!(count_clips(&c).expect("count"), 1);
        let after = get_clip(&c, &id).expect("get").expect("some");
        assert_eq!(after.last_seen_at, "2999-01-01T00:00:00+00:00");
        assert_eq!(after.source_window_title.as_deref(), Some("bash"));
        // Existing metadata preserved when new value is NULL.
        assert_eq!(after.source_app.as_deref(), Some("Terminal"));
    }

    #[test]
    fn fts_add_search_remove() {
        let c = conn();
        let row = ClipRow::new(
            content_hash(b"rustlang"),
            "text/plain".into(),
            "text".into(),
            8,
        );
        insert_clip(&c, &row).expect("insert");
        fts_add(
            &c,
            &row.id,
            FtsFields {
                text: Some("the rust programming language"),
                source_app: Some("Firefox"),
                source_window_title: Some("rust-lang.org"),
                source_url: None,
            },
        )
        .expect("fts add");

        let hits = fts_search(&c, "rust", 10).expect("search");
        assert_eq!(hits, vec![row.id.clone()]);

        // Search across the app column too.
        let hits_app = fts_search(&c, "Firefox", 10).expect("search app");
        assert_eq!(hits_app, vec![row.id.clone()]);

        fts_remove(
            &c,
            &row.id,
            FtsFields {
                text: Some("the rust programming language"),
                source_app: Some("Firefox"),
                source_window_title: Some("rust-lang.org"),
                source_url: None,
            },
        )
        .expect("fts remove");
        assert!(fts_search(&c, "rust", 10).expect("search").is_empty());
    }

    #[test]
    fn delete_cascades_fts_map() {
        let c = conn();
        let row = ClipRow::new(content_hash(b"x"), "text/plain".into(), "text".into(), 1);
        insert_clip(&c, &row).expect("insert");
        fts_add(
            &c,
            &row.id,
            FtsFields {
                text: Some("deletable content"),
                ..Default::default()
            },
        )
        .expect("fts add");
        assert!(delete_clip(&c, &row.id).expect("delete"));
        // Map row cascaded away, so search returns nothing even though the
        // contentless index still holds an orphaned entry.
        assert!(fts_search(&c, "deletable", 10).expect("search").is_empty());
    }

    #[test]
    fn favorite_and_settings() {
        let c = conn();
        let row = ClipRow::new(content_hash(b"fav"), "text/plain".into(), "text".into(), 3);
        insert_clip(&c, &row).expect("insert");
        assert!(set_favorite(&c, &row.id, true).expect("fav"));
        assert!(
            get_clip(&c, &row.id)
                .expect("get")
                .expect("some")
                .is_favorite
        );

        set_setting(&c, "semantic.enabled", "false").expect("set");
        assert_eq!(
            get_setting(&c, "semantic.enabled").expect("get").as_deref(),
            Some("false")
        );
    }

    #[test]
    fn excluded_apps_add_remove() {
        let c = conn();
        let id = add_excluded_app(&c, "app_name", "Slack").expect("add");
        assert!(list_excluded_apps(&c)
            .expect("list")
            .iter()
            .any(|a| a.match_value == "Slack"));
        assert!(remove_excluded_app(&c, id).expect("remove"));
        assert!(add_excluded_app(&c, "bogus", "x").is_err());
    }

    #[test]
    fn eviction_by_count_and_age_exempts_favorites() {
        let c = conn();
        // Insert clips with increasing last_seen_at.
        for i in 0..5 {
            let mut row = ClipRow::new(
                content_hash(format!("c{i}").as_bytes()),
                "text/plain".into(),
                "text".into(),
                1,
            );
            row.last_seen_at = format!("2020-01-0{}T00:00:00+00:00", i + 1);
            row.is_favorite = i == 0; // oldest one is a favorite
            insert_clip(&c, &row).expect("insert");
        }
        // Keep newest 2 non-favorites; favorite exempt.
        let deleted = evict(&c, None, Some(2)).expect("evict");
        assert_eq!(deleted, 2);
        let remaining = count_clips(&c).expect("count");
        // 5 total - 2 evicted = 3 (2 recent non-fav + 1 favorite).
        assert_eq!(remaining, 3);

        // Age eviction removes the old non-favorites but not the favorite.
        let deleted_age = evict(&c, Some(1), None).expect("evict age");
        assert_eq!(deleted_age, 2);
        assert_eq!(count_clips(&c).expect("count"), 1);
        let last = recent_clips(&c, 10).expect("recent");
        assert!(last[0].is_favorite);
    }

    #[test]
    fn clear_removes_everything() {
        let c = conn();
        for i in 0..3 {
            let row = ClipRow::new(
                content_hash(format!("k{i}").as_bytes()),
                "text/plain".into(),
                "text".into(),
                1,
            );
            insert_clip(&c, &row).expect("insert");
            fts_add(
                &c,
                &row.id,
                FtsFields {
                    text: Some("clearme"),
                    ..Default::default()
                },
            )
            .expect("fts");
        }
        let n = clear_clips(&c).expect("clear");
        assert_eq!(n, 3);
        assert_eq!(count_clips(&c).expect("count"), 0);
        assert!(fts_search(&c, "clearme", 10).expect("search").is_empty());
    }
}
