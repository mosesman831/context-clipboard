#!/usr/bin/env bash
# Packaging stub for Linux (SPEC §16 / release checklist).
# Not a full build pipeline — prints the steps a real release job will run.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${VERSION:-$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')}"
OUT="${OUT_DIR:-$ROOT/dist/linux}"

echo "==> Context Clipboard Linux packaging stub (v${VERSION})"
echo "    root: $ROOT"
echo "    out:  $OUT"
echo
echo "Would run:"
echo "  1. cargo build --release -p clipboard-daemon -p clipboard-ui"
echo "  2. stage binaries into AppDir:"
echo "       usr/bin/context-clipboardd"
echo "       usr/bin/context-clipboard (UI, when ready)"
echo "       usr/share/applications/context-clipboard.desktop"
echo "       usr/share/icons/hicolor/*/apps/context-clipboard.png"
echo "  3. build AppImage via appimagetool → context-clipboard-${VERSION}-x86_64.AppImage"
echo "  4. build .deb via cargo-deb or fpm → context-clipboard_${VERSION}_amd64.deb"
echo "  5. sha256sum both artifacts into SHA256SUMS"
echo "  6. (optional) sign with GPG detached signatures"
echo
echo "Stub complete — no artifacts written."
