# Image schema columns (integrator note)

Image capture (`clipboard-daemon::image_capture`) expects these nullable columns on `clips`:

| Column | Type | Purpose |
|--------|------|---------|
| `thumb_ciphertext` | BLOB | AES-GCM ciphertext of the thumbnail |
| `thumb_nonce` | BLOB | 12-byte nonce for `thumb_ciphertext` |
| `thumb_mime` | TEXT | e.g. `image/jpeg` or `image/png` |
| `width` | INTEGER | original width in pixels |
| `height` | INTEGER | original height in pixels |

Do **not** put image bytes in `text_ciphertext` / `text_nonce`.

Migration: schema version **2** (`clipboard_core::schema::SCHEMA_VERSION`). Applied via `ALTER TABLE` after v1.
