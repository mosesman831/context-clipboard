# Context Clipboard

> **LatticeAG · Desktop** · Local-first clipboard manager with source context
>
> **Status:** build-ready. Decisions locked from [`PLAN.md`](./PLAN.md). Implement against this document.

A desktop clipboard manager that remembers not only *what* you copied, but *where* it came from (app, window title, time, optional browser URL). History stays on device, encrypted at rest, searchable with fast keyword and metadata filters.

**v1 promise:** privacy-conscious capture + source context + trustworthy local search on macOS and Linux X11. Semantic search and browser-tab URL enrichment land in v1.1 / phase 2.

---

## 1. Product

### Problem

Clipboard managers exist (CopyQ, Pastebot, Alfred/Raycast clipboard history, Windows Clipboard History). Hours later you remember the snippet but not which app or page it came from. Keyword search across plain history is weak when you only remember *context*.

### Positioning (honest)

- **Not claiming:** “nobody else does context or search.”
- **Claiming:** local-only by default, encrypted history, per-app exclusions and retention, source metadata as a first-class field, OSS with a clear threat model.
- **Competitors to acknowledge in README:** CopyQ (OSS/scriptable), Alfred/Raycast (launcher-integrated), Pastebot (polished macOS), system clipboard history on Windows/macOS.

### Non-goals (v1)

- Cloud sync / accounts / multi-device relay
- Required ML / semantic search
- Windows
- Soft ML category labels (“meeting notes”)
- Image OCR or image embeddings
- Open localhost HTTP bridges for browser extensions

---

## 2. Locked decisions

| Decision | Choice |
|---|---|
| License | Apache-2.0 |
| Name | `context-clipboard` (product: Context Clipboard; org: LatticeAG) |
| Process model | Always-on Rust daemon + on-demand Tauri tray UI over local IPC |
| Search v1 | FTS5 + metadata filters (required) |
| Semantic search | Deferred to **v1.1** (design hooks now: `clip_vec` table optional/empty) |
| Embeddings runtime (v1.1) | ONNX Runtime + BGE-small-en-v1.5 int8 — **not** llama.cpp |
| Encryption at rest | Required in v1 (OS keychain-backed key) |
| Sync | Out of v1; schema keeps UUIDs + `updated_at` + migrations |
| Platforms | macOS + Linux X11 first-class; Wayland best-effort; Windows out |
| Browser URLs | Phase 2 via Chrome Native Messaging; v1 stores app + window title only |
| Hotkey default | `Cmd+Shift+.` / `Ctrl+Shift+.` (rebindable); avoid Paste-and-Match-Style collision |
| Categorization | Deterministic rules: `url`, `code`, `text`, `image`, `file` |
| Telemetry | Off by default; never log clipboard contents |

---

## 3. Architecture

```
┌─────────────────────────────────────────────────────────┐
│  context-clipboardd  (Rust, always-on, single-instance) │
│  - pasteboard / clipboard watcher                       │
│  - dedup, size caps, secret heuristics, app denylist    │
│  - rules categorizer                                    │
│  - encrypt + write SQLite (WAL)                         │
│  - FTS5 index writer                                    │
│  - retention / eviction                                 │
│  - Unix domain socket IPC server                        │
└──────────────────────────┬──────────────────────────────┘
                           │ length-prefixed JSON (v1)
                           ▼
┌─────────────────────────────────────────────────────────┐
│  context-clipboard  (Tauri, spawn on tray / hotkey)     │
│  - system tray icon                                     │
│  - popup: recent 5 + search                             │
│  - settings: exclusions, retention, hotkey, pause       │
│  - read-mostly DB access via daemon IPC                 │
└─────────────────────────────────────────────────────────┘

Phase 2 (not v1):
  Chrome extension ↔ Native Messaging host ↔ daemon
  (URL + page title when copy originated in Chrome)

v1.1 (not v1):
  embed worker (ORT + BGE) load-on-demand → clip_vec → hybrid search
```

### Process rules

- Daemon is the **only** SQLite writer.
- UI connects as a client; may hold a **read-only** SQLite connection for search if that proves faster, but writes always go through the daemon.
- UI crash must not stop capture.
- Single-instance: lockfile + socket path under the platform runtime dir.
- Autostart installs the **daemon** (launchd agent / XDG autostart). Tray UI may also autostart minimized, but capture does not depend on the UI process.
- Logs: journald / os_log / rotating file under user data dir. Redact clip bodies always.

