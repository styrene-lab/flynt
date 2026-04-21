#!/bin/bash
set -e

VERSION="${1:-0.1.0}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$ROOT/dist"
mkdir -p "$DIST"

echo "=== Building Codex v$VERSION ==="

# ── macOS ────────────────────────────────────────────────────────────────────
echo "--- macOS desktop ---"
cd "$ROOT/crates/codex-app"
dx build --platform desktop --release
cd "$ROOT"

APP="target/dx/codex-app/release/macos/CodexApp.app"
cp crates/codex-app/assets/icon.icns "$APP/Contents/Resources/AppIcon.icns"

# Excalidraw bundle — lazy-loaded at runtime, not picked up by asset!() macro
mkdir -p "$APP/Contents/Resources/assets/vendor"
cp crates/codex-app/assets/vendor/excalidraw.bundle.js "$APP/Contents/Resources/assets/vendor/"
/usr/libexec/PlistBuddy -c "Set :CFBundleIconFile AppIcon" "$APP/Contents/Info.plist" 2>/dev/null || \
  /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string AppIcon" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP/Contents/Info.plist" 2>/dev/null

SIGN_ID="Developer ID Application: CHRISTOPHER RYAN WILSON (UZBY9DM42N)"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
ENTITLEMENTS="$ROOT/crates/codex-app/Codex.entitlements"

# Sign all nested binaries first (dylibs, frameworks)
find "$APP/Contents" -type f \( -name "*.dylib" -o -perm +111 \) ! -name "Info.plist" ! -name "*.plist" | while read -r bin; do
  codesign -f -s "$SIGN_ID" --keychain "$KEYCHAIN" --options runtime --timestamp "$bin" 2>/dev/null || true
done

# Sign the main app bundle with hardened runtime + entitlements
codesign -f -s "$SIGN_ID" \
  --keychain "$KEYCHAIN" \
  --options runtime \
  --timestamp \
  --entitlements "$ENTITLEMENTS" \
  "$APP"

# DMG
STAGING=$(mktemp -d)
cp -r "$APP" "$STAGING/Codex.app"
ln -s /Applications "$STAGING/Applications"
hdiutil create -volname "Codex" -srcfolder "$STAGING" -ov -format UDZO "$DIST/Codex-$VERSION.dmg"
codesign -f -s "$SIGN_ID" \
  --keychain "$KEYCHAIN" \
  --timestamp \
  "$DIST/Codex-$VERSION.dmg"
rm -rf "$STAGING"
echo "✓ macOS DMG: $DIST/Codex-$VERSION.dmg"

# ── iOS ──────────────────────────────────────────────────────────────────────
echo "--- iOS ---"
cd "$ROOT/crates/codex-mobile"
IPHONEOS_DEPLOYMENT_TARGET=17.0 dx build --platform ios --device --release
cd "$ROOT"

MAPP="target/dx/codex-mobile/release/ios/CodexMobile.app"

# Icons
cp "crates/codex-mobile/assets/AppIcon.appiconset/icon-120.png" "$MAPP/AppIcon60x60@2x.png"
cp "crates/codex-mobile/assets/AppIcon.appiconset/icon-180.png" "$MAPP/AppIcon60x60@3x.png"
cp "crates/codex-mobile/assets/AppIcon.appiconset/icon-80.png"  "$MAPP/AppIcon40x40@2x.png"
cp "crates/codex-mobile/assets/AppIcon.appiconset/icon-120.png" "$MAPP/AppIcon40x40@3x.png"
cp "crates/codex-mobile/assets/AppIcon.appiconset/icon-58.png"  "$MAPP/AppIcon29x29@2x.png"
cp "crates/codex-mobile/assets/AppIcon.appiconset/icon-87.png"  "$MAPP/AppIcon29x29@3x.png"
cp "crates/codex-mobile/assets/AppIcon.appiconset/icon-40.png"  "$MAPP/AppIcon20x20@2x.png"
cp "crates/codex-mobile/assets/AppIcon.appiconset/icon-60.png"  "$MAPP/AppIcon20x20@3x.png"

# Plist
/usr/libexec/PlistBuddy -c "Add :CFBundleIcons dict" "$MAPP/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleIcons:CFBundlePrimaryIcon dict" "$MAPP/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleIcons:CFBundlePrimaryIcon:CFBundleIconFiles array" "$MAPP/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleIcons:CFBundlePrimaryIcon:CFBundleIconFiles:0 string AppIcon60x60" "$MAPP/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleIcons:CFBundlePrimaryIcon:CFBundleIconFiles:1 string AppIcon40x40" "$MAPP/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleIcons:CFBundlePrimaryIcon:CFBundleIconFiles:2 string AppIcon29x29" "$MAPP/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleIcons:CFBundlePrimaryIcon:CFBundleIconFiles:3 string AppIcon20x20" "$MAPP/Info.plist" 2>/dev/null || true

codesign -f -s "Apple Development: CHRISTOPHER RYAN WILSON (Q4FM48AWU9)" \
  --entitlements /tmp/entitlements.plist "$MAPP"

# IPA
IPA_STAGING=$(mktemp -d)
mkdir -p "$IPA_STAGING/Payload"
cp -r "$MAPP" "$IPA_STAGING/Payload/CodexMobile.app"
cd "$IPA_STAGING" && zip -r -q "$DIST/Codex-$VERSION.ipa" Payload/
cd "$ROOT" && rm -rf "$IPA_STAGING"
echo "✓ iOS IPA: $DIST/Codex-$VERSION.ipa"

echo ""
echo "=== Codex v$VERSION ==="
ls -lh "$DIST"/Codex-"$VERSION".*
