//! Image clipboard capture helpers (SPEC §7, §16.6).
//!
//! Reads RGBA bitmaps from `arboard`, builds a capped thumbnail, encrypts it,
//! and persists via [`crate::db::store_image`]. Decode failures are soft-skipped
//! so the capture loop keeps running.

use crate::db::{self, CapturedImage};
use crate::state::AppState;
use clipboard_core::schema::content_hash;
use image::imageops::FilterType;
use image::{DynamicImage, ImageBuffer, ImageFormat, RgbaImage};
use std::io::Cursor;
use std::sync::Arc;
use tracing::{debug, warn};

/// Default max edge for thumbnails when config is unavailable.
pub const DEFAULT_THUMB_MAX_EDGE: u32 = 512;
/// Default full-image byte cap (5 MiB).
pub const DEFAULT_MAX_IMAGE_BYTES: usize = 5_242_880;
/// JPEG quality for encoded thumbnails.
const JPEG_QUALITY: u8 = 85;

/// Prepared image payload ready to encrypt and store.
#[derive(Debug, Clone)]
pub struct PreparedImage {
    pub content_hash: String,
    pub width: u32,
    pub height: u32,
    pub thumb_bytes: Vec<u8>,
    pub thumb_mime: String,
    pub preview: String,
    pub mime: String,
    pub byte_size: i64,
    /// Encoded full image when ≤ max_image_bytes; otherwise `None`.
    pub full_encoded: Option<Vec<u8>>,
}

/// Compute thumbnail width/height so the longest edge is ≤ `max_edge`.
///
/// Zero-dimension inputs yield `(0, 0)`. Values already within the cap are
/// returned unchanged.
pub fn thumbnail_dimensions(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    if width == 0 || height == 0 || max_edge == 0 {
        return (0, 0);
    }
    let longest = width.max(height);
    if longest <= max_edge {
        return (width, height);
    }
    let scale = f64::from(max_edge) / f64::from(longest);
    let tw = (f64::from(width) * scale).round().max(1.0) as u32;
    let th = (f64::from(height) * scale).round().max(1.0) as u32;
    (tw, th)
}

/// Build a list preview like `[image 800x600]`.
pub fn image_preview(width: u32, height: u32) -> String {
    format!("[image {width}x{height}]")
}

/// Encode an RGBA buffer as a JPEG or PNG thumbnail and optional full image.
///
/// Returns `None` when the buffer length does not match `width * height * 4`
/// (undecodable / corrupt) so callers can skip without crashing.
pub fn prepare_rgba_image(
    width: u32,
    height: u32,
    rgba: &[u8],
    max_edge: u32,
    max_image_bytes: usize,
) -> Option<PreparedImage> {
    let expected = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
    if rgba.len() != expected || width == 0 || height == 0 {
        return None;
    }

    let img: RgbaImage = ImageBuffer::from_raw(width, height, rgba.to_vec())?;
    let hash = content_hash(rgba);

    let (tw, th) = thumbnail_dimensions(width, height, max_edge);
    let thumb_img = if tw == width && th == height {
        DynamicImage::ImageRgba8(img.clone())
    } else {
        DynamicImage::ImageRgba8(img.clone()).resize_exact(tw, th, FilterType::Triangle)
    };

    let (thumb_bytes, thumb_mime) = encode_thumbnail(&thumb_img)?;
    let full_encoded = encode_full_png(&DynamicImage::ImageRgba8(img), max_image_bytes);

    Some(PreparedImage {
        content_hash: hash,
        width,
        height,
        thumb_bytes,
        thumb_mime,
        preview: image_preview(width, height),
        mime: "image/png".to_string(),
        byte_size: (width as i64) * (height as i64) * 4,
        full_encoded,
    })
}

fn encode_thumbnail(img: &DynamicImage) -> Option<(Vec<u8>, String)> {
    // Prefer JPEG for compact thumbs; fall back to PNG if JPEG encode fails.
    let rgb = img.to_rgb8();
    let mut jpeg_buf = Cursor::new(Vec::new());
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buf, JPEG_QUALITY);
    if encoder.encode_image(&rgb).is_ok() {
        return Some((jpeg_buf.into_inner(), "image/jpeg".to_string()));
    }

    let mut png_buf = Cursor::new(Vec::new());
    if img.write_to(&mut png_buf, ImageFormat::Png).is_ok() {
        return Some((png_buf.into_inner(), "image/png".to_string()));
    }
    None
}

fn encode_full_png(img: &DynamicImage, max_image_bytes: usize) -> Option<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png).ok()?;
    let bytes = buf.into_inner();
    if bytes.len() <= max_image_bytes {
        Some(bytes)
    } else {
        None
    }
}

