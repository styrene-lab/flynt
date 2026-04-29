"""Test 1: Welcome / Onboarding — UI Guide Section 1."""

import pytest


class TestWelcomeScreen:
    """First-launch experience."""

    def test_welcome_shows_codyx_title(self, fresh_app):
        """The welcome screen shows the Codyx branding."""
        title = fresh_app.page.text_content(".welcome-title")
        assert title == "Codyx"

    def test_welcome_shows_three_paths(self, fresh_app):
        """Three primary action cards are visible."""
        cards = fresh_app.page.query_selector_all(".welcome-path-card")
        assert len(cards) >= 3  # Start writing, Sync, Join

    def test_start_writing_creates_vault(self, fresh_app):
        """Clicking 'Start writing' creates a vault and navigates to Notes."""
        fresh_app.page.click(".welcome-path-card.primary")
        # Should navigate away from welcome
        fresh_app.page.wait_for_selector(".sidebar", timeout=10000)
        # Vault directory should have content
        assert (fresh_app.vault_dir / ".codex" / "config.toml").exists()

    def test_sync_expands_options(self, fresh_app):
        """Clicking 'Sync across devices' shows sync options."""
        # Find the sync card (second path card)
        cards = fresh_app.page.query_selector_all(".welcome-path-card")
        cards[1].click()
        fresh_app.page.wait_for_selector(".welcome-sync-options", timeout=3000)

    def test_advanced_section_collapsed(self, fresh_app):
        """Advanced options are hidden by default."""
        advanced = fresh_app.page.query_selector(".welcome-options")
        assert advanced is None

    def test_advanced_section_expands(self, fresh_app):
        """Clicking 'Advanced' reveals advanced options."""
        fresh_app.page.click(".welcome-toggle-advanced")
        fresh_app.page.wait_for_selector(".welcome-options", timeout=2000)
        options = fresh_app.page.query_selector_all(".welcome-option")
        assert len(options) >= 3  # Open folder, Clone, Import

    def test_existing_vault_shows_open(self, app):
        """When a vault exists, welcome shows 'Open your notebook'."""
        # Navigate to welcome
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette-input", timeout=2000)
        app.page.fill(".palette-input", "Welcome")
        app.page.keyboard.press("Enter")
        app.page.wait_for_selector(".welcome-path-card.primary", timeout=5000)

        text = app.page.text_content(".welcome-path-card.primary .welcome-path-title")
        assert "Open" in text


class TestCloneDialog:
    """Clone / connect notebook dialog."""

    def test_clone_dialog_opens(self, fresh_app):
        """Join a shared vault opens the clone dialog."""
        cards = fresh_app.page.query_selector_all(".welcome-path-card")
        cards[2].click()  # Join a shared vault
        fresh_app.page.wait_for_selector(".modal-dialog", timeout=3000)

    def test_clone_dialog_has_fields(self, fresh_app):
        """Clone dialog has URL, branch, and token fields."""
        cards = fresh_app.page.query_selector_all(".welcome-path-card")
        cards[2].click()
        fresh_app.page.wait_for_selector(".modal-dialog", timeout=3000)

        fields = fresh_app.page.query_selector_all(".modal-field input")
        assert len(fields) >= 3  # URL, branch, token

    def test_clone_dialog_cancel(self, fresh_app):
        """Cancel button closes the dialog."""
        cards = fresh_app.page.query_selector_all(".welcome-path-card")
        cards[2].click()
        fresh_app.page.wait_for_selector(".modal-dialog", timeout=3000)

        fresh_app.page.click(".modal-btn.secondary")
        dialog = fresh_app.page.query_selector(".modal-dialog")
        assert dialog is None

    def test_clone_dialog_escape(self, fresh_app):
        """Escape closes the dialog."""
        cards = fresh_app.page.query_selector_all(".welcome-path-card")
        cards[2].click()
        fresh_app.page.wait_for_selector(".modal-dialog", timeout=3000)

        fresh_app.page.keyboard.press("Escape")
        fresh_app.page.wait_for_timeout(500)
        dialog = fresh_app.page.query_selector(".modal-dialog")
        assert dialog is None
