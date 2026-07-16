# Context Clipboard — readiness plan (decisions locked)

**Status:** complete. Recommendations accepted and encoded in [`SPEC.md`](./SPEC.md).

This file is the audit trail of what was wrong with the original vision draft and what we locked. Implement against **SPEC.md**, not this plan.

---

## Original verdict

The vision (local clipboard + source context + findability) was sound. The draft SPEC was not shippable: impossible RAM targets, privacy underspecified, daemon vs Tauri undefined, “zero cloud” vs sync contradiction, no OSS/release scaffolding.

---

## Locked choices (all taken)

| Topic | Locked as |
|---|---|
| Semantic search | Deferred to **v1.1**; FTS5 required in v1.0 |
| Process model | **Daemon + on-demand Tauri UI** over Unix socket IPC |
| Encryption at rest | **Yes in v1** (AES-256-GCM, OS keychain key) |
| Linux Wayland | **Best-effort**, documented matrix |
| License | **Apache-2.0** |
| Brand | **context-clipboard** / Context Clipboard · LatticeAG |
| Sync / DO tunnel | **Out of v1** |
| Embeddings (when added) | **ONNX + BGE-small**, not llama.cpp |
| Browser URLs | **Phase 2** Native Messaging |
| Hotkey | **Cmd/Ctrl+Shift+.** rebindable |
| Categorization | Deterministic rules + ≥90% on fixtures |
| Memory SLOs | Split by process (daemon ≤25MB idle; UI ≤80MB open) |

---

## What changed vs the first SPEC

1. Dropped `<10MB / <50MB` fantasy budgets.
2. Replaced “zero cloud + DO tunnel” with local-only v1 and sync explicitly deferred.
3. Defined process split, schema, IPC, crypto, retention, exclusions.
4. Platform matrix with Wayland honesty and macOS Accessibility optional.
5. Semantic search and Chrome extension moved out of v1.0.
6. Added OSS/release checklist and dependency-ordered phases (no fake calendar weeks).
7. Measurable success criteria tied to fixtures and RSS.

---

## Build order

Follow SPEC §11: Phase 0 bootstrap → walking skeleton → capture/privacy → findability → packaging/OSS release → then v1.1 semantic → Chrome extension.

---

## Historical note

Three parallel reviews (architecture, OSS product, tech feasibility) informed these locks. Their detailed findings are superseded by SPEC.md; re-litigate only if implementation evidence breaks a success criterion.
