# Context Clipboard — production / OSS readiness plan

**Status:** planning only. No application code exists yet. Repo today is `SPEC.md` plus this plan.

**Goal of this doc:** lock the highest-leverage SPEC fixes before any build. After these decisions are accepted, we rewrite `SPEC.md` and then implement.

---

## Verdict

The product thesis is good: local clipboard history with *source context* and strong findability. The current SPEC is not shippable as written. The blockers are contradictions and missing production concerns, not missing UI polish.

Three independent reviews of the SPEC agree on the same cluster of problems: impossible RAM targets, privacy underspecification, undefined process/data model, platform permission reality, and OSS release scaffolding that does not exist.

---

## Current baseline

| Item | State |
|---|---|
| Application code | None |
| SPEC | Vision + 6 features + 4 success criteria + 3-week outline |
| License / CI / packaging / threat model | Absent |
| Schema / IPC / permissions matrix | Absent |

We are not fixing a codebase. We are fixing the contract we will build against.

---

## Must-fix before build (ranked)

### 1. Kill the impossible memory targets

**Problem.** `<10MB idle` and `<50MB during search` cannot coexist with Tauri (webview) plus a warm BGE embedder. Empty Tauri tray apps commonly sit in the tens of MB. BGE-small weights alone are ~25–130MB depending on quantization; a running embedder is more.

**Why it matters.** Bad success criteria drive bad architecture (hidden processes, fake “idle” definitions, endless optimization thrash).

**Proposed lock:**

- Split SLOs by process, not one product-wide number.
- Daemon idle, model unloaded: ≤25MB RSS.
- Daemon + warm embedder: ≤120MB RSS.
- Search UI while open: ≤80MB additional RSS.
- Search latency: FTS p95 <100ms over 10k items; semantic p95 <300ms over 10k *already-embedded* items when the model is warm.
- Embedding is async on ingest and never part of the search latency budget.

### 2. Rewrite the privacy / retention / secrets model

**Problem.** The app permanently indexes everything the user copies. “Local-first, zero cloud” is not a threat model. No retention cap, no secret exclusion, no at-rest encryption, no pause-capture.

**Why it matters.** Clipboard managers store passwords, tokens, customer data, and private messages. An OSS privacy brand that ships an unencrypted searchable history of secrets will not get trusted.

**Proposed lock:**

- Default retention: 30 days *or* max N items (configurable), whichever hits first.
- Exclude by default: password-manager apps (denylist), macOS concealed pasteboard types, high-confidence secrets (API keys, PEM private keys, credit-card Luhn).
- Pause capture: tray toggle + global shortcut.
- Per-app exclusion in v1.
- At-rest encryption for clip text/blobs via OS keychain-backed key.
- Clear history / delete item / inspect stored metadata in v1.
- No telemetry by default. Crash reports opt-in and must never include clip contents.

### 3. Resolve “zero cloud” vs sync

**Problem.** Adjacent bullets say “zero cloud” and “optional encrypted sync via DO tunnel.” The tunnel is undefined (transport, keys, conflicts, threat model).

**Proposed lock:**

- v1 is local-only. No sync.
- Reframe messaging: “local-only by default.”
- Sync is a separate design after v1, with its own threat model. Cheap prep now: UUID item IDs, `updated_at`, schema migrations table.

### 4. Lock process architecture and data model

**Problem.** Diagram shows a “Rust daemon”; stack says Tauri app. No schema, no dedup, no FTS, no vector index strategy, no retention, no WAL ownership.

**Proposed lock:**

