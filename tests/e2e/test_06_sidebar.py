"""Test 6: Sidebar & Navigation — UI Guide Sections 8, 9."""

import platform
import pytest

needs_webview = pytest.mark.skipif(
    platform.system() != "Linux",
    reason="DOM tests require Linux WebKit inspector (CDP)"
)


class TestVaultStructure:
    """Vault directory structure (all platforms)."""

    def test_codex_dir_exists(self, app):
        assert (app.vault_dir / ".codex").is_dir()

    def test_notes_on_disk(self, app):
        notes = list(app.vault_dir.glob("*.md"))
        titles = [n.stem for n in notes]
        assert "Welcome" in titles
        assert "Project Notes" in titles

    def test_nested_notes_on_disk(self, app):
        assert (app.vault_dir / "design" / "Architecture.md").exists()


@needs_webview
class TestToolbar:
    def test_toolbar_shows_vault_name(self, app):
        app.page.wait_for_selector(".toolbar", timeout=5000)
        name = app.page.text_content(".toolbar-vault-name")
        assert name == "Test Vault"

    def test_toolbar_has_build_hash(self, app):
        hash_el = app.page.query_selector(".toolbar-build-hash")
        assert hash_el is not None
        assert len(hash_el.text_content().strip()) > 0


@needs_webview
class TestVaultSwitcher:
    def test_current_vault_shown(self, app):
        app.page.wait_for_selector(".vault-current", timeout=5000)
        name = app.page.text_content(".vault-current-name")
        assert "Test Vault" in name

    def test_add_vault_button(self, app):
        app.page.wait_for_selector(".vault-actions", timeout=5000)
        add_btn = app.page.query_selector(".vault-actions .sidebar-doc:has-text('Add vault')")
        assert add_btn is not None

    def test_add_vault_opens_form(self, app):
        app.page.wait_for_selector(".vault-actions", timeout=5000)
        app.page.click(".vault-actions .sidebar-doc:has-text('Add vault')")
        app.page.wait_for_selector(".vault-add-form", timeout=2000)


@needs_webview
class TestNavigation:
    def test_nav_buttons_exist(self, app):
        buttons = app.page.query_selector_all(".nav-btn")
        assert len(buttons) == 4

    def test_nav_to_kanban(self, app):
        app.page.click(".nav-btn[title='Kanban']")
        app.page.wait_for_selector(".view-kanban", timeout=5000)

    def test_nav_to_settings(self, app):
        app.page.click(".nav-btn[title='Settings']")
        app.page.wait_for_selector(".settings-root", timeout=5000)
