# Local Codyx Build Notes

## Current build path

On Nix/NixOS, build the desktop package with:

```sh
NIXPKGS_ALLOW_UNFREE=1 nix build .#default --impure --print-build-logs
```

The built binary is available at:

```text
./result/bin/codyx
```

## What the Nix package currently handles

The package definition in `flake.nix` includes:

- `cargoLock.outputHashes` for the git dependency `omegon-extension-0.17.0-rc.1`.
- `doCheck = false` for the installable package build.

Package-time tests are disabled because the `codex-store` git integration tests currently depend on mutable git default-branch/global-config behavior that is not stable inside the Nix sandbox. Run validation separately during development.

## Validation commands

Useful checks while iterating:

```sh
cargo check -p codex-store
cargo test -p codex-core -p codex-store
```

## Known packaging issue

The Nix package builds and launches, but the UI currently renders without styling. The likely cause is that `packages.default` builds the Dioxus desktop app through raw Cargo, while the application references CSS/theme assets through Dioxus asset paths such as `asset!("/assets/...")`.

The package output currently contains the binary, desktop file, and icon, but not the full Dioxus asset bundle in the runtime location expected by the app.

The next packaging fix should either:

1. Build/bundle the desktop app through Dioxus (`dx build` / `dx bundle`) in the Nix package, or
2. Keep the Cargo build but explicitly install the CSS/theme/vendor assets where the Dioxus runtime expects them.

## Related runtime fix

Annotated git tag creation uses Codex's local signature helper instead of `repo.signature()`, so creating tags does not require global git `user.name` / `user.email` to be configured.