### Repo layout (target)

```
/
  SPEC.md
  PLAN.md
  LICENSE                 # Apache-2.0
  README.md
  SECURITY.md
  CONTRIBUTING.md
  CODE_OF_CONDUCT.md
  Cargo.toml              # workspace
  crates/
    clipboard-daemon/     # context-clipboardd
    clipboard-core/       # shared: schema, crypto, categorize, ipc types
    clipboard-ui/         # Tauri app
  extension/              # Chrome (phase 2; stub dir ok in v1)
  fixtures/               # categorization + FTS golden fixtures
  .github/workflows/      # CI
```

---

## 4. Data model

### Paths

| Platform | Data dir |
|---|---|
| macOS | `~/Library/Application Support/ContextClipboard/` |
| Linux | `$XDG_DATA_HOME/context-clipboard/` (default `~/.local/share/context-clipboard/`) |

Files: `history.db` (SQLite), `history.db-wal`, `history.db-shm`, `config.toml`, `logs/`.

Key material: OS keychain / secret service entry `latticeag.context-clipboard.db-key` (32-byte AES key).

### Schema (v1)

```sql
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

CREATE TABLE schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL
);

CREATE TABLE clips (
  id TEXT PRIMARY KEY,                 -- UUID v7
  content_hash TEXT NOT NULL,          -- SHA-256 of normalized plaintext bytes
  mime TEXT NOT NULL,                  -- e.g. text/plain, image/png, text/uri-list
  category TEXT NOT NULL,              -- url | code | text | image | file
  -- ciphertext columns store AEAD blobs (nonce||ciphertext||tag) as BLOB
  text_ciphertext BLOB,                -- NULL for pure image/file
  text_nonce BLOB,
  preview_plaintext TEXT,              -- short safe preview for UI list (≤200 chars), still subject to exclusion rules
  source_app TEXT,
  source_bundle_id TEXT,               -- macOS bundle id / Linux app_id when known
  source_window_title TEXT,
  source_url TEXT,                     -- NULL in v1 unless phase-2 extension later backfills
  byte_size INTEGER NOT NULL,
  created_at TEXT NOT NULL,            -- RFC3339 UTC
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

-- populated by daemon on insert/update/delete of searchable text
CREATE TABLE clips_fts_map (
  clip_id TEXT PRIMARY KEY REFERENCES clips(id) ON DELETE CASCADE,
  fts_rowid INTEGER NOT NULL
);

CREATE TABLE apps_excluded (
  id INTEGER PRIMARY KEY,
  match_type TEXT NOT NULL,            -- bundle_id | app_name | path_prefix
  match_value TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(match_type, match_value)
);

CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- reserved for v1.1; create empty so migrations stay forward-compatible
CREATE TABLE clip_vec (
  clip_id TEXT PRIMARY KEY REFERENCES clips(id) ON DELETE CASCADE,
  dim INTEGER NOT NULL CHECK(dim = 384),
  embedding BLOB NOT NULL,             -- float32[384] or int8[384] (documented in v1.1)
  model_id TEXT NOT NULL,
  embedded_at TEXT NOT NULL
);
```

### Dedup and caps

- On capture: hash normalized UTF-8 text (or image bytes). If hash exists → update `last_seen_at` / source metadata if richer; do not insert a duplicate row.
- Max stored text per item: **1 MiB**. Larger → store preview + hash + metadata only (`category` still set).
- Max embedded/searchable text (v1.1): first **8 KiB** after normalization.
- Images: store **thumbnail** (max edge 512px, JPEG/WebP) encrypted; full image optional up to **5 MiB**, else thumbnail + hash only.
- File lists (`text/uri-list` / file URLs): store paths/names as text category `file`; do not copy file contents into the DB.

### Retention (defaults)

| Setting | Default |
|---|---|
| Max age | 30 days |
| Max items | 10_000 |
| Eviction | oldest by `last_seen_at`, favorites exempt |
| Favorites | unlimited until user deletes |

Settings are user-configurable in the UI.

---

## 5. Privacy and security

