# Context Clipboard

Local-first clipboard manager that stores what you copied and where it came from (app, window title, time). History stays on your machine, encrypted at rest. No accounts, no cloud sync, no telemetry.

**Status:** early development. Behavior and scope are defined in [SPEC.md](./SPEC.md). Expect incomplete builds and breaking changes until v1.

## Architecture

Always-on Rust daemon (`context-clipboardd`) captures and stores history; an on-demand Tauri tray UI talks to it over local IPC.

## Platform support

| Platform | Support |
|---|---|
| macOS | First-class |
| Linux X11 | First-class |
| Linux Wayland | Best-effort |
| Windows | Not supported |

## Build

Prerequisites:

- Rust 1.83+
- On Linux, X11 capture deps will be documented as watchers land (arboard / x11 stack). SQLite is bundled via rusqlite.

```bash
cargo build -p clipboard-daemon
cargo build -p clipboard-core
```

The tray UI crate (`clipboard-ui`) is a stub until Tauri wiring lands. Workspace default members are core + daemon so headless CI does not need a display.

## Quick start

Once the daemon CLI is wired:

```bash
context-clipboardd
context-clipboard status
context-clipboard recent
context-clipboard search "query"
```

## Competitors

Other clipboard tools already do history and search well. CopyQ is open and scriptable. Alfred and Raycast fold history into a launcher. Pastebot is a polished macOS app. System clipboard history exists on macOS and Windows.

Context Clipboard is aiming at local-only defaults, encrypted storage, per-app exclusions and retention, and source metadata as a first-class field, with an explicit threat model in [SECURITY.md](./SECURITY.md).

## Privacy

- Local-only by default. No telemetry.
- Encryption at rest with an OS keychain-backed key (v1).
- Secret heuristics, app denylist, pause capture, and retention limits (see SPEC §5).
- Logs must never include clipboard contents.

## Docs

- [SPEC.md](./SPEC.md) — product contract
- [SECURITY.md](./SECURITY.md) — threat model and disclosure
- [CONTRIBUTING.md](./CONTRIBUTING.md)
- [LICENSE](./LICENSE) — Apache-2.0
