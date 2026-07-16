//! Clipboard capture loop (Phase 2).
//!
//! Polls the OS clipboard for text every `poll_interval_ms` via `arboard`.
//! On a change it hashes, categorizes, applies pause + secret heuristics +
//! app denylist, encrypts, and dedups/inserts through the store layer.
//!
//! When text is empty or unavailable, falls through to image capture
//! ([`crate::image_capture`]). Source app / window enrichment is not wired
//! here yet (returns `None`); see the platform-enricher TODOs in the crate
//! report. File-list payloads beyond text are still future work.

use crate::db::{self, CapturedClip};
use crate::image_capture;
use crate::state::AppState;
use clipboard_core::categorize::{categorize, Category};
use clipboard_core::schema::content_hash;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

const PREVIEW_MAX_CHARS: usize = 200;

/// Spawn the capture loop on a dedicated OS thread (arboard is blocking and its
/// clipboard handle is not `Send`). The thread exits when `shutdown` is set.
pub fn spawn(state: Arc<AppState>, shutdown: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("clipboard-capture".to_string())
        .spawn(move || run(state, shutdown))
        .expect("failed to spawn capture thread")
}

fn run(state: Arc<AppState>, shutdown: Arc<AtomicBool>) {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            // Headless / no display / Wayland-without-protocol: capture is
            // simply disabled; the IPC server keeps working.
            warn!(error = %e, "clipboard unavailable; capture disabled");
            return;
        }
    };
    info!("clipboard capture loop started");

    let mut last_hash: Option<String> = None;
    while !shutdown.load(Ordering::SeqCst) {
        let interval = Duration::from_millis(state.poll_interval_ms().max(50));

        if !state.is_paused() {
            match clipboard.get_text() {
                Ok(text) if !text.is_empty() => {
                    if let Err(e) = process_text(&state, &text, &mut last_hash) {
                        // Never log the body itself.
                        warn!(error = %e, "failed to store clip");
                    }
                }
                Ok(_) | Err(arboard::Error::ContentNotAvailable) => {
                    // IMAGE: call image_capture::try_store_image(...)
                    if let Err(e) =
                        image_capture::try_store_image(&state, &mut clipboard, &mut last_hash)
                    {
                        warn!(error = %e, "failed to store image clip");
                    }
                }
                Err(e) => debug!(error = %e, "clipboard read error"),
            }
        }

        std::thread::sleep(interval);
    }
    info!("clipboard capture loop stopped");
}

fn process_text(
    state: &Arc<AppState>,
    text: &str,
    last_hash: &mut Option<String>,
) -> anyhow::Result<()> {
    let normalized = normalize(text);
    if normalized.is_empty() {
        return Ok(());
    }
    let hash = content_hash(normalized.as_bytes());
    if last_hash.as_deref() == Some(hash.as_str()) {
        return Ok(());
    }

    // Secret heuristics (SPEC §5.4): skip storing high-confidence secrets.
    if state.secret_heuristics_enabled() && looks_secret(&normalized) {
        debug!("skipping clip flagged by secret heuristics");
        *last_hash = Some(hash);
        return Ok(());
    }

    // Source app/window enrichment is not wired yet, so these are `None`; the
    // denylist check still runs so it is correct the moment enrichers land.
    let source_app: Option<String> = None;
    let source_bundle_id: Option<String> = None;

    let category = categorize("text/plain", Some(&normalized));
    let mime = mime_for(category);
    let byte_size = normalized.len() as i64;
    let max_bytes = state.max_text_bytes();

    // Over the cap: keep preview + hash + metadata only (SPEC §4).
    let stored_text = if normalized.len() > max_bytes {
        None
    } else {
        Some(normalized.clone())
    };

    let clip = CapturedClip {
        content_hash: hash.clone(),
        mime: mime.to_string(),
        category: category.as_str().to_string(),
        text: stored_text,
        preview: preview(&normalized),
        source_app: source_app.clone(),
        source_bundle_id: source_bundle_id.clone(),
        source_window_title: None,
        source_url: None,
        byte_size,
    };

    let inserted = {
        let conn = state
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        if db::is_app_excluded(&conn, source_app.as_deref(), source_bundle_id.as_deref())? {
            debug!("skipping clip from denylisted app");
            *last_hash = Some(hash);
            return Ok(());
        }
        db::store_capture(&conn, &state.key, clip)?
    };
    if inserted {
        debug!(
            category = category.as_str(),
            bytes = byte_size,
            "stored new clip"
        );
    }
    *last_hash = Some(hash);
    Ok(())
}