### Threat model (v1)

**In scope**

- Local malware / other users on the same machine reading `history.db` from disk → mitigated by AEAD encryption + key in OS keychain.
- Accidental long-term storage of secrets → mitigated by denylist, concealed types, secret heuristics, retention, pause, delete.
- UI XSS if clipboard HTML rendered in webview → mitigated by never rendering raw HTML; plaintext/escaped preview only; CSP; minimal Tauri IPC allowlist.

**Out of scope (v1)**

- Attacker who already has the unlocked user session and keychain access (they can read clipboard live anyway).
- Evil-maid with keychain unlock and user passphrase.
- Compelled cloud disclosure (no cloud).

### Capture exclusions (defaults)

1. **Pause capture** — tray toggle + hotkey (`Cmd+Shift+P` / `Ctrl+Shift+P`).
2. **App denylist** (seeded, editable):
   - macOS: 1Password, Bitwarden, LastPass, KeePassXC, Keychain Access (bundle IDs listed in `clipboard-core`).
   - Linux: matching desktop app_ids / process names for the same class of tools.
3. **macOS concealed pasteboard** — honor `org.nspasteboard.ConcealedType` (and transient types): do not store.
4. **Secret heuristics** (skip store if high confidence):
   - PEM private key blocks
   - AWS-style access keys / common token prefixes (documented allowlist of patterns in code)
   - Credit-card numbers passing Luhn + length checks
5. **Private/incognito** — best-effort via window title heuristics only in v1; do not claim reliability. Phase 2 extension can mark private windows explicitly.

### Crypto

- Algorithm: **AES-256-GCM**.
- Key: 32 random bytes in OS keychain / libsecret.
- Per-field nonce; never reuse nonce with same key.
- `preview_plaintext` is a convenience for the list UI; it must still respect exclusion (never preview skipped secrets). Prefer empty preview when unsure.
- Secure delete: `DELETE` + periodic `PRAGMA wal_checkpoint(TRUNCATE)` + optional `VACUUM` from settings. Document that SQLite deletion is not forensic wipe.

### IPC security

- Unix domain socket in user runtime dir with `0600` permissions.
- Protocol: length-prefixed JSON (schema versioned). Upgrade path to protobuf later if needed.
- No TCP localhost server in v1.
- Daemon authenticates peer UID == self.

### Webview security

- CSP default-deny with only local assets.
- Clipboard content displayed as text nodes / escaped strings only.
- Disable Tauri APIs not required by the UI allowlist.

---

## 6. Platform matrix

| Capability | macOS | Linux X11 | Linux Wayland |
|---|---|---|---|
| Text/image clipboard capture | Required | Required | Best-effort |
| Frontmost app name | Required | Required (EWMH) | Best-effort / often unavailable |
| Window title | Optional (needs Accessibility) | Required when EWMH allows | Best-effort |
| Browser tab URL | Phase 2 extension | Phase 2 extension | Phase 2 extension |
| Global hotkey | Required | Required | Best-effort |
| Autostart | launchd Launch Agent | XDG autostart | XDG autostart |
| Keychain | Keychain Services | Secret Service / libsecret | Secret Service |
| Signed release | Developer ID + notarization | checksummed AppImage + deb | same packages |

### macOS specifics

- Capture via `NSPasteboard` changeCount polling (interval ~200–500ms) or equivalent change notifications.
- Accessibility permission: **optional**. First-run screen explains what it unlocks (window titles). Core history works without it.
- Hardened runtime, notarized `.dmg` or `.pkg`.
- Minimum: macOS 13+.

### Linux specifics

- X11: selection/`CLIPBOARD` monitoring via existing crate patterns (e.g. x11rb / arboard with care for ownership).
- Wayland: use available protocols (`ext-data-control` / compositor-specific) when present; otherwise degrade gracefully and document.
- Supported test targets for v1 CI: Ubuntu latest X11 (Xvfb), plus manual GNOME/KDE notes in README.
- Minimum: glibc-based distros we package for (Ubuntu 22.04+ / Fedora recent).

### Hotkeys

| Action | Default |
|---|---|
| Open popup | `Cmd+Shift+.` / `Ctrl+Shift+.` |
| Pause/resume capture | `Cmd+Shift+P` / `Ctrl+Shift+P` |

