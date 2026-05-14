# Local Flynt Build Notes

## Nix/NixOS build

```sh
NIXPKGS_ALLOW_UNFREE=1 nix build .#flynt --impure --print-build-logs
./result/bin/flynt

NIXPKGS_ALLOW_UNFREE=1 nix build .#flynt-agent --impure --print-build-logs
./result/bin/flynt-agent --help
```

The Nix package uses `dx build` (Dioxus CLI) internally, which handles
asset hashing and bundling. The output binary and its `assets/` directory
are installed to `$out/bin/`.

The flake also exposes runnable apps:

```sh
nix run .#flynt
nix run .#flynt-agent -- --help
```

## What the Nix package handles

- `dioxus-cli` for proper asset bundling (CSS, JS, themes get hashed filenames)
- WebKitGTK + GTK3 runtime dependencies via `wrapGAppsHook3`
- Desktop entry + icon installation
- `omegon-extension` git dependency hash
- `flynt-agent` as a separate package and app output

## Known issues

- **doCheck is disabled** — git integration tests need mutable git config
  which isn't available in the Nix sandbox. Run `cargo test` separately.
- The package uses `stdenv.mkDerivation` with `dioxus-cli` instead of
  `rustPlatform.buildRustPackage` because the `asset!()` macro requires
  the dx build asset pipeline for hashed filenames.

## Development commands

```sh
# Enter dev shell with all deps
nix develop

# Check compilation
cargo check -p flynt-app

# Run tests
cargo test -p flynt-core -p flynt-store

# Build + run locally (outside Nix)
cargo run --package flynt-app --bin flynt
```

## Android tablet testing

On macOS with Homebrew-provided Android tools:

```sh
brew install openjdk
brew install --cask android-commandlinetools android-platform-tools android-ndk
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo install dioxus-cli --version 0.7.9 --locked

export JAVA_HOME=/opt/homebrew/opt/openjdk
export ANDROID_HOME=/opt/homebrew/share/android-commandlinetools
export ANDROID_SDK_ROOT=$ANDROID_HOME
export ANDROID_NDK_HOME=/opt/homebrew/share/android-ndk
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$PATH"
yes | sdkmanager --licenses
sdkmanager "platforms;android-35" "build-tools;35.0.0"

just android-check
just android-apk
just android-devices
just android-install
```

`just android-install` expects a connected and authorized USB debugging device.
If multiple devices are attached, set `ANDROID_SERIAL` to the target device ID
shown by `just android-devices`.

## Annotated git tags

Tag creation uses a local signature fallback, so it works without
global `user.name` / `user.email` git config.