fn mime_for(category: Category) -> &'static str {
    match category {
        Category::File => "text/uri-list",
        Category::Image => "image/png",
        _ => "text/plain",
    }
}

/// Normalize line endings and trim trailing whitespace for stable hashing.
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_string()
}

/// Short, single-line, escaped preview (≤200 chars). Never used for secrets
/// (those are dropped before this runs).
fn preview(text: &str) -> String {
    let single_line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = single_line.trim();
    trimmed.chars().take(PREVIEW_MAX_CHARS).collect()
}

/// Basic secret detection (SPEC §5.4). Intentionally conservative — high
/// confidence only. Extend with the full documented allowlist over time.
pub fn looks_secret(text: &str) -> bool {
    let t = text.trim();

    // PEM private key blocks.
    if t.contains("-----BEGIN") && t.contains("PRIVATE KEY-----") {
        return true;
    }

    // AWS access key id prefixes (AKIA/ASIA) followed by 16 uppercase/digits.
    for prefix in ["AKIA", "ASIA"] {
        if let Some(pos) = t.find(prefix) {
            let tail: String = t[pos + prefix.len()..]
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                .collect();
            if tail.len() >= 16 {
                return true;
            }
        }
    }

    // Common token prefixes.
    let token_prefixes = ["ghp_", "github_pat_", "xoxb-", "xoxp-", "sk-", "AIza"];
    for tok in token_prefixes {
        if let Some(pos) = t.find(tok) {
            let tail_len = t[pos + tok.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .count();
            if tail_len >= 12 {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipboard_core::categorize::{categorize, Category};

    #[test]
    fn detects_pem_private_key() {
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIabc\n-----END RSA PRIVATE KEY-----";
        assert!(looks_secret(pem));
    }

    #[test]
    fn detects_aws_access_key() {
        assert!(looks_secret("key: AKIAIOSFODNN7EXAMPLE"));
        assert!(looks_secret("ASIAY34FZKBOKMYDR7GH"));
    }

    #[test]
    fn detects_common_token_prefixes() {
        assert!(looks_secret("ghp_1234567890abcdefghij"));
        assert!(looks_secret("sk-abcdefghijklmnop"));
    }

    #[test]
    fn ignores_ordinary_text() {
        assert!(!looks_secret("just a normal sentence about akita dogs"));
        assert!(!looks_secret("meeting notes for tomorrow"));
    }

    #[test]
    fn normalize_strips_trailing_and_crlf() {
        assert_eq!(normalize("a\r\nb   \n"), "a\nb");
    }

    #[test]
    fn preview_is_single_line_and_bounded() {
        let p = preview("line1\nline2\tend");
        assert!(!p.contains('\n'));
        let long = "x".repeat(500);
        assert_eq!(preview(&long).chars().count(), PREVIEW_MAX_CHARS);
    }

    #[test]
    fn categorizer_recognizes_url_and_code() {
        assert_eq!(
            categorize("text/plain", Some("https://example.com/path")),
            Category::Url
        );
        assert_eq!(
            categorize("text/plain", Some("fn main() { let x = 1; return x; }")),
            Category::Code
        );
        assert_eq!(
            categorize("text/plain", Some("just some plain words here")),
            Category::Text
        );
    }
}
