# Contributing

Thanks for helping. Read [SPEC.md](./SPEC.md) before large changes. Locked decisions in §2 are not bikeshed targets; if a change touches privacy, platforms, or those decisions, update the SPEC in the same PR.

## Setup

- Rust 1.83+ (`rustup` stable is fine)
- Node.js only when the Tauri UI work starts; not required for core/daemon yet
- Linux X11: extra system packages for clipboard capture will land with the watchers; SQLite is bundled

```bash
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test
```

`cargo test` uses workspace default members (`clipboard-core`, `clipboard-daemon`). That keeps headless CI free of a Tauri display. Use `-p clipboard-ui` when working on the tray app.

## Branch naming

Prefer short, scoped names:

- `feat/<topic>`
- `fix/<topic>`
- `docs/<topic>`
- `ci/<topic>`

## Code style

- `rustfmt` with the repo `rustfmt.toml` (edition 2021)
- Clippy clean with `-D warnings` on CI
- Keep diffs focused; do not reformat unrelated files

## SPEC-driven changes

- Implement against SPEC phases and success criteria
- Categorization rules and fixture expectations live under `fixtures/`; CI targets ≥90% accuracy once the categorizer is in place
- Do not add telemetry, cloud sync, or Windows support without a SPEC update

## Privacy when developing

- Never log clipboard contents (including in tests that print fixtures wholesale to CI logs)
- Prefer synthetic fixture text over real clipboard dumps from your machine
- Secret-looking samples in fixtures are intentional for categorizer/heuristic tests; still treat them as fake

## Pull requests

- Describe what SPEC section or phase the change advances
- Include tests when behavior changes
- Link related issues when they exist
