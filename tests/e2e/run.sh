#!/usr/bin/env bash
# Run Codyx E2E tests via Podman.
#
# Usage:
#   ./tests/e2e/run.sh              # build + run all tests
#   ./tests/e2e/run.sh test_01      # run only welcome tests
#   ./tests/e2e/run.sh -k "palette" # pytest -k filter
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
IMAGE="codyx-e2e"
BINARY="$REPO_ROOT/target/release/codyx"

# Build the binary if needed
if [ ! -f "$BINARY" ]; then
    echo "Building codyx (release)..."
    cargo build --package codex-app --bin codyx --release --manifest-path "$REPO_ROOT/Cargo.toml"
fi

# Build the test container
echo "Building test container..."
podman build -t "$IMAGE" -f "$REPO_ROOT/tests/e2e/Containerfile" "$REPO_ROOT/tests/e2e"

# Run tests
# - Mount the binary into the container
# - Create a tmpdir for vault fixtures
# - Pass through any pytest args
echo "Running tests..."
podman run --rm \
    -v "$BINARY:/usr/local/bin/codyx:ro" \
    -v "$REPO_ROOT/tests/e2e:/tests:ro" \
    -e CODYX_BINARY=/usr/local/bin/codyx \
    --tmpfs /tmp \
    "$IMAGE" "$@"
