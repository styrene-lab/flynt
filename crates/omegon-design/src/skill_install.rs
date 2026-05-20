//! Bundled-skill installer.
//!
//! Writes `~/.omegon/skills/flynt-design/SKILL.md` from the bundled bytes when
//! the file is missing or its contents differ. Content-aware so unchanged
//! bytes don't trigger a rewrite (preserves mtime, no spurious file events).
//!
//! The skill content is the source of truth for how the agent should behave
//! when designing on canvas. By bundling and self-installing, the extension
//! guarantees the skill is in place before the first prompt — no
//! `omegon skills install` step required.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// The bundled skill content, baked into the binary at compile time.
pub const SKILL_BYTES: &[u8] = include_bytes!("../assets/SKILL.md");

/// The bundled style-guide template, also baked in. We don't auto-install
/// it (the user opts in by copying it into their project), but exposing it
/// here lets the `design_load_style_guide` tool surface a "no guide
/// configured — here's a starter you can copy" hint.
pub const STYLE_GUIDE_TEMPLATE_BYTES: &[u8] = include_bytes!("../assets/style-guide-template.md");

fn skill_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not resolve home dir")?;
    Ok(home.join(".omegon").join("skills").join("flynt-design"))
}

/// Install or refresh the bundled SKILL.md. Idempotent and content-aware.
pub fn install_bundled_skill() -> Result<()> {
    let dir = skill_dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = dir.join("SKILL.md");

    let current = std::fs::read(&path).ok();
    if current.as_deref() == Some(SKILL_BYTES) {
        tracing::debug!("flynt-design skill up-to-date at {}", path.display());
        return Ok(());
    }

    std::fs::write(&path, SKILL_BYTES).with_context(|| format!("write {}", path.display()))?;
    tracing::info!("flynt-design skill installed at {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_bytes_have_required_frontmatter() {
        let s = std::str::from_utf8(SKILL_BYTES).unwrap();
        assert!(s.starts_with("+++\n"), "missing TOML frontmatter open");
        assert!(s.contains("name = \"flynt-design\""), "skill name");
        assert!(s.contains("triggers = ["), "triggers list");
        assert!(s.contains("Disclosure"), "disclosure section");
    }

    #[test]
    fn style_guide_template_has_frontmatter() {
        let s = std::str::from_utf8(STYLE_GUIDE_TEMPLATE_BYTES).unwrap();
        assert!(s.starts_with("+++\n"));
        assert!(s.contains("[brand]") || s.contains("[colors]"));
    }
}
