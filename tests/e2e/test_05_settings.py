"""Test 5: Settings — UI Guide Section 7."""

import platform
import pytest

needs_webview = pytest.mark.skipif(
    platform.system() != "Linux",
    reason="DOM tests require Linux WebKit inspector (CDP)"
)


class TestSettingsFilesystem:
    """Settings persistence (all platforms)."""

    def test_config_file_exists(self, app):
        """Vault has a config.toml."""
        assert (app.vault_dir / ".codex" / "config.toml").exists()

    def test_config_has_vault_name(self, app):
        """Config file contains the vault name."""
        content = (app.vault_dir / ".codex" / "config.toml").read_text()
        assert "Test Vault" in content

    def test_config_has_sync_section(self, app):
        """Config file has a sync section."""
        content = (app.vault_dir / ".codex" / "config.toml").read_text()
        assert "[sync]" in content


@needs_webview
class TestBasicSettings:
    def test_settings_view_loads(self, app):
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)

    def test_appearance_section_visible(self, app):
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        headings = app.page.query_selector_all(".settings-heading")
        texts = [h.text_content().upper() for h in headings]
        assert "APPEARANCE" in texts

    def test_vault_name_editable(self, app):
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        name_input = app.page.query_selector(".settings-row:has(.settings-label:text('Name')) input")
        assert name_input is not None
        assert name_input.input_value() == "Test Vault"


@needs_webview
class TestAdvancedSettings:
    def test_advanced_hidden_by_default(self, app):
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        headings = app.page.query_selector_all(".settings-heading")
        texts = [h.text_content().upper() for h in headings]
        assert "VISUALIZATION" not in texts
        assert "PROVIDERS" not in texts

    def test_advanced_toggle_reveals_sections(self, app):
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        app.page.click(".settings-toggle-btn")
        app.page.wait_for_timeout(500)
        headings = app.page.query_selector_all(".settings-heading")
        texts = [h.text_content().upper() for h in headings]
        assert "VISUALIZATION" in texts
        assert "IDENTITY" in texts
        assert "PROVIDERS" in texts

    def test_save_button_exists(self, app):
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
        save_btn = app.page.query_selector(".settings-save-bar .btn-primary")
        assert save_btn is not None
        assert "Save" in save_btn.text_content()
