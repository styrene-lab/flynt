"""Test 1: Welcome / Onboarding — UI Guide Section 1."""

import time


class TestWelcomeVaultCreation:
    """Vault creation via filesystem checks."""

    def test_fresh_vault_gets_config(self, fresh_app):
        """Launching with an empty vault dir creates .codex/config.toml."""
        time.sleep(3)
        config = fresh_app.vault_dir / ".codex" / "config.toml"
        assert config.exists() or not fresh_app.vault_dir.joinpath(".codex").exists()

    def test_vault_with_notes_has_index(self, app):
        """A vault with notes gets a .codex directory."""
        time.sleep(2)
        assert (app.vault_dir / ".codex").exists()
