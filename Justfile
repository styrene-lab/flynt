# Codex — Knowledge & Task Tracker
set shell := ["bash", "-cu"]

default:
    @just --list --unsorted

vault := env_var_or_default("CODEX_VAULT", env_var("HOME") + "/workspace/black-meridian")
version := "0.1.0"

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

# Bundle .app with version info and URL scheme registration
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

# ─── Code Signing (YubiKey via rcodesign) ───────────────────

# Sign the .app bundle with Developer ID Application cert on YubiKey.
# Same flow as Omegon: rcodesign + smartcard slot 9c.
sign: bundle
    #!/usr/bin/env bash
    set -euo pipefail
    APP="dist/Codex.app"

    echo "Signing Codex.app with Apple Developer ID (YubiKey)..."
    if [ -n "${SMARTCARD_PIN:-}" ]; then
        echo "Using SMARTCARD_PIN from environment"
        echo "⚡ Touch YubiKey when it blinks"
        rcodesign sign \
            --smartcard-slot 9c \
            --smartcard-pin-env SMARTCARD_PIN \
            --code-signature-flags runtime \
            "$APP"
    else
        echo "⚡ Enter PIN when prompted, then touch YubiKey when it blinks"
        rcodesign sign \
            --smartcard-slot 9c \
            --code-signature-flags runtime \
            "$APP"
    fi

    echo ""
    echo "Verifying signature..."
    codesign -dvvv "$APP" 2>&1 | grep -E "Authority|Team|Signature|Identifier"
    echo "✓ Signed"

# ─── Notarization ──────────────────────────────────────────

# Notarize the signed .app for direct distribution.
# Requires a keychain profile: just setup-notarize
notarize: sign
    #!/usr/bin/env bash
    set -euo pipefail
    APP="dist/Codex.app"
    ZIP="dist/Codex-{{version}}.zip"

    ditto -c -k --keepParent "$APP" "$ZIP"
    echo "Submitting for Apple notarization..."

    if xcrun notarytool history --keychain-profile "codex" >/dev/null 2>&1; then
        xcrun notarytool submit "$ZIP" --keychain-profile "codex" --wait
        xcrun stapler staple "$APP"
        echo "✓ Notarized and stapled"
    else
        echo "✗ No keychain profile 'codex'. Run: just setup-notarize"
        exit 1
    fi

# One-time: store notarization credentials in the keychain.
setup-notarize:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Storing Apple notarization credentials..."
    echo "You'll need your Apple ID, team ID, and an app-specific password."
    echo "(Generate at https://appleid.apple.com/account/manage → App-Specific Passwords)"
    echo ""
    xcrun notarytool store-credentials "codex" \
        --apple-id "" \
        --team-id "UZBY9DM42N"
    echo "✓ Credentials stored as keychain profile 'codex'"

# ─── Distribution ───────────────────────────────────────────

# Create a distributable .dmg from the signed .app
dmg: sign
    #!/usr/bin/env bash
    set -euo pipefail
    DMG="dist/Codex-{{version}}.dmg"
    rm -f "$DMG"
    hdiutil create -volname "Codex" -srcfolder "dist/Codex.app" \
        -ov -format UDZO "$DMG"
    echo "✓ Created $DMG"

# Full release: bundle → sign → notarize → dmg
release: notarize dmg
    @echo "✓ Codex {{version}} ready for distribution"
    @echo "  dist/Codex.app          (signed + notarized)"
    @echo "  dist/Codex-{{version}}.dmg  (distributable)"

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
