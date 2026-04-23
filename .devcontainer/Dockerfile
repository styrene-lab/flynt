FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# System dependencies for codex-store (git2 vendored, sqlite bundled)
# and optional desktop build (webkit2gtk for wry)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    cmake \
    libssl-dev \
    libsqlite3-dev \
    # Desktop build deps (optional — needed only for `dx build`)
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    libglib2.0-dev \
    libxdo-dev \
    libayatana-appindicator3-dev \
    && rm -rf /var/lib/apt/lists/*

# Dioxus CLI for desktop builds
RUN cargo install dioxus-cli

# Pre-warm the cargo registry
WORKDIR /tmp/warm
COPY Cargo.toml Cargo.lock ./
COPY crates/codex-core/Cargo.toml crates/codex-core/Cargo.toml
COPY crates/codex-store/Cargo.toml crates/codex-store/Cargo.toml
COPY crates/codex-app/Cargo.toml crates/codex-app/Cargo.toml
COPY crates/codex-agent/Cargo.toml crates/codex-agent/Cargo.toml
COPY crates/codex-mobile/Cargo.toml crates/codex-mobile/Cargo.toml
RUN mkdir -p crates/codex-core/src crates/codex-store/src crates/codex-app/src crates/codex-agent/src crates/codex-mobile/src \
    && echo "fn main(){}" > crates/codex-core/src/lib.rs \
    && echo "fn main(){}" > crates/codex-store/src/lib.rs \
    && echo "fn main(){}" > crates/codex-app/src/lib.rs \
    && echo "fn main(){}" > crates/codex-agent/src/main.rs \
    && echo "fn main(){}" > crates/codex-mobile/src/main.rs \
    && cargo fetch || true \
    && rm -rf /tmp/warm

WORKDIR /workspaces/codex