Both rebindable in settings. Persist to `config.toml` / `settings` table.

---

## 7. Features (v1 checklist)

### Capture and store

- [ ] Daemon watches clipboard for text, images, file URL lists
- [ ] Enrich with timestamp, app name, bundle/app id, window title when available
- [ ] Dedup by content hash; update `last_seen_at`
- [ ] Encrypt sensitive fields; write SQLite + FTS
- [ ] Apply denylist, concealed types, secret heuristics, pause
- [ ] Enforce retention (age + count)
- [ ] Image thumbnails with size caps

### UI

- [ ] System tray icon (paused state visually distinct)
- [ ] Popup: 5 most recent items
- [ ] Search box: FTS over text + app + window title (+ URL column when present)
- [ ] Filters: category, app, time range
- [ ] Click / Enter pastes selected item (write to clipboard + optional auto-paste where OS allows)
- [ ] Delete item, clear history, favorite toggle
- [ ] Settings: retention, exclusions, hotkeys, launch at login, reveal data dir
- [ ] First-run onboarding (permissions, what is stored, pause control)

### Categorization (rules)

- [ ] `url` — single URL / URI parse success
- [ ] `image` — image MIME / bitmap payload
- [ ] `file` — file URL list / paths
- [ ] `code` — fences, high symbol density, shebang, common extension-like snippets (heuristic documented)
- [ ] `text` — default
- [ ] Fixture suite ≥500 items under `fixtures/`; CI fails if accuracy on fixtures < **90%**

### Packaging and OSS

- [ ] Apache-2.0 `LICENSE`
- [ ] `SECURITY.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, honest README
- [ ] CI: fmt, clippy, test, fixture suite, macOS build, Linux build
- [ ] Signed macOS package + Linux AppImage + `.deb`
- [ ] Signed update channel (Tauri updater or daemon-aware equivalent)
- [ ] Platform support table in README

### Explicitly not in v1

- [ ] Semantic / vector search (v1.1)
- [ ] Chrome extension (phase 2)
- [ ] Sync
- [ ] Windows
- [ ] Telemetry

---

## 8. Success criteria (measurable)

| Metric | Target | How measured |
|---|---|---|
| Daemon RSS idle (model N/A in v1) | ≤ **25 MB** p95 | `ps`/`smem` on M-series macOS + Ubuntu X11 after 5 min idle |
| UI RSS while popup open | ≤ **80 MB** p95 | same, popup open with 10k DB |
| FTS search latency | ≤ **100 ms** p95 | query suite over 10k fixture clips, warm DB |
| Capture drop rate under burst | ≤ **1%** | 1000 rapid copies scripted |
| Categorization on fixtures | ≥ **90%** | `fixtures/` CI job |
| Core history without Accessibility | Works | automated + manual |
| No clip contents in default logs | Audited | grep CI on sample log output |
| First OSS release | macOS notarized + Linux AppImage/deb | release checklist |

Semantic latency and embedder RSS targets apply only when v1.1 lands (see §12).

---

## 9. IPC sketch (v1)

Versioned messages over the Unix socket. Illustrative, not exhaustive:

```text
Client → Server
  Ping { v }
  Search { query, limit, category?, app?, since? }
  Recent { limit }
  Get { id }
  Delete { id }
  Clear {}
  SetPaused { paused }
  GetStatus {}
  UpdateSettings { ... }
  ListExcludedApps {}
  AddExcludedApp { ... }
  RemoveExcludedApp { id }

Server → Client
  Ok / Err { code, message }
  Status { paused, count, version, platform_caps }
  SearchResults { items: [ClipSummary] }
  ClipDetail { ... }   // decrypts text for paste/reveal only
```

`ClipSummary` includes id, preview, category, app, window title, timestamps — never full ciphertext.

---

## 10. Config defaults (`config.toml`)

```toml
[capture]
paused = false
poll_interval_ms = 300
max_text_bytes = 1048576
max_image_bytes = 5242880
thumbnail_max_edge = 512

[retention]
max_age_days = 30
max_items = 10000

[ui]
hotkey_open = "Cmd+Shift+Period"
hotkey_pause = "Cmd+Shift+P"
recent_count = 5
launch_at_login = true

