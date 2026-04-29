# Local Codyx Build Notes

## Nix/NixOS build

```sh
NIXPKGS_ALLOW_UNFREE=1 nix build .#default --impure --print-build-logs
./result/bin/codyx
```

The Nix package uses `dx build` (Dioxus CLI) internally, which handles
asset hashing and bundling. The output binary and its `assets/` directory
are installed to `$out/bin/`.

## What the Nix package handles

- `dioxus-cli` for proper asset bundling (CSS, JS, themes get hashed filenames)
- WebKitGTK + GTK3 runtime dependencies via `wrapGAppsHook3`
- Desktop entry + icon installation
- `omegon-extension` git dependency hash

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
cargo check -p codex-app

# Run tests
cargo test -p codex-core -p codex-store

# Build + run locally (outside Nix)
cargo run --package codex-app --bin codyx
```

## Annotated git tags

Tag creation uses a local signature fallback, so it works without
global `user.name` / `user.email` git config.
