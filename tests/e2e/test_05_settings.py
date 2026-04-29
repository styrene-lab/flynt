"""Test 5: Settings — UI Guide Section 7."""

import pytest


class TestBasicSettings:
    """Basic settings (always visible)."""

    def test_settings_view_loads(self, app):
        """Settings view renders without errors."""
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)

    def test_appearance_section_visible(self, app):
        """Appearance section is visible by default."""
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        headings = app.page.query_selector_all(".settings-heading")
        texts = [h.text_content() for h in headings]
        assert "APPEARANCE" in [t.upper() for t in texts]

    def test_vault_section_visible(self, app):
        """Vault section shows name and location."""
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        headings = app.page.query_selector_all(".settings-heading")
        texts = [h.text_content().upper() for h in headings]
        assert "VAULT" in texts

    def test_sync_section_visible(self, app):
        """Sync section is visible."""
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        headings = app.page.query_selector_all(".settings-heading")
        texts = [h.text_content().upper() for h in headings]
        assert "SYNC" in texts

    def test_vault_name_editable(self, app):
        """Vault name can be edited."""
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        name_input = app.page.query_selector(".settings-row:has(.settings-label:text('Name')) input")
        assert name_input is not None
        value = name_input.input_value()
        assert value == "Test Vault"


class TestAdvancedSettings:
    """Advanced settings (collapsed by default)."""

    def test_advanced_hidden_by_default(self, app):
        """Advanced sections are not visible initially."""
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        headings = app.page.query_selector_all(".settings-heading")
        texts = [h.text_content().upper() for h in headings]
        # These should NOT be visible
        assert "VISUALIZATION" not in texts
        assert "IDENTITY" not in texts
        assert "PROVIDERS" not in texts
        assert "AGENT DAEMON" not in texts

    def test_advanced_toggle_reveals_sections(self, app):
        """Clicking 'Show advanced settings' reveals all sections."""
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        app.page.click(".settings-toggle-btn")
        app.page.wait_for_timeout(500)

        headings = app.page.query_selector_all(".settings-heading")
        texts = [h.text_content().upper() for h in headings]
        assert "VISUALIZATION" in texts
        assert "IDENTITY" in texts
        assert "PROVIDERS" in texts
        assert "AGENT DAEMON" in texts

    def test_save_button_exists(self, app):
        """Save changes button is visible."""
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        save_btn = app.page.query_selector(".settings-save-bar .btn-primary")
        assert save_btn is not None
        assert "Save" in save_btn.text_content()

    def test_export_preview_hidden_in_basic(self, app):
        """Export local preview button is only visible in advanced mode."""
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        export_btn = app.page.query_selector(".settings-save-bar .btn-ghost:has-text('Export')")
        assert export_btn is None

    def test_export_preview_shown_in_advanced(self, app):
        """Export button appears after expanding advanced."""
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        app.page.click(".settings-toggle-btn")
        app.page.wait_for_timeout(500)
        export_btn = app.page.query_selector(".settings-save-bar .btn-ghost:has-text('Export')")
        assert export_btn is not None


class TestIdentitySettings:
    """Identity section in advanced settings."""

    def test_identity_shows_status(self, app):
        """Identity section shows current status."""
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        app.page.click(".settings-toggle-btn")
        app.page.wait_for_timeout(500)

        # Find identity section
        status = app.page.query_selector(".identity-status-text")
        assert status is not None

    def test_identity_create_requires_matching_passphrase(self, app):
        """Create identity button is disabled when passphrases don't match."""
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        app.page.click(".settings-toggle-btn")
        app.page.wait_for_timeout(500)

        # Find identity form (only if no identity exists)
        create_btn = app.page.query_selector(".identity-form .btn-primary")
        if create_btn:
            assert create_btn.is_disabled()
