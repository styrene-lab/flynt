#!/bin/bash
set -e

VERSION="${1:-0.1.0}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$ROOT/dist"
mkdir -p "$DIST"

echo "=== Building Flynt v$VERSION ==="

# ── macOS ────────────────────────────────────────────────────────────────────
echo "--- macOS desktop ---"
cd "$ROOT/crates/flynt-app"
dx build --platform desktop --release
cd "$ROOT"

APP="target/dx/flynt-app/release/macos/FlyntApp.app"
# Copy icon as both names — Dioxus sets CFBundleIconFile to "icon.icns"
cp crates/flynt-app/assets/icon.icns "$APP/Contents/Resources/icon.icns"
cp crates/flynt-app/assets/icon.icns "$APP/Contents/Resources/AppIcon.icns"

# Vendor assets — keep unhashed fallbacks available in signed app bundles.
mkdir -p "$APP/Contents/Resources/assets/vendor"
cp crates/flynt-app/assets/vendor/* "$APP/Contents/Resources/assets/vendor/"
/usr/libexec/PlistBuddy -c "Set :CFBundleIconFile AppIcon" "$APP/Contents/Info.plist" 2>/dev/null || \
  /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string AppIcon" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP/Contents/Info.plist" 2>/dev/null
/usr/libexec/PlistBuddy -c "Set :CFBundleName Flynt" "$APP/Contents/Info.plist" 2>/dev/null
/usr/libexec/PlistBuddy -c "Add :CFBundleDisplayName string Flynt" "$APP/Contents/Info.plist" 2>/dev/null || \
  /usr/libexec/PlistBuddy -c "Set :CFBundleDisplayName Flynt" "$APP/Contents/Info.plist"

SIGN_ID="Developer ID Application: CHRISTOPHER RYAN WILSON (UZBY9DM42N)"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
ENTITLEMENTS="$ROOT/crates/flynt-app/Codex.entitlements"

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

# DMG — styled installer with background + icon layout
# Stage the app as "Flynt.app" (Dioxus outputs "FlyntApp.app")
DMG_STAGING=$(mktemp -d)
cp -R "$APP" "$DMG_STAGING/Flynt.app"
codesign -f -s "$SIGN_ID" --keychain "$KEYCHAIN" --options runtime --timestamp --entitlements "$ENTITLEMENTS" "$DMG_STAGING/Flynt.app"

rm -f "$DIST/Flynt-$VERSION.dmg"
if command -v create-dmg &>/dev/null; then
  create-dmg \
    --volname "Flynt" \
    --volicon "crates/flynt-app/assets/icon.icns" \
    --background "$ROOT/scripts/dmg-assets/background@2x.png" \
    --window-pos 200 100 \
    --window-size 1024 700 \
    --icon-size 128 \
    --icon "Flynt.app" 260 340 \
    --hide-extension "Flynt.app" \
    --app-drop-link 760 340 \
    --text-size 14 \
    "$DIST/Flynt-$VERSION.dmg" \
    "$DMG_STAGING/Flynt.app" || true
  codesign -f -s "$SIGN_ID" --keychain "$KEYCHAIN" --timestamp "$DIST/Flynt-$VERSION.dmg"
else
  echo "  (create-dmg not found, using plain hdiutil)"
  ln -s /Applications "$DMG_STAGING/Applications"
  hdiutil create -volname "Flynt" -srcfolder "$DMG_STAGING" -ov -format UDZO "$DIST/Flynt-$VERSION.dmg"
  codesign -f -s "$SIGN_ID" --keychain "$KEYCHAIN" --timestamp "$DIST/Flynt-$VERSION.dmg"
fi
rm -rf "$DMG_STAGING"
echo "✓ macOS DMG: $DIST/Flynt-$VERSION.dmg"

# ── iOS ──────────────────────────────────────────────────────────────────────
echo "--- iOS ---"
cd "$ROOT/crates/flynt-mobile"
IPHONEOS_DEPLOYMENT_TARGET=17.0 dx build --platform ios --device --release
cd "$ROOT"

MAPP="target/dx/flynt-mobile/release/ios/FlyntMobile.app"

# Icons
cp "crates/flynt-mobile/assets/AppIcon.appiconset/icon-120.png" "$MAPP/AppIcon60x60@2x.png"
cp "crates/flynt-mobile/assets/AppIcon.appiconset/icon-180.png" "$MAPP/AppIcon60x60@3x.png"
cp "crates/flynt-mobile/assets/AppIcon.appiconset/icon-80.png"  "$MAPP/AppIcon40x40@2x.png"
cp "crates/flynt-mobile/assets/AppIcon.appiconset/icon-120.png" "$MAPP/AppIcon40x40@3x.png"
cp "crates/flynt-mobile/assets/AppIcon.appiconset/icon-58.png"  "$MAPP/AppIcon29x29@2x.png"
cp "crates/flynt-mobile/assets/AppIcon.appiconset/icon-87.png"  "$MAPP/AppIcon29x29@3x.png"
cp "crates/flynt-mobile/assets/AppIcon.appiconset/icon-40.png"  "$MAPP/AppIcon20x20@2x.png"
cp "crates/flynt-mobile/assets/AppIcon.appiconset/icon-60.png"  "$MAPP/AppIcon20x20@3x.png"

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
cp -r "$MAPP" "$IPA_STAGING/Payload/FlyntMobile.app"
cd "$IPA_STAGING" && zip -r -q "$DIST/Flynt-$VERSION.ipa" Payload/
cd "$ROOT" && rm -rf "$IPA_STAGING"
echo "✓ iOS IPA: $DIST/Flynt-$VERSION.ipa"

# ── Linux (cross-compile or native) ─────────────────────────────────────────
if [[ "$(uname)" == "Linux" ]]; then
  echo "--- Linux desktop ---"
  cd "$ROOT/crates/flynt-app"
  dx build --platform desktop --release
  cd "$ROOT"

  LINUX_BIN="target/dx/flynt-app/release/linux/flynt-app"
  if [ -f "$LINUX_BIN" ]; then
    # Create tarball with binary + icon + .desktop file
    LINUX_STAGING=$(mktemp -d)
    mkdir -p "$LINUX_STAGING/flynt-$VERSION"
    cp "$LINUX_BIN" "$LINUX_STAGING/flynt-$VERSION/flynt"
    cp "crates/flynt-app/assets/icon.png" "$LINUX_STAGING/flynt-$VERSION/flynt.png"
    cat > "$LINUX_STAGING/flynt-$VERSION/flynt.desktop" <<DESK
[Desktop Entry]
Name=Flynt
Comment=Markdown notes, kanban, and knowledge graph
Exec=flynt
Icon=flynt
Type=Application
Categories=Office;TextEditor;
DESK
    cat > "$LINUX_STAGING/flynt-$VERSION/install.sh" <<'INST'
#!/bin/bash
set -e
PREFIX="${1:-$HOME/.local}"
install -Dm755 flynt "$PREFIX/bin/flynt"
install -Dm644 flynt.png "$PREFIX/share/icons/hicolor/512x512/apps/flynt.png"
install -Dm644 flynt.desktop "$PREFIX/share/applications/flynt.desktop"
sed -i "s|Exec=flynt|Exec=$PREFIX/bin/flynt|" "$PREFIX/share/applications/flynt.desktop"
sed -i "s|Icon=flynt|Icon=$PREFIX/share/icons/hicolor/512x512/apps/flynt.png|" "$PREFIX/share/applications/flynt.desktop"
echo "Installed to $PREFIX; run: $PREFIX/bin/flynt"
INST
    chmod +x "$LINUX_STAGING/flynt-$VERSION/install.sh"
    tar -czf "$DIST/Flynt-$VERSION-linux-x86_64.tar.gz" -C "$LINUX_STAGING" "flynt-$VERSION"
    rm -rf "$LINUX_STAGING"
    echo "✓ Linux tarball: $DIST/Flynt-$VERSION-linux-x86_64.tar.gz"
  fi
elif [[ "$(uname)" == "Darwin" ]]; then
  echo "--- Linux (skipped — run on NixOS for native build) ---"
fi

echo ""
echo "=== Flynt v$VERSION ==="
ls -lh "$DIST"/Flynt-"$VERSION".*
