# clipboard-ui

`clipboard-ui` builds the `context-clipboard` binary for Context Clipboard.

The production UI is intended to be a Tauri tray app:

- a tray icon with a paused state
- a popup showing the five most recent clips
- search over clip text and source metadata
- delete, clear, favorite, paste, and settings flows
- first-run copy for permissions and privacy controls

This phase-1 crate is a practical shell for headless CI and early daemon work. It ships:

- a typed IPC client for the daemon Unix socket
- a `clap` CLI that exercises the daemon protocol
- an interactive terminal prompt for manual testing
- a small `frontend/` stub for the future Tauri webview

No HTTP server is started by this crate. The `serve-ui` command prints the path to the static development stub only. Browser extension work must use Native Messaging later, not an open localhost bridge.

## Running against the daemon

Start the daemon separately, then run:

```sh
cargo run -p clipboard-ui --bin context-clipboard -- status
cargo run -p clipboard-ui --bin context-clipboard -- recent
cargo run -p clipboard-ui --bin context-clipboard -- search "invoice"
cargo run -p clipboard-ui --bin context-clipboard -- get <clip-id>
cargo run -p clipboard-ui --bin context-clipboard -- delete <clip-id>
cargo run -p clipboard-ui --bin context-clipboard -- clear
cargo run -p clipboard-ui --bin context-clipboard -- pause
cargo run -p clipboard-ui --bin context-clipboard -- resume
```

Use `--json` to print the raw daemon response:

```sh
cargo run -p clipboard-ui --bin context-clipboard -- --json recent
```

By default the client connects to:

1. `$CONTEXT_CLIPBOARD_SOCKET`, when set
2. `$XDG_RUNTIME_DIR/context-clipboard/context-clipboard.sock`
3. `/tmp/context-clipboard-$UID.sock` as a development fallback

Override it explicitly with:

```sh
cargo run -p clipboard-ui --bin context-clipboard -- --socket /path/to/socket status
```

Run without a subcommand, or use `interactive`, to start the prompt:

```sh
cargo run -p clipboard-ui --bin context-clipboard
```

## Deferred Tauri work

The next UI step is to embed `frontend/` in a Tauri tray application and replace the terminal shell with Tauri commands that call the same IPC client. The webview must keep rendering clipboard data as text, use a strict local CSP, and avoid broad Tauri API allowlists.
