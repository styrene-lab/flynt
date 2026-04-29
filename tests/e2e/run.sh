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

echo "Building test container (includes Rust build — first run is slow)..."
podman build -t "$IMAGE" -f "$REPO_ROOT/tests/e2e/Containerfile" "$REPO_ROOT"

echo "Running tests..."
podman run --rm \
    --tmpfs /tmp \
    "$IMAGE" "$@"
