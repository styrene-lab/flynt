# Flynt — Knowledge & Task Tracker
set shell := ["bash", "-cu"]

default:
    @just --list --unsorted

vault := env_var_or_default("FLYNT_VAULT", env_var("HOME") + "/Documents/Flynt")
version := `grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/'`

# ─── Development ────────────────────────────────────────────

run:
    FLYNT_VAULT="{{vault}}" cargo run -p flynt-app

run-ui:
    FLYNT_VAULT="{{vault}}" dx serve --platform desktop

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
    cd crates/flynt-app && dx bundle --platform desktop --release && cd ../..
    # Dioxus outputs to target/dx/; copy to dist/
    rm -rf dist/Flynt.app
    mkdir -p dist
    cp -R target/dx/flynt/release/macos/Flynt.app dist/Flynt.app
    PLIST="dist/Flynt.app/Contents/Info.plist"

    # Version info
    /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString {{version}}" "$PLIST" 2>/dev/null || \
    /usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string {{version}}" "$PLIST"

    BUILD_NUM=$(date +%Y%m%d%H%M)
    /usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BUILD_NUM" "$PLIST" 2>/dev/null || \
    /usr/libexec/PlistBuddy -c "Add :CFBundleVersion string $BUILD_NUM" "$PLIST"

    # Minimum macOS version
    /usr/libexec/PlistBuddy -c "Set :LSMinimumSystemVersion 13.0" "$PLIST" 2>/dev/null || \
    /usr/libexec/PlistBuddy -c "Add :LSMinimumSystemVersion string 13.0" "$PLIST"

    # flynt-note:// URL scheme
    /usr/libexec/PlistBuddy -c "Delete :CFBundleURLTypes" "$PLIST" 2>/dev/null || true
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes array" "$PLIST"
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes:0 dict" "$PLIST"
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes:0:CFBundleURLName string io.styrene.flynt" "$PLIST"
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes:0:CFBundleURLSchemes array" "$PLIST"
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes:0:CFBundleURLSchemes:0 string flynt-note" "$PLIST"

    echo "✓ Bundled dist/Flynt.app (v{{version}} build $BUILD_NUM)"

# ─── Code Signing ───────────────────────────────────────────

# Sign the .app bundle with a Developer ID Application certificate.
sign: bundle
    #!/usr/bin/env bash
    set -euo pipefail
    APP="dist/Flynt.app"

    KEYCHAIN_MATCH=$(security find-identity -v -p codesigning | grep "{{dev_id_app_identity}}" | head -1)
    KEYCHAIN_IDENTITY=$(printf '%s\n' "$KEYCHAIN_MATCH" | awk '{print $2}')
    KEYCHAIN_IDENTITY_NAME=$(printf '%s\n' "$KEYCHAIN_MATCH" | sed 's/.*"\(.*\)"/\1/')
    if [ -n "$KEYCHAIN_IDENTITY" ]; then
        echo "Signing Flynt.app with $KEYCHAIN_IDENTITY_NAME ($KEYCHAIN_IDENTITY)..."
        while IFS= read -r bin; do
            codesign -f -s "$KEYCHAIN_IDENTITY" --options runtime --timestamp "$bin" 2>/dev/null || true
        done < <(find "$APP/Contents/MacOS" -type f)
        codesign --deep --force \
            --sign "$KEYCHAIN_IDENTITY" \
            --options runtime \
            --timestamp \
            "$APP"
    else
        echo "No keychain Developer ID Application identity matching '{{dev_id_app_identity}}'; trying rcodesign smartcard flow..."
        if [ -n "${SMARTCARD_PIN:-}" ]; then
            echo "Using SMARTCARD_PIN from environment"
            echo "Touch YubiKey when it blinks"
            rcodesign sign \
                --smartcard-slot 9c \
                --smartcard-pin-env SMARTCARD_PIN \
                --code-signature-flags runtime \
                "$APP"
        else
            echo "Enter PIN when prompted, then touch YubiKey when it blinks"
            rcodesign sign \
                --smartcard-slot 9c \
                --code-signature-flags runtime \
                "$APP"
        fi
    fi

    echo ""
    echo "Verifying signature..."
    codesign --verify --deep --strict --verbose=2 "$APP"
    codesign -dvvv "$APP" 2>&1 | grep -E "Authority|Team|Signature|Identifier|Timestamp"
    echo "✓ Signed"

