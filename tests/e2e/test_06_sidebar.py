"""Test 6: Sidebar & Navigation — UI Guide Sections 8, 9."""


class TestVaultStructure:
    """Vault directory structure."""

    def test_codex_dir_exists(self, app):
        assert (app.vault_dir / ".codex").is_dir()

    def test_notes_on_disk(self, app):
        notes = list(app.vault_dir.glob("*.md"))
        titles = [n.stem for n in notes]
        assert "Welcome" in titles
        assert "Project Notes" in titles

    def test_nested_notes_on_disk(self, app):
        assert (app.vault_dir / "design" / "Architecture.md").exists()
