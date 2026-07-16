# context-clipboard

> **Status:** vision draft. Production / OSS readiness plan lives in [`PLAN.md`](./PLAN.md). Do not implement against this file until the plan decisions are locked and this SPEC is rewritten.

> **LatticeAG · Desktop** · Context-aware clipboard manager

A desktop app that enriches every clipboard item with source context (app, project, URL, time), indexes everything locally with semantic search, and auto-categorizes into code/URL/text/image.

## Why

Clipboard managers exist (CopyQ, Pastebot, Ditto) but none do context enrichment or semantic search. You copy a code snippet from VS Code, a URL from Chrome, a paragraph from a doc — and hours later you can't find it. Context Clipboard remembers not just *what* you copied but *where* it came from.

## SPEC

### Architecture

```
Clipboard monitor (Rust daemon)
  ↓ on every copy
Context enricher (reads active window title, app name, browser URL)
  ↓
SQLite store (item text, source context, embedding vector)
  ↓
Search UI (Tauri system tray popup)
```

### Key Decisions

- **Local-first, zero cloud.** SQLite + local embeddings (BGE-small via llama.cpp)
- **Privacy:** all data stays on device. Optional encrypted sync via DO tunnel.
- **Embedding:** BGE-small-en (384 dims, <100MB RAM). Generated in background.
- **Browser integration:** Chrome extension reads URL + page title for copies made in browser.
- **Stack:** Tauri (Rust backend, webview frontend) for cross-platform.

### Features (v1)

- [ ] System tray icon with popup search
- [ ] Source enrichment: app name, window title, URL (from browser)
- [ ] Semantic search across clipboard history
- [ ] Auto-categorization: code, URL, text, image
- [ ] 5 most recent items in quick-access popup
- [ ] Keyboard shortcut to open (Cmd+Shift+V / Ctrl+Shift+V)

### Success Criteria

- <10MB RAM idle, <50MB during search
- Semantic search returns relevant results within 200ms
- Auto-categorization accuracy >90%
- Ships on macOS and Linux in v1

### Build Priority

```
Week 1: Clipboard monitor + SQLite store + basic system tray
Week 2: Context enrichment + semantic search + auto-categorization
Week 3: Chrome extension + polish + packaging
```
