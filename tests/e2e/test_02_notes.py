"""Test 2: Notes View — UI Guide Section 2."""

import pytest


class TestNoteList:
    """Sidebar note list behavior."""

    def test_sidebar_shows_notes(self, app):
        """Notes appear in the sidebar."""
        app.page.wait_for_selector(".sidebar-doc", timeout=5000)
        docs = app.page.query_selector_all(".sidebar-doc .doc-title")
        titles = [d.text_content() for d in docs]
        assert "Welcome" in titles
        assert "Project Notes" in titles

    def test_sidebar_has_folders(self, app):
        """Folder groups appear for nested notes."""
        folders = app.page.query_selector_all(".sidebar-folder-header")
        folder_names = [f.text_content() for f in folders]
        # 'design' folder should appear
        assert any("design" in n.lower() for n in folder_names)

    def test_sidebar_filters_internal_docs(self, app):
        """Agent delegations and memory facts are hidden from sidebar."""
        docs = app.page.query_selector_all(".sidebar-doc .doc-title")
        titles = [d.text_content() for d in docs]
        for title in titles:
            assert "delegation" not in title.lower()
            assert "memory" not in title.lower()

    def test_click_note_opens_in_pane(self, app):
        """Clicking a note opens it in the main pane."""
        app.page.click(".sidebar-doc:has(.doc-title:text('Welcome'))")
        app.page.wait_for_selector(".notes-pane", timeout=5000)
        # Tab bar should show the note
        app.page.wait_for_selector(".tab-bar", timeout=3000)


class TestNoteEditor:
    """Note editing behavior."""

    def test_note_renders_markdown(self, app):
        """Opened note shows rendered markdown content."""
        app.page.click(".sidebar-doc:has(.doc-title:text('Welcome'))")
        app.page.wait_for_selector(".markdown-body", timeout=5000)
        content = app.page.text_content(".markdown-body")
        assert "Hello world" in content

    def test_mode_toggle_exists(self, app):
        """Mode toggle button is visible in the top bar."""
        app.page.click(".sidebar-doc:has(.doc-title:text('Welcome'))")
        app.page.wait_for_selector(".notes-topbar", timeout=5000)
        # Should have mode toggle buttons
        buttons = app.page.query_selector_all(".notes-topbar .btn")
        assert len(buttons) > 0


class TestConflictResolution:
    """Merge conflict banner."""

    def test_conflict_banner_shows(self, conflict_app):
        """A file with conflict markers shows the resolution banner."""
        conflict_app.page.click(".sidebar-doc:has(.doc-title:text('Conflicted'))")
        conflict_app.page.wait_for_selector(".conflict-banner", timeout=5000)
        banner_text = conflict_app.page.text_content(".conflict-banner")
        assert "merge conflicts" in banner_text.lower()

    def test_conflict_banner_has_actions(self, conflict_app):
        """Banner has Keep mine, Keep theirs, and Edit manually buttons."""
        conflict_app.page.click(".sidebar-doc:has(.doc-title:text('Conflicted'))")
        conflict_app.page.wait_for_selector(".conflict-banner", timeout=5000)
        actions = conflict_app.page.query_selector_all(".conflict-actions .btn")
        labels = [a.text_content() for a in actions]
        assert "Keep mine" in labels
        assert "Keep theirs" in labels
        assert "Edit manually" in labels

    def test_keep_mine_resolves(self, conflict_app):
        """'Keep mine' removes conflict markers and keeps local content."""
        conflict_app.page.click(".sidebar-doc:has(.doc-title:text('Conflicted'))")
        conflict_app.page.wait_for_selector(".conflict-banner", timeout=5000)
        conflict_app.page.click(".conflict-actions .btn:text('Keep mine')")

        # Banner should disappear after resolution
        conflict_app.page.wait_for_timeout(2000)
        # File on disk should not have conflict markers
        content = (conflict_app.vault_dir / "Conflicted.md").read_text()
        assert "<<<<<<<" not in content
        assert "My local change" in content