- **Process:** always-on Rust daemon owns capture + SQLite write. Tauri tray UI is on-demand, connects over local Unix socket IPC, read-mostly. UI crash must not stop capture. Single-instance via lockfile/socket.
- **Rationale for split:** capture reliability and honest memory accounting beat “one process is simpler” for this product. Start with the split; do not invent a second daemon later under pressure.
- **Store:** SQLite WAL. Tables roughly: `clips` (uuid, content_hash, mime, text_ciphertext, app, window_title, url, category, created_at, last_seen_at, size), `clips_fts` (FTS5), `clip_vec` (384-d via sqlite-vec or float blob), `apps_excluded`, `schema_migrations`.
- **Dedup:** content-hash; repeated copy touches `last_seen_at`.
- **Caps:** text store max ~1–2MB per item; embed truncated text; images store thumbnail + hash, full blob size-capped or optional.
- **Retrieval v1:** FTS5 + metadata filters as the default path. Semantic is optional / experimental (see #5).

### 5. Demote semantic search from “required v1” to “optional / v1.1”

**Problem.** Semantic search is the differentiator *and* the largest source of RAM, packaging, license, and battery risk. llama.cpp is a heavy, LLM-oriented stack for a 33M sentence transformer.

**Proposed lock:**

- v1 ships fast keyword + metadata + time + app search as the guaranteed path.
- Semantic search is either (a) experimental behind a setting, or (b) deferred to v1.1.
- If kept experimental in v1: ONNX Runtime (or Candle) + BGE-small-en-v1.5 int8, not llama.cpp. Correct `query:` / `passage:` prefixes. Model unload when UI closed / after idle. Golden embedding tests against reference vectors.
- Vector search at personal corpus size (10k–100k): exact kNN via sqlite-vec or SIMD brute force. No HNSW in v1.

### 6. Face platform permission reality

**Problem.** macOS Accessibility / Input Monitoring, signing, notarization. Linux X11 vs Wayland. Cmd+Shift+V collides with “Paste and Match Style.”

**Proposed lock:**

| Platform | v1 support |
|---|---|
| macOS | First-class. NSPasteboard capture. Accessibility optional for window title; core history works without it. Signed + notarized release. |
| Linux X11 | First-class. |
| Linux Wayland | Best-effort / degraded. Document capability matrix. |
| Windows | Out of v1. |

- Default hotkey: non-colliding (e.g. Cmd+Shift+Period / Ctrl+Shift+Period) and **rebindable**.
- First-run onboarding explains TCC / Accessibility honestly.
- Prove permissions + autostart + tray in a walking skeleton before features.

### 7. Shrink browser integration

**Problem.** Native app cannot reliably read browser tab URLs. Extension IPC (native messaging vs localhost), Chrome-only scope, and copy-event race with OS clipboard are all unspecced.

**Proposed lock:**

- Without extension: store browser *app name* + window title only.
- Chrome extension via **Native Messaging** only (no open localhost HTTP in v1).
- Extension is phase 2 after capture + search work. Not Week-1 / not blocking first OSS tag.

### 8. Replace unfalsifiable categorization target

**Problem.** “>90% accuracy” with no taxonomy, gold set, or metric.

**Proposed lock:**

- v1 categories are deterministic: `url`, `code`, `text`, `image`, `file`.
- Rules + MIME / heuristics. Fixture suite of ~500 items. Measure and publish that number.
- Soft ML labels deferred.

### 9. Add OSS and release scaffolding to the SPEC (not “later”)

Missing today, required to call this production OSS:

- LICENSE (app) + model license + third-party policy (compatible with redistributing Tauri + ORT + BGE).
- SECURITY.md, threat model, coordinated disclosure.
- CONTRIBUTING.md, CODE_OF_CONDUCT.md, architecture overview.
- CI matrix: rustfmt/clippy/test, frontend lint/test, macOS build, Linux X11 build, packaging smoke.
- Packaging: macOS signed/notarized `.dmg` or `.pkg`; Linux AppImage + deb (Flatpak later).
- Signed update channel (Tauri updater or equivalent).
- Privacy policy: what is stored, retention, no default telemetry.
- Platform support table in README.
- Reproducible release checklist (tags, checksums, model checksum if bundled/downloaded).

---

## Spec contradictions to erase in the rewrite

1. `<10MB / <50MB` vs Tauri + BGE.
2. “Zero cloud” vs “DO tunnel” sync.
3. “Rust daemon” vs “Tauri stack” with no IPC/lifecycle story.
4. “Image” category vs text-only BGE embeddings with no image policy.
5. Background embedding vs always-warm semantic latency.
6. “Ships on Linux” without X11/Wayland matrix.
7. Three-week calendar packing of signing, extension, embeddings, and OSS release engineering.

Drop calendar “Week 1/2/3” language from the SPEC. Sequence by dependency, not weeks.

---

## Deferred from v1 (keep out of the first OSS release)

| Item | Why defer |
|---|---|
| Encrypted sync / DO tunnel | Scope + threat model of its own |
| Required semantic search | RAM, packaging, battery; prove FTS + context first |
| Image embeddings / OCR | Size, privacy, complexity |
| Soft ML categories | Unfalsifiable without eval harness |
| Windows | Different clipboard APIs; expand after macOS/Linux X11 |
| Firefox / Safari extensions | After Chrome Native Messaging path is proven |
| Wayland-perfect attribution | Document as degraded |

---

## Recommended v1 scope (rewrite target)

A local-only desktop clipboard manager for **macOS** and **Linux X11** that:

1. Captures text, URLs, images (thumbnail), and file references into an **encrypted** SQLite history.
2. Enriches items with timestamp, app name, window title where permissions allow, deterministic type, content hash.
3. Ships a tray popup with recent items, **keyword + metadata search**, rebindable hotkey, pause capture, per-app exclusions, retention limits, clear/delete.
4. Treats Accessibility / deeper context as optional enhancement, not a hard dependency for core history.
5. Ships signed packages, CI, LICENSE, SECURITY.md, and an honest platform support table.

**Differentiation for v1:** privacy-conscious source context + trustworthy local search, not “we invented semantic clipboard.”

**Optional experimental in v1.0 or v1.1:** local BGE semantic search behind a setting, model unload when idle.

**Phase 2:** Chrome Native Messaging for real tab URL/title; Wayland hardening; sync design.

---

## Minimal architecture to lock in SPEC

```
context-clipboardd (Rust, always-on)
  pasteboard watcher → dedup / caps / rules categorizer
  → SQLite WAL (clips + FTS5 + optional clip_vec)
  → embed queue (model usually unloaded)
  ← Unix socket IPC (single-instance)

Tauri tray UI (spawn on open)
  recent items + FTS search (+ optional semantic when warm)

Chrome Native Messaging host (phase 2)
  URL/title only when copy originated in-browser
```

Out of v1: cloud sync, soft ML labels, Windows, open localhost HTTP bridges.

---

## Build phases (after SPEC rewrite is accepted)

Dependency order, not calendar estimates:

1. **Walking skeleton** — daemon, tray, IPC, SQLite schema/migrations, autostart, hotkey, macOS TCC onboarding stub, Linux X11 capture stub, CI + LICENSE.
2. **Capture + privacy** — full clipboard monitor, caps, dedup, exclusions, pause, retention, encryption-at-rest, delete/clear.
3. **Findability** — FTS5, metadata filters, recent-5 popup, categorization fixtures.
4. **Packaging** — signed macOS, Linux packages, updater, README support table.
5. **Optional semantic** — ORT/Candle + BGE, sqlite-vec, unload policy, golden tests.
6. **Chrome extension** — Native Messaging, correlation with OS clipboard events.

Packaging and permissions land early (phase 1–2), not as a final polish step.

---

## Decisions needed from you before we rewrite SPEC / build

Reply with preferences (defaults in bold if you want us to proceed without debate):

1. **Semantic search in first OSS tag?** experimental-behind-toggle / **defer to v1.1** / required
2. **Process model?** **daemon + on-demand Tauri UI** / single Tauri process
3. **Encryption at rest in v1?** **yes** / later
4. **Linux Wayland?** **best-effort documented** / hard requirement
5. **Default license?** **Apache-2.0** / MIT / GPL-3.0 (affects model/runtime choices)
6. **Brand naming:** keep “context-clipboard” / “Context Clipboard” under LatticeAG?

---

## What we will do next (still planning → then build)

1. You confirm or amend the decisions above.
2. We rewrite `SPEC.md` to match this plan (measurable SLOs, schema sketch, platform matrix, OSS checklist, phased scope).
3. Only then scaffold the Tauri/Rust repo and start phase 1.

No application code until the rewritten SPEC is accepted.
