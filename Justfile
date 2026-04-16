# Codex — Obsidian-compatible knowledge base
set shell := ["bash", "-cu"]

default:
    @just --list --unsorted

vault := env_var_or_default("CODEX_VAULT", env_var("HOME") + "/workspace/black-meridian/obsidian/Black Meridian")

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

validate:
    cargo fmt --check
    cargo check
    cargo clippy --all-targets -- -D warnings
    cargo test

# ─── Build & distribution ───────────────────────────────────

build:
    cargo build --release

# Bundle .app, patch Info.plist with codex-note:// URL scheme, open result.
bundle:
    #!/usr/bin/env bash
    set -euo pipefail
    dx bundle --platform desktop --release
    PLIST="dist/Codex.app/Contents/Info.plist"
    # Add codex-note:// URL scheme registration
    /usr/libexec/PlistBuddy -c "Delete :CFBundleURLTypes" "$PLIST" 2>/dev/null || true
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes array"                              "$PLIST"
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes:0 dict"                             "$PLIST"
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes:0:CFBundleURLName string com.black-meridian.codex" "$PLIST"
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes:0:CFBundleURLSchemes array"         "$PLIST"
    /usr/libexec/PlistBuddy -c "Add :CFBundleURLTypes:0:CFBundleURLSchemes:0 string codex-note" "$PLIST"
    # Register with macOS launch services so other apps see it immediately
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
        -f dist/Codex.app
    echo "✓ Bundled and registered codex-note:// scheme"

open:
    CODEX_VAULT="{{vault}}" open dist/Codex.app

dist: bundle open

clean:
    cargo clean
    rm -rf dist
