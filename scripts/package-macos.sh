#!/usr/bin/env bash
# Packaging stub for macOS (SPEC §16 / release checklist).
# Not a full build pipeline — prints the steps a real release job will run.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${VERSION:-$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')}"
OUT="${OUT_DIR:-$ROOT/dist/macos}"
IDENTITY="${CODESIGN_IDENTITY:-\<Developer ID Application\>}"
BUNDLE_ID="com.latticeag.context-clipboard"

echo "==> Context Clipboard macOS packaging stub (v${VERSION})"
echo "    root: $ROOT"
echo "    out:  $OUT"
echo
echo "Would run:"
echo "  1. cargo build --release -p clipboard-daemon -p clipboard-ui --target aarch64-apple-darwin"
echo "     cargo build --release -p clipboard-daemon -p clipboard-ui --target x86_64-apple-darwin"
echo "  2. lipo universal binaries into Context Clipboard.app/Contents/MacOS/"
echo "  3. write Info.plist (CFBundleIdentifier=${BUNDLE_ID}, CFBundleVersion=${VERSION})"
echo "  4. codesign --deep --options runtime --sign \"${IDENTITY}\" Context Clipboard.app"
echo "  5. create DMG (hdiutil / create-dmg) → ContextClipboard-${VERSION}.dmg"
echo "  6. notarize: xcrun notarytool submit … --wait"
echo "  7. staple: xcrun stapler staple ContextClipboard-${VERSION}.dmg"
echo "  8. shasum -a 256 the DMG into SHA256SUMS"
echo
echo "Stub complete — no artifacts written."
