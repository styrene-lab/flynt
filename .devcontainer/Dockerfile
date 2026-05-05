FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# System dependencies for flynt-store (git2 vendored, sqlite bundled)
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
COPY crates/flynt-core/Cargo.toml crates/flynt-core/Cargo.toml
COPY crates/flynt-store/Cargo.toml crates/flynt-store/Cargo.toml
COPY crates/flynt-app/Cargo.toml crates/flynt-app/Cargo.toml
COPY crates/flynt-agent/Cargo.toml crates/flynt-agent/Cargo.toml
COPY crates/flynt-mobile/Cargo.toml crates/flynt-mobile/Cargo.toml
RUN mkdir -p crates/flynt-core/src crates/flynt-store/src crates/flynt-app/src crates/flynt-agent/src crates/flynt-mobile/src \
    && echo "fn main(){}" > crates/flynt-core/src/lib.rs \
    && echo "fn main(){}" > crates/flynt-store/src/lib.rs \
    && echo "fn main(){}" > crates/flynt-app/src/lib.rs \
    && echo "fn main(){}" > crates/flynt-agent/src/main.rs \
    && echo "fn main(){}" > crates/flynt-mobile/src/main.rs \
    && cargo fetch || true \
    && rm -rf /tmp/warm

WORKDIR /workspaces/codex
