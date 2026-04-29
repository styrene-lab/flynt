"""Test 6: Sidebar & Vault Switcher — UI Guide Sections 8, 9."""

import pytest


class TestToolbar:
    """Toolbar elements."""

    def test_toolbar_shows_vault_name(self, app):
        """Toolbar displays the vault name."""
        toolbar = app.page.wait_for_selector(".toolbar", timeout=5000)
        name = app.page.text_content(".toolbar-vault-name")
        assert name == "Test Vault"

    def test_toolbar_has_build_hash(self, app):
        """Build hash is visible in the toolbar."""
        hash_el = app.page.query_selector(".toolbar-build-hash")
        assert hash_el is not None
        assert len(hash_el.text_content().strip()) > 0

    def test_toolbar_search_input(self, app):
        """Search input is present and functional."""
        search = app.page.query_selector(".toolbar input[type='search'], .toolbar input[type='text']")
        assert search is not None


class TestVaultSwitcher:
    """Vault switcher in sidebar."""

    def test_current_vault_shown(self, app):
        """Current vault name and path are displayed."""
        app.page.wait_for_selector(".vault-current", timeout=5000)
        name = app.page.text_content(".vault-current-name")
        assert "Test Vault" in name

    def test_open_folder_button(self, app):
        """Open folder button is present."""
        app.page.wait_for_selector(".vault-actions", timeout=5000)
        open_btn = app.page.query_selector(".vault-actions .sidebar-doc:has-text('Open folder')")
        assert open_btn is not None

    def test_add_vault_button(self, app):
        """Add vault button is present."""
        app.page.wait_for_selector(".vault-actions", timeout=5000)
        add_btn = app.page.query_selector(".vault-actions .sidebar-doc:has-text('Add vault')")
        assert add_btn is not None

    def test_add_vault_opens_form(self, app):
        """Clicking Add vault opens the inline form."""
        app.page.wait_for_selector(".vault-actions", timeout=5000)
        app.page.click(".vault-actions .sidebar-doc:has-text('Add vault')")
        app.page.wait_for_selector(".vault-add-form", timeout=2000)


class TestNavigation:
    """Sidebar navigation buttons."""

    def test_nav_buttons_exist(self, app):
        """All four navigation buttons are present."""
        buttons = app.page.query_selector_all(".nav-btn")
        assert len(buttons) == 4  # Notes, Kanban, Graph, Settings

    def test_notes_active_by_default(self, app):
        """Notes nav button is active on launch."""
        active = app.page.query_selector(".nav-btn.active")
        assert active is not None
        assert active.get_attribute("title") == "Notes"

    def test_nav_to_kanban(self, app):
        """Clicking Kanban button navigates to board view."""
        app.page.click(".nav-btn[title='Kanban']")
        app.page.wait_for_selector(".view-kanban", timeout=5000)

    def test_nav_to_graph(self, app):
        """Clicking Graph button navigates to graph view."""
        app.page.click(".nav-btn[title='Graph']")
        app.page.wait_for_selector("svg", timeout=5000)

    def test_nav_to_settings(self, app):
        """Clicking Settings button navigates to settings view."""
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
