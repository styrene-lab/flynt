"""Test 4: Command Palette — UI Guide Section 6."""

import platform
import pytest

needs_webview = pytest.mark.skipif(
    platform.system() != "Linux",
    reason="DOM tests require Linux WebKit inspector (CDP)"
)


@needs_webview
class TestCommandMode:
    def test_palette_opens_on_cmd_p(self, app):
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette", timeout=2000)

    def test_palette_closes_on_escape(self, app):
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette", timeout=2000)
        app.page.keyboard.press("Escape")
        app.page.wait_for_timeout(500)
        assert app.page.query_selector(".palette") is None

    def test_palette_shows_commands(self, app):
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette-results", timeout=2000)
        items = app.page.query_selector_all(".palette-item")
        assert len(items) > 5

    def test_palette_fuzzy_search(self, app):
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette-input", timeout=2000)
        app.page.fill(".palette-input", "sett")
        app.page.wait_for_timeout(300)
        items = app.page.query_selector_all(".palette-item")
        labels = [i.text_content() for i in items]
        assert any("Settings" in l for l in labels)

    def test_palette_navigate_to_settings(self, app):
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette-input", timeout=2000)
        app.page.fill(".palette-input", "Settings")
        app.page.keyboard.press("Enter")
        app.page.wait_for_selector(".settings-root", timeout=5000)


@needs_webview
class TestAgentMode:
    def test_cmd_k_ignored_without_agent(self, app):
        app.page.keyboard.press("Meta+k")
        app.page.wait_for_timeout(500)
        assert app.page.query_selector(".palette-mode-tab:has-text('Agent')") is None

    def test_no_agent_tab_in_palette(self, app):
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette", timeout=2000)
        assert len(app.page.query_selector_all(".palette-mode-tab:has-text('Agent')")) == 0
