"""Test 5: Settings — UI Guide Section 7."""


class TestSettingsFilesystem:
    """Settings persistence."""

    def test_config_file_exists(self, app):
        assert (app.vault_dir / ".codex" / "config.toml").exists()

    def test_config_has_vault_name(self, app):
        content = (app.vault_dir / ".codex" / "config.toml").read_text()
        assert "Test Vault" in content

    def test_config_has_sync_section(self, app):
        content = (app.vault_dir / ".codex" / "config.toml").read_text()
        assert "[sync]" in content
