"""Shared fixtures for Codyx E2E tests."""

import os
import signal
import subprocess
import tempfile
import time
from pathlib import Path

import pytest
from playwright.sync_api import sync_playwright


REPO_ROOT = Path(__file__).parent.parent.parent

# Resolve binary: env override → release → debug
_env_bin = os.environ.get("CODYX_BINARY")
if _env_bin:
    BINARY = Path(_env_bin)
elif (REPO_ROOT / "target" / "release" / "codyx").exists():
    BINARY = REPO_ROOT / "target" / "release" / "codyx"
else:
    BINARY = REPO_ROOT / "target" / "debug" / "codyx"


def wait_for_port(port: int, timeout: float = 15.0) -> bool:
    """Wait until a TCP port is accepting connections."""
    import socket
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.5):
                return True
        except (ConnectionRefusedError, OSError):
            time.sleep(0.3)
    return False


@pytest.fixture(scope="session")
def binary_path():
    """Ensure the binary is built and return its path."""
    if not BINARY.exists():
        subprocess.run(
            ["cargo", "build", "--package", "codex-app", "--bin", "codyx"],
            cwd=REPO_ROOT,
            check=True,
        )
    assert BINARY.exists(), f"Binary not found at {BINARY}"
    return BINARY


@pytest.fixture
def vault_dir(tmp_path):
    """Create a fresh temporary vault directory."""
    vault = tmp_path / "test-vault"
    vault.mkdir()
    return vault


@pytest.fixture
def vault_with_notes(vault_dir):
    """Create a vault with some sample notes."""
    codex_dir = vault_dir / ".codex"
    codex_dir.mkdir()
    (codex_dir / "config.toml").write_text(
        'vault_name = "Test Vault"\n\n[sync]\nbackend = "none"\n\n[appearance]\ntheme = "alpharius"\n'
    )

    # Sample notes
    (vault_dir / "Welcome.md").write_text(
        '+++\ntitle = "Welcome"\ntags = []\n+++\n\n# Welcome\n\nHello world.\n'
    )
    (vault_dir / "Project Notes.md").write_text(
        '+++\ntitle = "Project Notes"\ntags = ["work"]\n+++\n\n# Project\n\nSome project notes.\n'
    )

    # Folder structure
    design_dir = vault_dir / "design"
    design_dir.mkdir()
    (design_dir / "Architecture.md").write_text(
        '+++\ntitle = "Architecture"\ntags = ["design"]\n+++\n\n# Architecture\n\nSystem design.\n'
    )

    return vault_dir


@pytest.fixture
def vault_with_conflict(vault_dir):
    """Create a vault with a conflicted file."""
    codex_dir = vault_dir / ".codex"
    codex_dir.mkdir()
    (codex_dir / "config.toml").write_text(
        'vault_name = "Conflict Test"\n\n[sync]\nbackend = "none"\n'
    )
    (vault_dir / "Conflicted.md").write_text(
        '+++\ntitle = "Conflicted"\ntags = []\n+++\n\n'
        'Some text before.\n\n'
        '<<<<<<< HEAD\n'
        'My local change.\n'
        '=======\n'
        'Their remote change.\n'
        '>>>>>>> main\n\n'
        'Text after.\n'
    )
    return vault_dir


@pytest.fixture
def vault_with_board(vault_dir):
    """Create a vault pre-configured with a kanban board."""
    codex_dir = vault_dir / ".codex"
    codex_dir.mkdir()
    (codex_dir / "config.toml").write_text(
        'vault_name = "Board Test"\n\n[sync]\nbackend = "none"\n'
    )
    # The board will be created via the UI in tests
    return vault_dir


class CodyxApp:
    """Manages a running Codyx instance for testing."""

    def __init__(self, binary: Path, vault_dir: Path, inspector_port: int = 9222):
        self.binary = binary
        self.vault_dir = vault_dir
        self.inspector_port = inspector_port
        self.process = None
        self.page = None
        self._playwright = None
        self._browser = None

    def start(self):
        """Launch the app and connect Playwright."""
        env = os.environ.copy()
        env["CODEX_VAULT"] = str(self.vault_dir)
        # Enable WebKit inspector for Playwright connection
        env["WEBKIT_INSPECTOR_SERVER"] = f"127.0.0.1:{self.inspector_port}"

        self.process = subprocess.Popen(
            [str(self.binary), "--vault", str(self.vault_dir)],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        # Wait for the inspector port
        if not wait_for_port(self.inspector_port):
            self.stop()
            raise RuntimeError(
                f"Codyx did not open inspector port {self.inspector_port} within timeout"
            )

        # Connect Playwright
        self._playwright = sync_playwright().start()
        self._browser = self._playwright.webkit.connect_over_cdp(
            f"http://127.0.0.1:{self.inspector_port}"
        )
        self.page = self._browser.contexts[0].pages[0]

        # Wait for the app to render
        self.page.wait_for_selector(".codex-shell, .view-welcome", timeout=10000)

    def stop(self):
        """Shut down the app and Playwright."""
        if self._browser:
            try:
                self._browser.close()
            except Exception:
                pass
        if self._playwright:
            try:
                self._playwright.stop()
            except Exception:
                pass
        if self.process:
            self.process.send_signal(signal.SIGTERM)
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()

    def __enter__(self):
        self.start()
        return self

    def __exit__(self, *args):
        self.stop()


@pytest.fixture
def app(binary_path, vault_with_notes):
    """Launch Codyx with a pre-populated vault and return a connected page."""
    with CodyxApp(binary_path, vault_with_notes, inspector_port=9222) as app:
        yield app


@pytest.fixture
def fresh_app(binary_path, vault_dir):
    """Launch Codyx with an empty vault (triggers welcome screen)."""
    with CodyxApp(binary_path, vault_dir, inspector_port=9223) as app:
        yield app


@pytest.fixture
def conflict_app(binary_path, vault_with_conflict):
    """Launch Codyx with a conflicted file."""
    with CodyxApp(binary_path, vault_with_conflict, inspector_port=9224) as app:
        yield app