# One-time: store notarization credentials in the keychain.
setup-notarize:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Storing Apple notarization credentials..."
    echo "You'll need your Apple ID, team ID, and an app-specific password."
    echo "(Generate at https://appleid.apple.com/account/manage → App-Specific Passwords)"
    echo ""
    xcrun notarytool store-credentials "flynt" \
        --apple-id "" \
        --team-id "UZBY9DM42N"
    echo "✓ Credentials stored as keychain profile 'flynt'"

# ─── Distribution ───────────────────────────────────────────

# Create a direct-download .dmg from the signed .app
dmg: sign
    #!/usr/bin/env bash
    set -euo pipefail
    DMG="dist/Flynt-{{version}}-macos.dmg"
    rm -f "$DMG"
    STAGING=$(mktemp -d)
    cp -R dist/Flynt.app "$STAGING/Flynt.app"
    ln -s /Applications "$STAGING/Applications"
    hdiutil create -volname "Flynt" -srcfolder "$STAGING" -ov -format UDZO "$DMG"
    rm -rf "$STAGING"
    echo "✓ Created $DMG"

# Create a direct-download .pkg installer from the signed .app
pkg: sign
    #!/usr/bin/env bash
    set -euo pipefail
    PKG="dist/Flynt-{{version}}-macos.pkg"
    rm -f "$PKG"
    if ! security find-identity -v | grep -q "{{dev_id_installer_identity}}"; then
        echo "No Developer ID Installer identity matching '{{dev_id_installer_identity}}'; cannot build a distributable PKG."
        echo "Set APPLE_DEVID_INSTALLER_IDENTITY or install a Developer ID Installer certificate."
        exit 1
    fi
    COMPONENT_PLIST=$(mktemp)
    COMPONENT_PKG=$(mktemp -u).pkg
    COMPONENT_ROOT=$(mktemp -d)
    trap 'rm -f "$COMPONENT_PLIST" "$COMPONENT_PKG"; rm -rf "$COMPONENT_ROOT"' EXIT
    cp -R "dist/Flynt.app" "$COMPONENT_ROOT/Flynt.app"
    cat > "$COMPONENT_PLIST" <<'PLIST'
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
    <plist version="1.0">
    <array>
      <dict>
        <key>RootRelativeBundlePath</key>
        <string>Flynt.app</string>
        <key>BundleIsRelocatable</key>
        <false/>
        <key>BundleIsVersionChecked</key>
        <false/>
        <key>BundleHasStrictIdentifier</key>
        <true/>
        <key>BundleOverwriteAction</key>
        <string>upgrade</string>
      </dict>
    </array>
    </plist>
    PLIST
    pkgbuild --root "$COMPONENT_ROOT" \
        --install-location /Applications \
        --identifier io.styrene.flynt \
        --version "{{version}}" \
        --component-plist "$COMPONENT_PLIST" \
        "$COMPONENT_PKG"
    productbuild --package "$COMPONENT_PKG" \
        --sign "{{dev_id_installer_identity}}" \
        "$PKG"
    pkgutil --check-signature "$PKG"
    echo "✓ Created $PKG"

