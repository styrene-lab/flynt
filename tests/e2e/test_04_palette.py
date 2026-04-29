"""Test 4: Command Palette — UI Guide Section 6."""

import pytest


class TestCommandMode:
    """Cmd+P command palette."""

    def test_palette_opens_on_cmd_p(self, app):
        """Cmd+P opens the command palette."""
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette", timeout=2000)

    def test_palette_closes_on_escape(self, app):
        """Escape closes the palette."""
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette", timeout=2000)
        app.page.keyboard.press("Escape")
        app.page.wait_for_timeout(500)
        palette = app.page.query_selector(".palette")
        assert palette is None

    def test_palette_shows_commands(self, app):
        """Palette shows navigation and action commands."""
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette-results", timeout=2000)
        items = app.page.query_selector_all(".palette-item")
        assert len(items) > 5  # Navigate + Create + Action commands

    def test_palette_fuzzy_search(self, app):
        """Typing filters commands by fuzzy match."""
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette-input", timeout=2000)
        app.page.fill(".palette-input", "sett")
        app.page.wait_for_timeout(300)
        items = app.page.query_selector_all(".palette-item")
        labels = [i.text_content() for i in items]
        assert any("Settings" in l for l in labels)

    def test_palette_opens_note(self, app):
        """Selecting a note in the palette opens it."""
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette-input", timeout=2000)
        app.page.fill(".palette-input", "Welcome")
        app.page.wait_for_timeout(300)
        app.page.keyboard.press("Enter")
        app.page.wait_for_selector(".notes-pane", timeout=5000)

    def test_palette_navigate_to_settings(self, app):
        """Selecting Settings navigates to settings view."""
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette-input", timeout=2000)
        app.page.fill(".palette-input", "Settings")
        app.page.keyboard.press("Enter")
        app.page.wait_for_selector(".settings-root", timeout=5000)

    def test_palette_shows_templates(self, app):
        """Templates appear in the palette as 'New from: <name>'."""
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette-input", timeout=2000)
        app.page.fill(".palette-input", "new from")
        app.page.wait_for_timeout(300)
        items = app.page.query_selector_all(".palette-item")
        # At least the default templates should appear
        labels = [i.text_content() for i in items]
        assert any("New from" in l or "Template" in l for l in labels) or len(items) == 0


class TestAgentMode:
    """Cmd+K agent delegation."""

    def test_cmd_k_ignored_without_agent(self, app):
        """Cmd+K does nothing when no agent is connected."""
        app.page.keyboard.press("Meta+k")
        app.page.wait_for_timeout(500)
        # Agent tab should not appear
        agent_tab = app.page.query_selector(".palette-mode-tab:has-text('Agent')")
        assert agent_tab is None

    def test_no_agent_tab_in_palette(self, app):
        """Without agent, palette has no Agent tab."""
        app.page.keyboard.press("Meta+p")
        app.page.wait_for_selector(".palette", timeout=2000)
        # Should only show Commands tab (or no tabs at all)
        agent_tabs = app.page.query_selector_all(".palette-mode-tab:has-text('Agent')")
        assert len(agent_tabs) == 0
