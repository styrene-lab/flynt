# Codyx E2E Tests

End-to-end UI tests using Playwright against the Dioxus webview.

## Prerequisites

- Playwright installed: `pip install playwright && playwright install webkit`
- Codyx built: `cargo build --package codex-app`

## Running

```bash
cd tests/e2e
python -m pytest -v
```

## How it works

1. Tests launch the `codyx` binary with `WEBKIT_INSPECTOR_SERVER=127.0.0.1:0` to enable CDP
2. Playwright connects to the webview via CDP
3. Tests interact with the rendered HTML/CSS (same as what the user sees)
4. Each test gets a fresh temporary vault via `CODEX_VAULT` env var

## Test structure

Tests are organized by UI guide sections:
- `test_01_welcome.py` — Welcome/onboarding flows
- `test_02_notes.py` — Note editor, embeds, conflict resolution
- `test_03_kanban.py` — Board/column/task CRUD
- `test_04_palette.py` — Command palette commands + agent mode
- `test_05_settings.py` — Settings sections, save, migration
- `test_06_sidebar.py` — Vault switcher, note list filtering
