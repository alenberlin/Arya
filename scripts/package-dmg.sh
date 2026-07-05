#!/usr/bin/env bash
#
# Builds a distributable Arya.app + DMG for macOS (Apple Silicon).
#
# Two things Tauri's default `tauri build` gets wrong for this app, both fixed here:
#
#   1. sherpa-rs (speaker diarization) links libonnxruntime + libsherpa-onnx-c-api
#      dynamically via @rpath, but Tauri doesn't copy them into the bundle — so the
#      installed app crashes at launch with "Library not loaded: @rpath/libonnxruntime…".
#      We copy every @rpath dylib into Contents/Frameworks and add the matching rpath.
#
#   2. Without a paid Apple Developer ID, Tauri only linker-ad-hoc-signs and drops the
#      configured entitlements. We ad-hoc re-sign with the real bundle identifier and
#      entitlements so macOS attributes TCC permissions (Input Monitoring, Accessibility,
#      Microphone) to the app and remembers the grants.
#
# The result is ad-hoc signed (not notarized): first launch needs a one-time
# right-click → Open, and there are no auto-updates. Both require a Developer ID.
#
# Usage: scripts/package-dmg.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

APP_ID="dev.arya.app"
ENT="$ROOT/src-tauri/entitlements.plist"
REL="$ROOT/src-tauri/target/release"
APP="$REL/bundle/macos/Arya.app"
DMG_DIR="$REL/bundle/dmg"

echo "==> Building release bundle (tauri build)…"
pnpm tauri build

echo "==> Bundling @rpath dylibs into the app…"
mkdir -p "$APP/Contents/Frameworks"
for dep in $(otool -L "$REL/arya" | awk '/@rpath\/.*\.dylib/ {print $1}' | sed 's|@rpath/||' | sort -u); do
  if [ -f "$REL/$dep" ]; then
    cp -f "$REL/$dep" "$APP/Contents/Frameworks/"
    echo "    + $dep"
  fi
done
# Point the executable at the bundled Frameworks (harmless if already present).
install_name_tool -add_rpath @executable_path/../Frameworks "$APP/Contents/MacOS/arya" 2>/dev/null || true

# Prefer the stable self-signed identity (scripts/create-signing-cert.sh) so TCC
# grants survive rebuilds; fall back to ad-hoc if it isn't installed.
SIGN_CN="Arya Local Signing"
if security find-certificate -c "$SIGN_CN" >/dev/null 2>&1; then
  SIGN_ID="$SIGN_CN"
  echo "==> Signing ($APP_ID) with stable identity '$SIGN_CN' — TCC grants persist across rebuilds…"
else
  SIGN_ID="-"
  echo "==> Ad-hoc signing ($APP_ID) — run scripts/create-signing-cert.sh to stop TCC re-grants each build…"
fi
xattr -cr "$APP"
codesign --force --deep --sign "$SIGN_ID" --identifier "$APP_ID" --entitlements "$ENT" "$APP"
codesign --verify --deep --strict "$APP"

echo "==> Repackaging DMG…"
VER="$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$APP/Contents/Info.plist")"
DMG="$DMG_DIR/Arya_${VER}_aarch64.dmg"
STAGE="$(mktemp -d)/dmg"; mkdir -p "$STAGE"
ditto "$APP" "$STAGE/Arya.app"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -volname "Arya $VER" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$(dirname "$STAGE")"

echo "==> Done. Installable DMG at:"
echo "    $DMG"
