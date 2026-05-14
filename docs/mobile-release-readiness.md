# Mobile Release Readiness

This document tracks the mobile release surfaces that should be ready before
Flynt treats iOS or Android as first-class distribution channels.

## iOS

Current status: build and signing scaffolding exists.

Release surfaces:

- TestFlight upload from CI
- IPA artifact from CI
- iOS Share Extension bundled and signed with the app

Known gaps:

- Confirm Dioxus iOS output path remains stable across CLI updates.
- Replace remaining `io.styrene.codex` bundle and app group identifiers when
  TestFlight continuity no longer requires them.
- Add an App Store release lane when production iOS distribution starts.
- Add mobile smoke tests that launch the built app on simulator or device.
- Decide whether direct IPA artifacts stay internal-only or become attached to
  public releases.

Required secrets:

- `APPLE_DIST_CERT_P12_B64`
- `APPLE_DIST_CERT_PASSWORD`
- `IOS_DIST_PROFILE_B64`
- `IOS_SHARE_DIST_PROFILE_B64`
- `APPLE_API_KEY_P8_B64`
- `APPLE_API_KEY_ID`
- `APPLE_API_ISSUER`

## Android

Current status: local debug APK builds are wired for physical-device testing.
Dioxus Android metadata exists in `crates/flynt-mobile/Dioxus.toml`, and the
root `Justfile` can check the Android toolchain, build a debug APK, list
devices, install onto a connected tablet, and stream logs.

Local tablet commands:

```sh
just android-check
just android-readiness
just android-apk
just android-devices
just android-install
just android-logs
```

Current debug APK output:

```sh
target/dx/flynt-mobile/debug/android/app/app/build/outputs/apk/debug/app-debug.apk
```

Planned release surfaces:

- Signed APK attached to GitHub Releases for direct sideloading
- Signed AAB uploaded to Google Play internal testing
- Google Play closed testing and production tracks once parity is reached

Known gaps:

- Verify app launch and storage behavior on a physical Android tablet.
- Decide direct-release ABI coverage and whether APKs are universal or split.
- Add production adaptive icons, theme, and splash resources.
- Add Android storage model for local Flynt projects if platform-specific code
  is needed.
- Finish Android share intent ingestion for text, URLs, markdown, and images.
- Add Android signing and Google Play upload CI jobs.
- Add emulator smoke tests for launch and project bootstrap.

Planned secrets:

- `ANDROID_KEYSTORE_B64`
- `ANDROID_KEYSTORE_PASSWORD`
- `ANDROID_KEY_ALIAS`
- `ANDROID_KEY_PASSWORD`
- `GOOGLE_PLAY_SERVICE_ACCOUNT_JSON`

## Policy

Mobile release work should not block desktop GitHub Releases, Homebrew, or Nix.
Manual desktop release dispatches are macOS-first by default; mobile jobs run
only when explicitly requested through workflow inputs or when a tag push builds
the full release set. Mobile artifacts stay non-blocking until mobile reaches
parity.
