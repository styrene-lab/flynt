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

Manual release dispatches are macOS-first by default so the team can validate
direct-download DMG/PKG changes without rebuilding every platform. Use the
workflow inputs `include_linux` and `include_ios` when a manual dispatch should
also build those artifacts. Tag pushes still build the full first-party release
set.

The macOS DMG is the primary direct-download artifact. It must be Developer ID
signed, notarized, and stapled before publication.

Flynt does not bundle Omegon in the desktop app. Direct-download users who open
the agent panel without a local Omegon runtime get an in-app setup panel that
checks the binary, ACP session, and Flynt extension state. The panel can launch
the upstream Omegon installer into `~/.local/bin` without prompting, use
Homebrew when available, persist an existing binary path, open runtime settings,
query ACP for Flynt extension/provider readiness, and recheck after each action.

Flynt also performs a lightweight self-update check against GitHub Releases.
When a newer release is available, the toolbar verifies the release's signed
`flynt-release.json` manifest before offering a direct macOS installer path.
For direct-download installs, Flynt downloads the selected PKG/DMG, verifies its
SHA-256 against the signed manifest, writes the verified artifact to Downloads,
and then opens it for the operator to install. If the signed manifest is absent
or invalid, Flynt falls back to the GitHub release page instead of presenting a
verified direct installer flow. Homebrew, Nix, and development builds are
labeled so operators do not accidentally cross installation channels. The badge
can be dismissed per version. The first beta path intentionally keeps
installation user-confirmed rather than mutating the running app bundle in place.

Update discovery is channel-based:

- Stable uses GitHub's latest non-prerelease release endpoint.
- Nightly scans recent GitHub prereleases for timestamped `nightly-*` tags and
  selects the newest signed release manifest whose channel is `nightly`.

The selected Flynt update channel is stored in the launcher profile, alongside
the existing operator-local launcher state. The app never infers the channel
from artifact names; the signed manifest is the authority.

Nightly publication uses the same release workflow with `tag_name=nightly`.
That path checks out the repository default branch, creates a tag like
`nightly-20260515143022-abc1234`, publishes it as a prerelease, and signs a
manifest for that exact tag and commit. Nightly tags are immutable release
records; rollback is handled by publishing a newer nightly from the desired
commit, not by moving an existing tag.

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
- `FLYNT_RELEASE_VERIFY_KEY_B64` - base64 Ed25519 public key embedded into
  release builds for update manifest verification
- `FLYNT_RELEASE_SIGNING_KEY_B64` - base64 Ed25519 32-byte seed used only by
  the release workflow to sign `flynt-release.json`

For backward compatibility, CI will also inspect `APPLE_INSTALLER_CERT_P12_B64`
if the explicit Developer ID Installer secret is absent, but the release fails
unless the imported identity is actually `Developer ID Installer`.

The release workflow installs the prebuilt Dioxus CLI binary for the runner
target and verifies its checksum. It must not compile `dioxus-cli` from source
inside release jobs.

## Icon Assets

The application icon artwork lives in `crates/flynt-app/assets/icon.svg` and
`crates/flynt-app/assets/icon-source.png`. The SVG is the editable vector
export; the 1024x1024 PNG is the raster source used for packaged app icons.
Generated icon files must be refreshed with:

```sh
python3 scripts/generate-icons.py
```

That command updates the desktop `icon.png`, `icon-256.png`, `icon.icns`, and
both iOS `AppIcon.appiconset` directories. Do not hand-edit individual icon
PNGs; the release scripts, Dioxus metadata, Nix package, DMG volume icon, and
iOS packaging paths all consume these generated files.

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
