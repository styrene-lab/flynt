"""Test 2: Notes View — UI Guide Section 2."""

import platform
import pytest

needs_webview = pytest.mark.skipif(
    platform.system() != "Linux",
    reason="DOM tests require Linux WebKit inspector (CDP)"
)


class TestNoteFilesystem:
    """Vault filesystem state (all platforms)."""

    def test_vault_has_notes(self, app):
        """Pre-populated vault has markdown files."""
        notes = list(app.vault_dir.glob("*.md"))
        assert len(notes) >= 2  # Welcome.md + Project Notes.md

    def test_vault_has_folders(self, app):
        """Nested notes create folder structure."""
        assert (app.vault_dir / "design" / "Architecture.md").exists()

    def test_internal_dirs_not_created(self, app):
        """Agent-internal directories aren't created on launch."""
        # These are only created when the agent writes to them
        import time
        time.sleep(2)
        # Fresh vault shouldn't have delegation files
        delegations = list(app.vault_dir.glob("ai/delegations/*.md"))
        assert len(delegations) == 0


class TestConflictFilesystem:
    """Conflict detection via filesystem (all platforms)."""

    def test_conflicted_file_has_markers(self, conflict_app):
        """The test fixture file has conflict markers."""
        content = (conflict_app.vault_dir / "Conflicted.md").read_text()
        assert "<<<<<<<" in content
        assert "=======" in content
        assert ">>>>>>>" in content


@needs_webview
class TestNoteList:
    """Sidebar note list (Linux DOM tests)."""

    def test_sidebar_shows_notes(self, app):
        app.page.wait_for_selector(".sidebar-doc", timeout=5000)
        docs = app.page.query_selector_all(".sidebar-doc .doc-title")
        titles = [d.text_content() for d in docs]
        assert "Welcome" in titles

    def test_sidebar_filters_internal_docs(self, app):
        docs = app.page.query_selector_all(".sidebar-doc .doc-title")
        titles = [d.text_content() for d in docs]
        for title in titles:
            assert "delegation" not in title.lower()

    def test_click_note_opens_in_pane(self, app):
        app.page.click(".sidebar-doc:has(.doc-title:text('Welcome'))")
        app.page.wait_for_selector(".notes-pane", timeout=5000)


@needs_webview
class TestNoteEditor:
    """Note editing (Linux DOM tests)."""

    def test_note_renders_markdown(self, app):
        app.page.click(".sidebar-doc:has(.doc-title:text('Welcome'))")
        app.page.wait_for_selector(".markdown-body", timeout=5000)
        content = app.page.text_content(".markdown-body")
        assert "Hello world" in content


@needs_webview
class TestConflictResolution:
    """Merge conflict banner (Linux DOM tests)."""

    def test_conflict_banner_shows(self, conflict_app):
        conflict_app.page.click(".sidebar-doc:has(.doc-title:text('Conflicted'))")
        conflict_app.page.wait_for_selector(".conflict-banner", timeout=5000)

    def test_keep_mine_resolves(self, conflict_app):
        conflict_app.page.click(".sidebar-doc:has(.doc-title:text('Conflicted'))")
        conflict_app.page.wait_for_selector(".conflict-banner", timeout=5000)
        conflict_app.page.click(".conflict-actions .btn:text('Keep mine')")
        conflict_app.page.wait_for_timeout(2000)
        content = (conflict_app.vault_dir / "Conflicted.md").read_text()
        assert "<<<<<<<" not in content
        assert "My local change" in content
