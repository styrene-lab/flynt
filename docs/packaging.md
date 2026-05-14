# Flynt Packaging Policy

Flynt ships first-party packages through a small set of channels that match how
the platform is normally used.

## First-Party Channels

| Channel | Platforms | Status | Notes |
|---------|-----------|--------|-------|
| GitHub Releases | macOS, Linux | Official | Canonical binary artifacts for every tagged release. |
| Homebrew Cask | macOS | Official | Installs the signed and notarized release DMG. |
| Homebrew Formula | Linux | Official | Installs the Linux release tarball. |
| Nix flake | Linux, macOS | Official | Builds `flynt` and `flynt-agent` from source. |

## GitHub Release Artifacts

Tagged releases publish:

- `Flynt-{version}-macos.dmg`
- `Flynt-{version}-macos.pkg`
- `flynt-v{version}-linux-amd64.tar.gz`
- `flynt-v{version}-linux-arm64.tar.gz`
- `flynt-agent-*` tarballs for supported platforms

The macOS DMG is the primary direct-download artifact. It must be Developer ID
signed, notarized, and stapled before publication.

For the first public beta target, `v0.10.0`, the direct-download PKG is required
rather than best-effort. CI must fail if no Developer ID Installer identity is
available, because the release should not publish without
`Flynt-0.10.0-macos.pkg`.

## Local macOS Release Validation

Local DMG validation:

```sh
just dmg
hdiutil verify dist/Flynt-{version}-macos.dmg
```

`just release` performs the full local DMG path: bundle, Developer ID
Application signing, DMG creation, notarization, and stapling. Notarization
uses the first available credential source:

- a local notarytool profile named `flynt`
- `APPLE_API_KEY_P8_B64`, `APPLE_API_KEY_ID`, and `APPLE_API_ISSUER`
- `ASC_KEY_PATH`, `ASC_KEY_ID`, and `ASC_ISSUER`

`just release-pkg` requires a local Developer ID Installer certificate. A
`3rd Party Mac Developer Installer` certificate is only valid for Mac App Store
or TestFlight packaging, not independent direct-download PKGs.

Required CI secrets for the `v0.10.0` direct-download PKG:

- `APPLE_DEVID_INSTALLER_CERT_P12_B64`
- `APPLE_DEVID_INSTALLER_CERT_PASSWORD`

For backward compatibility, CI will also inspect `APPLE_INSTALLER_CERT_P12_B64`
if the explicit Developer ID Installer secret is absent, but the release fails
unless the imported identity is actually `Developer ID Installer`.

## Deferred Channels

The project does not maintain first-party `.deb`, `.rpm`, AppImage, Flatpak, or
Snap packages. Those formats are welcome as community-maintained PRs when they
fit the target ecosystem's norms and do not add release-blocking maintenance
burden to the core project.

## Android

Android is not currently a first-party release channel, but it is locally
buildable for device testing. The repo includes Dioxus Android metadata,
scaffold documentation under `crates/flynt-mobile/android/`, and `just`
recipes for toolchain checks, debug APK builds, `adb install`, and logcat.

The local debug APK path is:

```sh
target/dx/flynt-mobile/debug/android/app/app/build/outputs/apk/debug/app-debug.apk
```

Android still lacks production signing, APK/AAB release jobs, Google Play
upload automation, and device/emulator validation in CI.

When Android becomes a product target, the first-party channels should be:

- GitHub Releases for signed APKs suitable for direct sideloading
- Google Play internal/closed testing for normal Android beta distribution
- Google Play production release if the Android app reaches parity

F-Droid, alternative stores, and distro-specific Android packaging are deferred
until the Android app exists and can be maintained without slowing the core
release train.
