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
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes:0:CFBundleURLSchemes:0 string codex-note" "$PLIST"

    echo "✓ Bundled dist/Flynt.app (v{{version}} build $BUILD_NUM)"

# ─── Code Signing (YubiKey via rcodesign) ───────────────────

# Sign the .app bundle with Developer ID Application cert on YubiKey.
# Same flow as Omegon: rcodesign + smartcard slot 9c.
sign: bundle
    #!/usr/bin/env bash
    set -euo pipefail
    APP="dist/Flynt.app"

    echo "Signing Flynt.app with Apple Developer ID (YubiKey)..."
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
    APP="dist/Flynt.app"
    ZIP="dist/Flynt-{{version}}.zip"

    ditto -c -k --keepParent "$APP" "$ZIP"
    echo "Submitting for Apple notarization..."

    if xcrun notarytool history --keychain-profile "flynt" >/dev/null 2>&1; then
        xcrun notarytool submit "$ZIP" --keychain-profile "flynt" --wait
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
    xcrun notarytool store-credentials "flynt" \
        --apple-id "" \
        --team-id "UZBY9DM42N"
    echo "✓ Credentials stored as keychain profile 'codex'"

# ─── Distribution ───────────────────────────────────────────

# Create a distributable .dmg from the signed .app
dmg: sign
    #!/usr/bin/env bash
    set -euo pipefail
    DMG="dist/Flynt-{{version}}.dmg"
    rm -f "$DMG"
    hdiutil create -volname "Flynt" -srcfolder "dist/Flynt.app" \
        -ov -format UDZO "$DMG"
    echo "✓ Created $DMG"

# Full release: bundle → sign → notarize → dmg
release: notarize dmg
    @echo "✓ Flynt {{version}} ready for distribution"
    @echo "  dist/Flynt.app          (signed + notarized)"
    @echo "  dist/Flynt-{{version}}.dmg  (distributable)"

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
        --entitlements crates/flynt-mobile/ios/Codex.entitlements \
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
        --entitlements crates/flynt-app/Codex.appstore.entitlements \
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

clean:
    cargo clean
    rm -rf dist
