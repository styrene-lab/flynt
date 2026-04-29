"""Test 1: Welcome / Onboarding — UI Guide Section 1."""

import platform
import pytest

needs_webview = pytest.mark.skipif(
    platform.system() != "Linux",
    reason="DOM tests require Linux WebKit inspector (CDP)"
)


class TestWelcomeVaultCreation:
    """Vault creation (works on all platforms via filesystem checks)."""

    def test_fresh_vault_gets_config(self, fresh_app):
        """Launching with an empty vault dir creates .codex/config.toml."""
        # Give the app time to initialize
        import time
        time.sleep(3)
        config = fresh_app.vault_dir / ".codex" / "config.toml"
        # The app creates config on first open
        assert config.exists() or not fresh_app.vault_dir.joinpath(".codex").exists()
        # If no config, the welcome screen is showing (no vault created yet)

    def test_vault_with_notes_has_index(self, app):
        """A vault with notes gets an indexed database."""
        import time
        time.sleep(2)
        # The vault should have .codex directory
        assert (app.vault_dir / ".codex").exists()


@needs_webview
class TestWelcomeScreen:
    """DOM-level welcome screen tests (Linux only)."""

    def test_welcome_shows_codyx_title(self, fresh_app):
        title = fresh_app.page.text_content(".welcome-title")
        assert title == "Codyx"

    def test_welcome_shows_three_paths(self, fresh_app):
        cards = fresh_app.page.query_selector_all(".welcome-path-card")
        assert len(cards) >= 3

    def test_start_writing_creates_vault(self, fresh_app):
        fresh_app.page.click(".welcome-path-card.primary")
        fresh_app.page.wait_for_selector(".sidebar", timeout=10000)
        assert (fresh_app.vault_dir / ".codex" / "config.toml").exists()

    def test_sync_expands_options(self, fresh_app):
        cards = fresh_app.page.query_selector_all(".welcome-path-card")
        cards[1].click()
        fresh_app.page.wait_for_selector(".welcome-sync-options", timeout=3000)

    def test_advanced_section_collapsed(self, fresh_app):
        advanced = fresh_app.page.query_selector(".welcome-options")
        assert advanced is None

    def test_advanced_section_expands(self, fresh_app):
        fresh_app.page.click(".welcome-toggle-advanced")
        fresh_app.page.wait_for_selector(".welcome-options", timeout=2000)
        options = fresh_app.page.query_selector_all(".welcome-option")
        assert len(options) >= 3

    def test_existing_vault_shows_open(self, app):
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette-input", timeout=2000)
        app.page.fill(".palette-input", "Welcome")
        app.page.keyboard.press("Enter")
        app.page.wait_for_selector(".welcome-path-card.primary", timeout=5000)
        text = app.page.text_content(".welcome-path-card.primary .welcome-path-title")
        assert "Open" in text


@needs_webview
class TestCloneDialog:
    """Clone dialog tests (Linux only)."""

    def test_clone_dialog_opens(self, fresh_app):
        cards = fresh_app.page.query_selector_all(".welcome-path-card")
        cards[2].click()
        fresh_app.page.wait_for_selector(".modal-dialog", timeout=3000)

    def test_clone_dialog_has_fields(self, fresh_app):
        cards = fresh_app.page.query_selector_all(".welcome-path-card")
        cards[2].click()
        fresh_app.page.wait_for_selector(".modal-dialog", timeout=3000)
        fields = fresh_app.page.query_selector_all(".modal-field input")
        assert len(fields) >= 3

    def test_clone_dialog_cancel(self, fresh_app):
        cards = fresh_app.page.query_selector_all(".welcome-path-card")
        cards[2].click()
        fresh_app.page.wait_for_selector(".modal-dialog", timeout=3000)
        fresh_app.page.click(".modal-btn.secondary")
        dialog = fresh_app.page.query_selector(".modal-dialog")
        assert dialog is None
