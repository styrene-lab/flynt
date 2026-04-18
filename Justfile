# Codex — Knowledge & Task Tracker
set shell := ["bash", "-cu"]

default:
    @just --list --unsorted

vault := env_var_or_default("CODEX_VAULT", env_var("HOME") + "/workspace/black-meridian")
version := "0.1.0"

# Signing identity — set via env or override on CLI: just sign SIGN_ID="..."
sign_id := env_var_or_default("CODEX_SIGN_ID", "Developer ID Application: Black Meridian, LLC")
installer_id := env_var_or_default("CODEX_INSTALLER_ID", "3rd Party Mac Developer Installer: Black Meridian, LLC")
team_id := env_var_or_default("CODEX_TEAM_ID", "")
apple_id := env_var_or_default("CODEX_APPLE_ID", "")
app_password := env_var_or_default("CODEX_APP_PASSWORD", "")

# ─── Development ────────────────────────────────────────────

run:
    CODEX_VAULT="{{vault}}" cargo run -p codex-app

run-ui:
    CODEX_VAULT="{{vault}}" dx serve --platform desktop

check:
    cargo check

test:
    cargo test

fmt:
    cargo fmt

clippy:
    cargo clippy --all-targets -- -D warnings

validate: fmt check clippy test

# ─── Build & Bundle ─────────────────────────────────────────

build:
    cargo build --release

# Bundle .app with URL scheme registration
bundle:
    #!/usr/bin/env bash
    set -euo pipefail
    dx bundle --platform desktop --release
    PLIST="dist/Codex.app/Contents/Info.plist"

    # Version info
    /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString {{version}}" "$PLIST" 2>/dev/null || \
    /usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string {{version}}" "$PLIST"

    BUILD_NUM=$(date +%Y%m%d%H%M)
    /usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BUILD_NUM" "$PLIST" 2>/dev/null || \
    /usr/libexec/PlistBuddy -c "Add :CFBundleVersion string $BUILD_NUM" "$PLIST"

    # Minimum macOS version
    /usr/libexec/PlistBuddy -c "Set :LSMinimumSystemVersion 13.0" "$PLIST" 2>/dev/null || \
    /usr/libexec/PlistBuddy -c "Add :LSMinimumSystemVersion string 13.0" "$PLIST"

    # codex-note:// URL scheme
    /usr/libexec/PlistBuddy -c "Delete :CFBundleURLTypes" "$PLIST" 2>/dev/null || true
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes array" "$PLIST"
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes:0 dict" "$PLIST"
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes:0:CFBundleURLName string com.black-meridian.codex" "$PLIST"
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes:0:CFBundleURLSchemes array" "$PLIST"
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes:0:CFBundleURLSchemes:0 string codex-note" "$PLIST"

    echo "✓ Bundled dist/Codex.app (v{{version}} build $BUILD_NUM)"

# ─── Code Signing ───────────────────────────────────────────

# Sign the .app bundle with hardened runtime + entitlements
sign: bundle
    #!/usr/bin/env bash
    set -euo pipefail
    ENTITLEMENTS="crates/codex-app/Codex.entitlements"
    APP="dist/Codex.app"

    echo "Signing with: {{sign_id}}"
    codesign --deep --force --options runtime \
        --entitlements "$ENTITLEMENTS" \
        --sign "{{sign_id}}" \
        "$APP"
    codesign --verify --verbose "$APP"
    echo "✓ Signed and verified"

# ─── Notarization (direct distribution) ────────────────────

# Notarize for direct distribution (outside App Store)
notarize: sign
    #!/usr/bin/env bash
    set -euo pipefail
    APP="dist/Codex.app"
    ZIP="dist/Codex-{{version}}.zip"

    ditto -c -k --keepParent "$APP" "$ZIP"
    echo "Submitting for notarization..."
    xcrun notarytool submit "$ZIP" \
        --apple-id "{{apple_id}}" \
        --password "{{app_password}}" \
        --team-id "{{team_id}}" \
        --wait
    xcrun stapler staple "$APP"
    echo "✓ Notarized and stapled"

# ─── TestFlight / App Store ─────────────────────────────────

# Build a signed .pkg for TestFlight upload
testflight: bundle
    #!/usr/bin/env bash
    set -euo pipefail
    APP="dist/Codex.app"
    PKG="dist/Codex-{{version}}.pkg"
    ENTITLEMENTS="crates/codex-app/Codex.entitlements"

    # Sign with Mac App Store identity (3rd Party Mac Developer Application)
    MAS_SIGN="${CODEX_MAS_SIGN_ID:-3rd Party Mac Developer Application: Black Meridian, LLC}"
    echo "Signing for App Store with: $MAS_SIGN"
    codesign --deep --force --options runtime \
        --entitlements "$ENTITLEMENTS" \
        --sign "$MAS_SIGN" \
        "$APP"
    codesign --verify --verbose "$APP"

    # Build installer .pkg
    echo "Building installer package..."
    productbuild --component "$APP" /Applications \
        --sign "{{installer_id}}" \
        "$PKG"

    echo "✓ Built $PKG"
    echo ""
    echo "Upload to App Store Connect:"
    echo "  xcrun altool --upload-app -f $PKG -t macos --apple-id {{apple_id}} --password {{app_password}}"
    echo "  — or use Transporter.app"

# ─── Distribution ───────────────────────────────────────────

# Create a distributable .dmg
dmg: sign
    #!/usr/bin/env bash
    set -euo pipefail
    DMG="dist/Codex-{{version}}.dmg"
    rm -f "$DMG"
    hdiutil create -volname "Codex" -srcfolder "dist/Codex.app" \
        -ov -format UDZO "$DMG"
    codesign --sign "{{sign_id}}" "$DMG"
    echo "✓ Created $DMG"

open:
    CODEX_VAULT="{{vault}}" open dist/Codex.app

dist: bundle open

# Register URL scheme with launch services
register:
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
        -f dist/Codex.app

clean:
    cargo clean
    rm -rf dist