# Notarize and staple the direct-download DMG.
# Requires a keychain profile (`just setup-notarize`) or App Store Connect API key env.
notarize: dmg
    #!/usr/bin/env bash
    set -euo pipefail
    DMG="dist/Flynt-{{version}}-macos.dmg"

    NOTARY_ARGS=()
    TMP_KEY=""
    if xcrun notarytool history --keychain-profile "flynt" >/dev/null 2>&1; then
        NOTARY_ARGS=(--keychain-profile "flynt")
    elif [ -n "${APPLE_API_KEY_P8_B64:-}" ] && [ -n "${APPLE_API_KEY_ID:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ]; then
        TMP_KEY=$(mktemp)
        echo "$APPLE_API_KEY_P8_B64" | base64 --decode > "$TMP_KEY"
        NOTARY_ARGS=(--key "$TMP_KEY" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER")
    elif [ -n "${ASC_KEY_PATH:-}" ] && [ -n "${ASC_KEY_ID:-}" ] && [ -n "${ASC_ISSUER:-}" ]; then
        NOTARY_ARGS=(--key "$ASC_KEY_PATH" --key-id "$ASC_KEY_ID" --issuer "$ASC_ISSUER")
    else
        echo "✗ No notarization credentials found."
        echo "  Run: just setup-notarize"
        echo "  Or set APPLE_API_KEY_P8_B64, APPLE_API_KEY_ID, and APPLE_API_ISSUER."
        exit 1
    fi
    trap 'rm -f "$TMP_KEY"' EXIT

    echo "Submitting $DMG for Apple notarization..."
    xcrun notarytool submit "$DMG" "${NOTARY_ARGS[@]}" --wait
    xcrun stapler staple "$DMG"
    echo "✓ Notarized direct-download DMG"

# Notarize and staple the direct-download PKG.
# Requires a Developer ID Installer cert and keychain profile or App Store Connect API key env.
notarize-pkg: pkg
    #!/usr/bin/env bash
    set -euo pipefail
    PKG="dist/Flynt-{{version}}-macos.pkg"

    NOTARY_ARGS=()
    TMP_KEY=""
    if xcrun notarytool history --keychain-profile "flynt" >/dev/null 2>&1; then
        NOTARY_ARGS=(--keychain-profile "flynt")
    elif [ -n "${APPLE_API_KEY_P8_B64:-}" ] && [ -n "${APPLE_API_KEY_ID:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ]; then
        TMP_KEY=$(mktemp)
        echo "$APPLE_API_KEY_P8_B64" | base64 --decode > "$TMP_KEY"
        NOTARY_ARGS=(--key "$TMP_KEY" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER")
    elif [ -n "${ASC_KEY_PATH:-}" ] && [ -n "${ASC_KEY_ID:-}" ] && [ -n "${ASC_ISSUER:-}" ]; then
        NOTARY_ARGS=(--key "$ASC_KEY_PATH" --key-id "$ASC_KEY_ID" --issuer "$ASC_ISSUER")
    else
        echo "✗ No notarization credentials found."
        echo "  Run: just setup-notarize"
        echo "  Or set APPLE_API_KEY_P8_B64, APPLE_API_KEY_ID, and APPLE_API_ISSUER."
        exit 1
    fi
    trap 'rm -f "$TMP_KEY"' EXIT

    echo "Submitting $PKG for Apple notarization..."
    xcrun notarytool submit "$PKG" "${NOTARY_ARGS[@]}" --wait
    xcrun stapler staple "$PKG"
    echo "✓ Notarized direct-download PKG"

# Full direct macOS release: app → sign → dmg → notarize/staple
release: notarize
    @echo "✓ Flynt {{version}} ready for distribution"
    @echo "  dist/Flynt.app          (signed)"
    @echo "  dist/Flynt-{{version}}-macos.dmg  (signed app, notarized + stapled)"

# Optional direct macOS installer release: app → sign → pkg → notarize/staple
release-pkg: notarize-pkg
    @echo "✓ Flynt {{version}} PKG ready for distribution"
    @echo "  dist/Flynt-{{version}}-macos.pkg  (signed, notarized + stapled)"

open:
    FLYNT_VAULT="{{vault}}" open dist/Flynt.app

dist: bundle open

# Register URL scheme with launch services
register:
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
        -f dist/Flynt.app

# ─── iOS ───────────────────────────────────────────────────

share_ext_src := "crates/flynt-mobile/ios/ShareExtension"
share_ext_bundle_id := "io.styrene.flynt.share-extension"
ios_team := env_var_or_default("APPLE_TEAM_ID", "UZBY9DM42N")
ios_profiles := "crates/flynt-mobile/ios/profiles"
asc_key_id := env_var_or_default("ASC_KEY_ID", "")
asc_issuer := env_var_or_default("ASC_ISSUER", "")
asc_key_path := env_var_or_default("ASC_KEY_PATH", "crates/flynt-mobile/ios/keys/AuthKey.p8")
dist_identity := env_var_or_default("APPLE_DIST_IDENTITY", "Apple Distribution")
installer_identity := env_var_or_default("APPLE_INSTALLER_IDENTITY", "3rd Party Mac Developer Installer")
dev_id_app_identity := env_var_or_default("APPLE_DEVID_APP_IDENTITY", "Developer ID Application")
dev_id_installer_identity := env_var_or_default("APPLE_DEVID_INSTALLER_IDENTITY", "Developer ID Installer")
android_package := "io.styrene.flynt"
android_java_home := env_var_or_default("JAVA_HOME", "/opt/homebrew/opt/openjdk")
android_home := env_var_or_default("ANDROID_HOME", "/opt/homebrew/share/android-commandlinetools")
android_ndk_home := env_var_or_default("ANDROID_NDK_HOME", "/opt/homebrew/share/android-ndk")
android_target := env_var_or_default("ANDROID_TARGET", "aarch64-linux-android")

# Build the iOS Share Extension .appex
build-share-extension:
    #!/usr/bin/env bash
    set -euo pipefail
    SDK="iphoneos"
    SWIFT_TARGET="arm64-apple-ios17.0"
    SDK_PATH=$(xcrun --sdk "$SDK" --show-sdk-path)
    BUILD_DIR="target/share-extension-build"
    APPEX_DIR="$BUILD_DIR/FlyntShare.appex"

    rm -rf "$BUILD_DIR"
    mkdir -p "$APPEX_DIR"

    xcrun --sdk "$SDK" swiftc \
        {{share_ext_src}}/Sources/*.swift \
        -o "$APPEX_DIR/FlyntShare" \
        -target "$SWIFT_TARGET" \
        -module-name FlyntShare \
        -application-extension \
        -Xlinker -e -Xlinker _NSExtensionMain \
        -framework Foundation \
        -framework UIKit \
        -framework SwiftUI \
        -framework UniformTypeIdentifiers \
        -sdk "$SDK_PATH" \
        -O

    cp {{share_ext_src}}/Info.plist "$APPEX_DIR/Info.plist"
    echo "  Built share extension at $APPEX_DIR"

# Inject share extension into a built iOS .app bundle
inject-share-extension app_path:
    #!/usr/bin/env bash
    set -euo pipefail
    PLUGINS="{{app_path}}/PlugIns"
    mkdir -p "$PLUGINS"
    cp -R target/share-extension-build/FlyntShare.appex "$PLUGINS/"
    # Embed provisioning profiles
    cp {{ios_profiles}}/Flynt_Dist.mobileprovision "{{app_path}}/embedded.mobileprovision"
    cp {{ios_profiles}}/Flynt_Share_Dist.mobileprovision "$PLUGINS/FlyntShare.appex/embedded.mobileprovision"
    echo "  Injected ShareExtension + profiles into {{app_path}}"

# Sign iOS app + share extension (inside-out)
sign-ios app_path identity="Apple Development":
    #!/usr/bin/env bash
    set -euo pipefail
    # Sign extension first
    codesign --force \
        --entitlements {{share_ext_src}}/ShareExtension.entitlements \
        --sign "{{identity}}" \
        "{{app_path}}/PlugIns/FlyntShare.appex"
    # Sign main app
    codesign --force \
        --entitlements crates/flynt-mobile/ios/Flynt.entitlements \
        --sign "{{identity}}" \
        "{{app_path}}"
    echo "  Signed app + share extension"

# Full iOS build: dx build -> share extension -> inject -> sign
ios-release:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Building iOS app..."
    cd crates/flynt-mobile && IPHONEOS_DEPLOYMENT_TARGET=17.0 dx build --platform ios --device --release && cd ../..

    APP=$(find target/dx/flynt-mobile -name "*.app" -type d | head -1)
    PLIST="$APP/Info.plist"

    # Patch Info.plist for App Store Connect requirements
    PB=/usr/libexec/PlistBuddy
    $PB -c "Add :CFBundlePackageType string APPL" "$PLIST" 2>/dev/null || \
    $PB -c "Set :CFBundlePackageType APPL" "$PLIST"
    $PB -c "Add :MinimumOSVersion string 17.0" "$PLIST" 2>/dev/null || \
    $PB -c "Set :MinimumOSVersion 17.0" "$PLIST"
    $PB -c "Add :DTPlatformName string iphoneos" "$PLIST" 2>/dev/null || \
    $PB -c "Set :DTPlatformName iphoneos" "$PLIST"
    SDK_VER=$(xcrun --sdk iphoneos --show-sdk-version 2>/dev/null || echo "17.0")
    $PB -c "Add :DTPlatformVersion string $SDK_VER" "$PLIST" 2>/dev/null || \
    $PB -c "Set :DTPlatformVersion $SDK_VER" "$PLIST"
    $PB -c "Add :DTSDKName string iphoneos$SDK_VER" "$PLIST" 2>/dev/null || \
    $PB -c "Set :DTSDKName iphoneos$SDK_VER" "$PLIST"
    XCODE_BUILD=$(defaults read "$(xcode-select -p)/../Info.plist" DTXcodeBuild 2>/dev/null || echo "17E202")
    SDK_BUILD=$(xcrun --sdk iphoneos --show-sdk-build-version 2>/dev/null || echo "23E252")
    $PB -c "Add :DTXcode string $XCODE_BUILD" "$PLIST" 2>/dev/null || \
    $PB -c "Set :DTXcode $XCODE_BUILD" "$PLIST"
    $PB -c "Add :DTXcodeBuild string $XCODE_BUILD" "$PLIST" 2>/dev/null || \
    $PB -c "Set :DTXcodeBuild $XCODE_BUILD" "$PLIST"
    $PB -c "Add :DTSDKBuild string $SDK_BUILD" "$PLIST" 2>/dev/null || \
    $PB -c "Set :DTSDKBuild $SDK_BUILD" "$PLIST"
    $PB -c "Add :DTCompiler string com.apple.compilers.llvm.clang.1_0" "$PLIST" 2>/dev/null || \
    $PB -c "Set :DTCompiler com.apple.compilers.llvm.clang.1_0" "$PLIST"
    $PB -c "Set :CFBundleDisplayName Flynt" "$PLIST"
    # CFBundleSupportedPlatforms must be single value
    $PB -c "Delete :CFBundleSupportedPlatforms" "$PLIST" 2>/dev/null || true
    $PB -c "Add :CFBundleSupportedPlatforms array" "$PLIST"
    $PB -c "Add :CFBundleSupportedPlatforms:0 string iPhoneOS" "$PLIST"
    # Icon asset catalog reference
    $PB -c "Add :CFBundleIconName string AppIcon" "$PLIST" 2>/dev/null || \
    $PB -c "Set :CFBundleIconName AppIcon" "$PLIST"
    # Launch screen (UILaunchScreen dict for iOS 14+, replaces storyboard requirement)
    $PB -c "Add :UILaunchScreen dict" "$PLIST" 2>/dev/null || true
    echo "  Patched Info.plist"

    # Compile asset catalog and merge actool's generated icon plist keys
    # Apple's validation requires the exact CFBundleIcons structure that actool produces —
    # hand-crafted PlistBuddy entries are subtly different and get rejected.
    PARTIAL_PLIST=$(mktemp)
    xcrun actool "crates/flynt-mobile/assets/Assets.xcassets" \
        --compile "$APP" \
        --platform iphoneos \
        --minimum-deployment-target 17.0 \
        --app-icon AppIcon \
        --target-device iphone \
        --target-device ipad \
        --compress-pngs \
        --output-format human-readable-text \
        --output-partial-info-plist "$PARTIAL_PLIST"
    # Merge actool's partial plist (CFBundleIcons, CFBundlePrimaryIcon, CFBundleIconName)
    plutil -convert xml1 "$PLIST"
    $PB -c "Merge $PARTIAL_PLIST" "$PLIST"
    rm -f "$PARTIAL_PLIST"
    # Convert to binary plist (standard for iOS bundles)
    plutil -convert binary1 "$PLIST"

    echo "Building share extension..."
    just build-share-extension
    echo "Injecting into $APP..."
    just inject-share-extension "$APP"
    just sign-ios "$APP" "{{dist_identity}}"
    echo "iOS bundle with Share Extension ready at $APP"

# Package .app into .ipa via xcarchive + xcodebuild export
ios-ipa: ios-release
    #!/usr/bin/env bash
    set -euo pipefail
    APP=$(find target/dx/flynt-mobile -name "*.app" -type d | head -1)
    APP_NAME=$(basename "$APP")
    ARCHIVE="target/Flynt.xcarchive"
    rm -rf "$ARCHIVE"
    mkdir -p "$ARCHIVE/Products/Applications"
    cp -R "$APP" "$ARCHIVE/Products/Applications/"
    /usr/libexec/PlistBuddy -c "Add :ArchiveVersion integer 2" "$ARCHIVE/Info.plist"
    /usr/libexec/PlistBuddy -c "Add :Name string Flynt" "$ARCHIVE/Info.plist"
    /usr/libexec/PlistBuddy -c "Add :SchemeName string Flynt" "$ARCHIVE/Info.plist"
    /usr/libexec/PlistBuddy -c "Add :ApplicationProperties dict" "$ARCHIVE/Info.plist"
    /usr/libexec/PlistBuddy -c "Add :ApplicationProperties:ApplicationPath string Applications/$APP_NAME" "$ARCHIVE/Info.plist"
    /usr/libexec/PlistBuddy -c "Add :ApplicationProperties:CFBundleIdentifier string io.styrene.flynt" "$ARCHIVE/Info.plist"
    /usr/libexec/PlistBuddy -c "Add :ApplicationProperties:CFBundleShortVersionString string 0.4.0" "$ARCHIVE/Info.plist"
    /usr/libexec/PlistBuddy -c "Add :ApplicationProperties:CFBundleVersion string 0.4.0" "$ARCHIVE/Info.plist"
    /usr/libexec/PlistBuddy -c "Add :ApplicationProperties:SigningIdentity string {{dist_identity}}" "$ARCHIVE/Info.plist"
    /usr/libexec/PlistBuddy -c "Add :ApplicationProperties:Team string UZBY9DM42N" "$ARCHIVE/Info.plist"
    IPA_DIR="target/ipa"
    rm -rf "$IPA_DIR"
    xcodebuild -exportArchive \
        -archivePath "$ARCHIVE" \
        -exportPath "$IPA_DIR" \
        -exportOptionsPlist crates/flynt-mobile/ios/ExportOptions.plist
    echo "  Exported IPA to $IPA_DIR/"

# Upload IPA to TestFlight
ios-testflight: ios-ipa
    #!/usr/bin/env bash
    set -euo pipefail
    IPA=$(ls target/ipa/*.ipa | head -1)
    just _upload-testflight ios "$IPA"

# ─── macOS TestFlight ──────────────────────────────────────

# Bundle + sign macOS app for TestFlight (uses Apple Distribution, not Developer ID)
mac-testflight-build: bundle
    #!/usr/bin/env bash
    set -euo pipefail
    APP="dist/Flynt.app"
    PLIST="$APP/Contents/Info.plist"
    PB=/usr/libexec/PlistBuddy

    # Add LSApplicationCategoryType (required for App Store/TestFlight)
    $PB -c "Add :LSApplicationCategoryType string public.app-category.productivity" "$PLIST" 2>/dev/null || \
    $PB -c "Set :LSApplicationCategoryType public.app-category.productivity" "$PLIST"
    $PB -c "Add :ITSAppUsesNonExemptEncryption bool false" "$PLIST" 2>/dev/null || \
    $PB -c "Set :ITSAppUsesNonExemptEncryption false" "$PLIST"

    # Embed provisioning profile (strip quarantine xattr)
    cp crates/flynt-app/profiles/Styrene_Flynt_PKM_Beta.provisionprofile "$APP/Contents/embedded.provisionprofile"
    xattr -cr "$APP"

    echo "Signing for TestFlight with Apple Distribution cert..."
    codesign --deep --force \
        --entitlements crates/flynt-app/Flynt.appstore.entitlements \
        --sign "{{dist_identity}}" \
        --options runtime \
        "$APP"

    echo "Packaging .pkg for upload..."
    PKG="dist/Flynt.pkg"
    productbuild --component "$APP" /Applications --sign "{{installer_identity}}" "$PKG"
    echo "  Built $PKG"

# Upload macOS build to TestFlight
mac-testflight: mac-testflight-build
    #!/usr/bin/env bash
    set -euo pipefail
    just _upload-testflight osx dist/Flynt.pkg

# ─── Shared upload helper ──────────────────────────────────

# Upload a build to TestFlight via App Store Connect API
_upload-testflight type file:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p private_keys
    cp "{{asc_key_path}}" "private_keys/AuthKey_{{asc_key_id}}.p8"
    echo "Uploading {{file}} to TestFlight..."
    xcrun altool --upload-app \
        --type "{{type}}" \
        --file "{{file}}" \
        --api-key "{{asc_key_id}}" \
        --api-issuer "{{asc_issuer}}"
    echo "  Uploaded — check App Store Connect for processing status"

# Upload both iOS and macOS to TestFlight
testflight: ios-testflight mac-testflight
    @echo "  Both platforms uploaded to TestFlight"

# ─── Android ───────────────────────────────────────────────

# Verify the local Android toolchain needed for tablet sideload testing.
android-check:
    #!/usr/bin/env bash
    set -euo pipefail
    export JAVA_HOME="{{android_java_home}}"
    export ANDROID_HOME="{{android_home}}"
    export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
    export ANDROID_NDK_HOME="{{android_ndk_home}}"
    export PATH="$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"

    command -v java >/dev/null
    command -v adb >/dev/null
    command -v sdkmanager >/dev/null
    command -v dx >/dev/null
    test -d "$ANDROID_HOME"
    test -d "$ANDROID_NDK_HOME"
    rustup target list --installed | grep -qx "{{android_target}}"
    sdkmanager --list_installed | grep -q "platforms;android-35"
    sdkmanager --list_installed | grep -q "build-tools;35.0.0"
    echo "✓ Android toolchain ready for {{android_package}}"
    java -version 2>&1 | head -1
    dx --version

# Verify Android release scaffolding and local toolchain are present.
android-readiness: android-check
    #!/usr/bin/env bash
    set -euo pipefail
    test -f crates/flynt-mobile/android/README.md
    test -f crates/flynt-mobile/android/AndroidManifest.xml.template
    test -f docs/mobile-release-readiness.md
    echo "✓ Android scaffold present for {{android_package}}"

# Build a local debug APK for Android tablet sideload testing.
android-apk target=android_target profile="debug":
    #!/usr/bin/env bash
    set -euo pipefail
    export JAVA_HOME="{{android_java_home}}"
    export ANDROID_HOME="{{android_home}}"
    export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
    export ANDROID_NDK_HOME="{{android_ndk_home}}"
    export PATH="$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"

    RELEASE_FLAG=""
    if [ "{{profile}}" = "release" ]; then
        RELEASE_FLAG="--release"
    elif [ "{{profile}}" != "debug" ]; then
        echo "profile must be 'debug' or 'release'"
        exit 1
    fi

    cd crates/flynt-mobile
    dx build --platform android --target "{{target}}" $RELEASE_FLAG
    cd ../..
    APK=$(find "target/dx/flynt-mobile/{{profile}}/android/app/app/build/outputs/apk" -name "*.apk" | sort | head -1)
    test -n "$APK"
    echo "✓ Built $APK"

# List locally connected Android devices visible to adb.
android-devices:
    #!/usr/bin/env bash
    set -euo pipefail
    export JAVA_HOME="{{android_java_home}}"
    export ANDROID_HOME="{{android_home}}"
    export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
    export ANDROID_NDK_HOME="{{android_ndk_home}}"
    export PATH="$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"
    adb devices -l

# Build and install a local APK onto a connected Android tablet.
android-install target=android_target profile="debug":
    #!/usr/bin/env bash
    set -euo pipefail
    just android-apk "{{target}}" "{{profile}}"
    export JAVA_HOME="{{android_java_home}}"
    export ANDROID_HOME="{{android_home}}"
    export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
    export ANDROID_NDK_HOME="{{android_ndk_home}}"
    export PATH="$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"

    DEVICE_COUNT=$(adb devices | awk 'NR > 1 && $2 == "device" { count++ } END { print count + 0 }')
    if [ "$DEVICE_COUNT" -eq 0 ]; then
        echo "No authorized Android device found. Enable USB debugging, connect the tablet, and accept the RSA prompt."
        adb devices -l
        exit 1
    fi
    if [ "$DEVICE_COUNT" -gt 1 ] && [ -z "${ANDROID_SERIAL:-}" ]; then
        echo "Multiple devices found. Set ANDROID_SERIAL to choose one."
        adb devices -l
        exit 1
    fi

    APK=$(find "target/dx/flynt-mobile/{{profile}}/android/app/app/build/outputs/apk" -name "*.apk" | sort | head -1)
    adb install -r "$APK"
    echo "✓ Installed $APK"

# Stream Flynt logs from a connected Android device.
android-logs:
    #!/usr/bin/env bash
    set -euo pipefail
    export JAVA_HOME="{{android_java_home}}"
    export ANDROID_HOME="{{android_home}}"
    export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
    export ANDROID_NDK_HOME="{{android_ndk_home}}"
    export PATH="$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"
    adb logcat | grep --line-buffered -E "Flynt|flynt|dioxus|wry|rust"

# Placeholder for future Google Play Android App Bundle builds.
android-aab:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Android AAB builds are not wired yet."
    echo "See docs/mobile-release-readiness.md and crates/flynt-mobile/android/README.md."
    exit 1

clean:
    cargo clean
    rm -rf dist