[privacy]
honor_concealed = true
secret_heuristics = true
# apps_excluded seeded in DB on first run
```

---

## 11. Build phases (implementation order)

Do not interpret as calendar weeks. Complete each phase’s exit criteria before the next.

### Phase 0 — repo bootstrap

- Cargo workspace, Apache-2.0, README stub, CI skeleton, `clipboard-core` crate with schema + migrations.
- **Exit:** `cargo test` green on Linux CI; empty DB migrates.

### Phase 1 — walking skeleton

- Daemon single-instance + socket + Ping/Status.
- Tauri tray opens popup shell talking to daemon.
- Autostart stubs; hotkey registration; first-run permission copy.
- **Exit:** install locally, tray opens, daemon survives UI quit.

### Phase 2 — capture + privacy

- Clipboard watchers (macOS + Linux X11).
- Encrypt, insert, dedup, caps, denylist, concealed, heuristics, pause, retention.
- **Exit:** copy text/image/file-list; rows appear encrypted; exclusions work; burst test passes.

### Phase 3 — findability + UI

- FTS5 search, recent 5, filters, paste, delete/clear/favorite, settings UI.
- Categorization + fixture CI.
- **Exit:** success criteria for FTS latency + categorization met on fixtures.

### Phase 4 — packaging + OSS release

- Signed macOS, Linux AppImage + deb, updater, SECURITY.md, support table, release tags/checksums.
- **Exit:** v1.0.0 GitHub release installable on both platforms.

### Phase 5 — v1.1 semantic (after v1.0)

- ORT + BGE-small-en-v1.5 int8, load on demand, unload when UI closed / idle timeout.
- Fill `clip_vec`; hybrid: FTS first, semantic rerank/parallel when enabled.
- Golden embedding tests vs reference vectors; `query:` / `passage:` prefixes.
- SLOs: daemon+warm embedder ≤120 MB; semantic p95 ≤300 ms on 10k warm.

### Phase 6 — Chrome Native Messaging (phase 2)

- Extension + host manifest; correlate with OS clipboard events; fill `source_url`.
- No open HTTP server.

---

## 12. v1.1 / phase 2 hooks (do not build in v1.0)

Documented so v1 schema and IPC do not paint us into a corner:

- `clip_vec` table exists empty.
- Settings keys reserved: `semantic.enabled`, `semantic.model_id`.
- IPC may add `SearchMode { fts | hybrid }` later without breaking v1 clients (additive).
- Extension will use Native Messaging only; reserve host name `com.latticeag.context_clipboard`.

---

## 13. OSS production checklist

Ship blockers for calling v1 “production OSS”:

- [ ] `LICENSE` Apache-2.0
- [ ] Third-party notices (Tauri, SQLite, ORT when added, fonts)
- [ ] `SECURITY.md` with disclosure contact and threat model summary
- [ ] `CONTRIBUTING.md` (dev setup: Rust stable, Node for UI, platform notes)
- [ ] `CODE_OF_CONDUCT.md`
- [ ] README: install, permissions, support matrix, competitors honesty, privacy defaults
- [ ] CI matrix: Linux + macOS, tests, clippy, fmt, fixtures
- [ ] Release workflow: tagged builds, checksums, notarization, signed updater keys
- [ ] No default telemetry; document crash-report policy if ever added
- [ ] Model distribution policy written before v1.1 (download + checksum, not silent)

---

## 14. Open implementation details (left to implementers, not product forks)

These are intentional flex points; do not bikeshed in SPEC:

- Exact Rust crates for arboard/x11/wayland (prefer maintained, audit before pin).
- UI framework inside Tauri (vanilla / Svelte / React) — pick one and stay consistent; prefer small bundle.
- Whether UI uses read-only SQLite vs pure IPC for search — measure in phase 3.
- Thumbnail codec (JPEG vs WebP).

If a choice affects privacy, platform support, or the locked decisions in §2, update this SPEC in the same PR.

---

## 15. Definition of done for “ready to build”

This SPEC is the build contract when:

1. Locked decisions in §2 are unchanged (they are).
2. Phases 0–4 define v1.0 scope exclusively.
3. Success criteria in §8 are the acceptance tests for v1.0.

**Next step:** implement Phase 0 (repo bootstrap) against this document.
