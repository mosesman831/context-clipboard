# Context Clipboard

Local-first clipboard manager that stores what you copied and where it came from (app, window title, time). History stays on your machine, encrypted at rest. No accounts, no cloud sync, no telemetry.

**Status:** early development (phases 0–2 in progress). Contract: [SPEC.md](./SPEC.md).

## Architecture

Always-on Rust daemon (`context-clipboardd`) captures and indexes history in SQLite (FTS5, AES-GCM). The `context-clipboard` CLI talks to it over a Unix socket. A Tauri tray popup is planned; the UI crate currently ships the IPC client + CLI.

```
context-clipboardd  →  SQLite (encrypted) + FTS5
        ↑ Unix socket IPC
context-clipboard   (status | recent | search | …)
```

## Platform support

| Platform | Support |
|---|---|
| macOS | First-class (signing/notarization still TODO) |
| Linux X11 | First-class |
| Linux Wayland | Best-effort |
| Windows | Not supported |

## Build

Prerequisites: Rust stable (1.83+ intended; CI uses current stable).

```bash
cargo build --workspace
cargo test --workspace
```

Binaries:

- `context-clipboardd` — daemon
- `context-clipboard` — CLI / IPC client

## Quick start

```bash
# terminal 1
cargo run -p clipboard-daemon -- --data-dir /tmp/cc-demo

# terminal 2 (same machine; socket under XDG_RUNTIME_DIR or data-dir)
cargo run -p clipboard-ui --bin context-clipboard -- --socket /tmp/cc-demo/context-clipboardd.sock status
cargo run -p clipboard-ui --bin context-clipboard -- --socket /tmp/cc-demo/context-clipboardd.sock recent
cargo run -p clipboard-ui --bin context-clipboard -- --socket /tmp/cc-demo/context-clipboardd.sock search "query"
```

Copy text in another app while the daemon runs; it polls the clipboard, encrypts, categorizes, and indexes. Pause with `pause` / resume with `resume`.

## Competitors

CopyQ is open and scriptable. Alfred and Raycast fold history into a launcher. Pastebot is a polished macOS app. System clipboard history exists on macOS and Windows.

This project aims at local-only defaults, encrypted storage, per-app exclusions and retention, and source metadata as a first-class field. Threat model: [SECURITY.md](./SECURITY.md).

## Privacy

- Local-only. No telemetry.
- Encryption at rest (dev key file today; OS keychain planned per SPEC).
- Secret heuristics, app denylist, pause capture, retention limits.
- Logs never include clipboard contents.

## Docs

- [SPEC.md](./SPEC.md) — product contract
- [PLAN.md](./PLAN.md) — locked readiness decisions
- [SECURITY.md](./SECURITY.md)
- [CONTRIBUTING.md](./CONTRIBUTING.md)
- [LICENSE](./LICENSE) — Apache-2.0
