"""Test 2: Notes View — UI Guide Section 2."""

import time


class TestNoteFilesystem:
    """Vault filesystem state."""

    def test_vault_has_notes(self, app):
        notes = list(app.vault_dir.glob("*.md"))
        assert len(notes) >= 2

    def test_vault_has_folders(self, app):
        assert (app.vault_dir / "design" / "Architecture.md").exists()

    def test_internal_dirs_not_created(self, app):
        time.sleep(2)
        delegations = list(app.vault_dir.glob("ai/delegations/*.md"))
        assert len(delegations) == 0


class TestConflictFilesystem:
    """Conflict detection via filesystem."""

    def test_conflicted_file_has_markers(self, conflict_app):
        content = (conflict_app.vault_dir / "Conflicted.md").read_text()
        assert "<<<<<<<" in content
        assert "=======" in content
        assert ">>>>>>>" in content
