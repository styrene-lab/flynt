# Android Scaffold

Android is reserved as the next mobile platform target for `flynt-mobile`.
This directory records the intended package identity, release artifacts, and
missing build pieces so Android work can start without rediscovering release
requirements.

## Intended Identity

- Package name: `io.styrene.flynt`
- App name: `Flynt`
- Minimum target: Android 10 / API 29 unless product requirements change
- Release artifacts:
  - Signed APK for GitHub Releases and sideload testing
  - Signed AAB for Google Play internal, closed, and production tracks

## Local Tablet Testing

This repo can build a debug APK locally with the Dioxus Android target. On a
Homebrew macOS setup, the checked-in recipes default to:

- `JAVA_HOME=/opt/homebrew/opt/openjdk`
- `ANDROID_HOME=/opt/homebrew/share/android-commandlinetools`
- `ANDROID_NDK_HOME=/opt/homebrew/share/android-ndk`
- `ANDROID_TARGET=aarch64-linux-android`

Install the local prerequisites once:

```sh
brew install openjdk
brew install --cask android-commandlinetools android-platform-tools android-ndk
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo install dioxus-cli --version 0.7.9 --locked
yes | sdkmanager --licenses
sdkmanager "platforms;android-35" "build-tools;35.0.0"
```

Then build and install on a USB-connected tablet:

```sh
just android-check
just android-readiness
just android-apk
just android-devices
just android-install
```

`just android-install` builds a debug APK and installs it with `adb install -r`.
If more than one device is connected, set `ANDROID_SERIAL` to the device ID from
`just android-devices`.

Current local APK output:

```sh
target/dx/flynt-mobile/debug/android/app/app/build/outputs/apk/debug/app-debug.apk
```

`just android-aab` intentionally remains a placeholder until release signing and
Google Play upload plumbing are wired.

## Required Future Work

- Verify app launch and storage behavior on physical tablets.
- Decide which ABIs should be built for direct sideload release APKs.
- Add production adaptive icon assets and Android splash/theme resources.
- Add Android storage/import strategy for Flynt projects if the mobile runtime
  needs platform-specific behavior.
- Finish Android share intent ingestion beyond manifest-level SEND registration.
- Add signing secrets for upload and sideload builds:
  - `ANDROID_KEYSTORE_B64`
  - `ANDROID_KEYSTORE_PASSWORD`
  - `ANDROID_KEY_ALIAS`
  - `ANDROID_KEY_PASSWORD`
  - `GOOGLE_PLAY_SERVICE_ACCOUNT_JSON`
- Add release CI jobs for APK/AAB build, signing, upload-artifact, and optional
  Google Play internal track upload.

## Manifest Template

`AndroidManifest.xml.template` is intentionally not consumed yet. It documents
the expected package permissions and intent surfaces for the future Android app.
