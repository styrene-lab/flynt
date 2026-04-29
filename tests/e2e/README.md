# Codyx E2E Tests

End-to-end UI tests using Playwright against the Dioxus webview.

## Running with Podman (recommended)

```bash
./tests/e2e/run.sh                  # build + run all tests
./tests/e2e/run.sh test_01          # run only welcome tests
./tests/e2e/run.sh -k "palette"     # pytest -k filter
```

This builds the `codyx` binary (release), builds a Playwright container,
mounts the binary in, and runs all tests in isolation.

## Running locally

```bash
pip install playwright pytest
playwright install webkit
cargo build --package codex-app --bin codyx
cd tests/e2e
python -m pytest -v
```

## How it works

1. Tests launch the `codyx` binary with `WEBKIT_INSPECTOR_SERVER` to enable CDP
2. Playwright connects to the webview via Chrome DevTools Protocol
3. Tests interact with the rendered HTML/CSS (same as what the user sees)
4. Each test gets a fresh temporary vault via `CODEX_VAULT` env var

## Environment variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `CODYX_BINARY` | Path to the codyx binary | Auto-detected from `target/` |

## Test structure

Tests are organized by UI guide sections (see `docs/ui-guide.md`):

| File | Section | Tests |
|------|---------|-------|
| `test_01_welcome.py` | Welcome / onboarding | 8 |
| `test_02_notes.py` | Notes view, editor, conflicts | 8 |
| `test_03_kanban.py` | Kanban board CRUD | 5 |
| `test_04_palette.py` | Command palette | 7 |
| `test_05_settings.py` | Settings sections | 8 |
| `test_06_sidebar.py` | Toolbar, vault switcher, nav | 8 |