/// Read an image from the clipboard (when text is empty/unavailable), prepare
/// a thumbnail, and store it. Returns `Ok(true)` when a new or touched row was
/// processed, `Ok(false)` when there was nothing usable.
pub fn try_store_image(
    state: &Arc<AppState>,
    clipboard: &mut arboard::Clipboard,
    last_hash: &mut Option<String>,
) -> anyhow::Result<bool> {
    let image = match clipboard.get_image() {
        Ok(img) => img,
        Err(arboard::Error::ContentNotAvailable) => return Ok(false),
        Err(e) => {
            debug!(error = %e, "clipboard image read error");
            return Ok(false);
        }
    };

    let width = image.width as u32;
    let height = image.height as u32;
    let rgba = image.bytes.as_ref();

    let max_edge = state.thumbnail_max_edge();
    let max_bytes = state.max_image_bytes();

    let prepared = match prepare_rgba_image(width, height, rgba, max_edge, max_bytes) {
        Some(p) => p,
        None => {
            debug!(width, height, "skipping undecodable clipboard image");
            return Ok(false);
        }
    };

    if last_hash.as_deref() == Some(prepared.content_hash.as_str()) {
        return Ok(false);
    }

    let source_app: Option<String> = None;
    let source_bundle_id: Option<String> = None;

    let captured = CapturedImage {
        content_hash: prepared.content_hash.clone(),
        mime: prepared.mime,
        preview: prepared.preview,
        thumb_bytes: prepared.thumb_bytes,
        thumb_mime: prepared.thumb_mime,
        width: prepared.width as i64,
        height: prepared.height as i64,
        source_app: source_app.clone(),
        source_bundle_id: source_bundle_id.clone(),
        source_window_title: None,
        source_url: None,
        byte_size: prepared.byte_size,
    };

    let inserted = {
        let conn = state
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        if db::is_app_excluded(&conn, source_app.as_deref(), source_bundle_id.as_deref())? {
            debug!("skipping image from denylisted app");
            *last_hash = Some(prepared.content_hash);
            return Ok(false);
        }
        match db::store_image(&conn, &state.key, captured) {
            Ok(v) => v,
            Err(e) => {
                // Soft-fail schema/store issues so the capture loop continues.
                warn!(error = %e, "failed to store image clip");
                return Ok(false);
            }
        }
    };

    if inserted {
        debug!(
            width = prepared.width,
            height = prepared.height,
            bytes = prepared.byte_size,
            "stored new image clip"
        );
    }
    *last_hash = Some(prepared.content_hash);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_downscale_caps_longest_edge() {
        assert_eq!(thumbnail_dimensions(800, 600, 512), (512, 384));
        assert_eq!(thumbnail_dimensions(600, 800, 512), (384, 512));
        assert_eq!(thumbnail_dimensions(1024, 1024, 512), (512, 512));
    }

    #[test]
    fn thumbnail_preserves_small_images() {
        assert_eq!(thumbnail_dimensions(320, 240, 512), (320, 240));
        assert_eq!(thumbnail_dimensions(1, 1, 512), (1, 1));
    }

    #[test]
    fn thumbnail_zero_inputs() {
        assert_eq!(thumbnail_dimensions(0, 100, 512), (0, 0));
        assert_eq!(thumbnail_dimensions(100, 0, 512), (0, 0));
        assert_eq!(thumbnail_dimensions(100, 100, 0), (0, 0));
    }

    #[test]
    fn thumbnail_wide_and_tall_aspect() {
        let (w, h) = thumbnail_dimensions(2000, 100, 512);
        assert_eq!(w, 512);
        assert_eq!(h, 26); // 100 * 512/2000 ≈ 25.6 → 26
        let (w2, h2) = thumbnail_dimensions(100, 2000, 512);
        assert_eq!(w2, 26);
        assert_eq!(h2, 512);
    }

    #[test]
    fn image_preview_format() {
        assert_eq!(image_preview(800, 600), "[image 800x600]");
    }

    #[test]
    fn prepare_rejects_bad_buffer_len() {
        assert!(prepare_rgba_image(2, 2, &[0u8; 10], 512, DEFAULT_MAX_IMAGE_BYTES).is_none());
        assert!(prepare_rgba_image(0, 0, &[], 512, DEFAULT_MAX_IMAGE_BYTES).is_none());
    }

    #[test]
    fn prepare_solid_rgba_produces_thumb() {
        let w = 64u32;
        let h = 48u32;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for px in rgba.chunks_exact_mut(4) {
            px[0] = 200;
            px[1] = 100;
            px[2] = 50;
            px[3] = 255;
        }
        let prepared =
            prepare_rgba_image(w, h, &rgba, 512, DEFAULT_MAX_IMAGE_BYTES).expect("prepare");
        assert_eq!(prepared.width, w);
        assert_eq!(prepared.height, h);
        assert_eq!(prepared.preview, "[image 64x48]");
        assert!(!prepared.thumb_bytes.is_empty());
        assert!(
            prepared.thumb_mime == "image/jpeg" || prepared.thumb_mime == "image/png",
            "unexpected mime {}",
            prepared.thumb_mime
        );
        assert_eq!(prepared.content_hash, content_hash(&rgba));
        // Small image fits under the full-image cap.
        assert!(prepared.full_encoded.is_some());
    }

    #[test]
    fn prepare_downscales_large_image() {
        let w = 1000u32;
        let h = 800u32;
        let rgba = vec![128u8; (w * h * 4) as usize];
        let prepared =
            prepare_rgba_image(w, h, &rgba, 512, DEFAULT_MAX_IMAGE_BYTES).expect("prepare");
        // Original dims preserved in metadata; thumb is smaller on disk.
        assert_eq!(prepared.width, w);
        assert_eq!(prepared.height, h);
        assert!(!prepared.thumb_bytes.is_empty());
        // Encoded thumb should be far smaller than raw RGBA.
        assert!(prepared.thumb_bytes.len() < rgba.len());
    }
}
