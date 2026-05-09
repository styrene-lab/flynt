//! omegon-design — extension binary.
//!
//! Two responsibilities:
//!  1. On startup, install (or refresh) the bundled `flynt-design` skill at
//!     `~/.omegon/skills/flynt-design/SKILL.md`. Idempotent and content-aware
//!     — only writes when bytes differ, so subsequent launches with the same
//!     bundled content are no-ops.
//!  2. Serve a small set of canvas-design helper tools over the omegon ACP
//!     extension protocol: `design_describe_influences`, `design_load_style_guide`,
//!     `design_suggest_theme`, `design_critique`. None of these write canvas
//!     content directly — they inform the agent's design decisions and audit
//!     the current state. Canvas writes still go through `flynt-agent`'s
//!     `canvas_set_cells`.

use anyhow::Result;
use std::path::PathBuf;

mod extension;
mod skill_install;
mod style_guide;

fn vault_root() -> PathBuf {
    if let Ok(v) = std::env::var("FLYNT_VAULT") {
        return PathBuf::from(v);
    }
    if let Ok(v) = std::env::var("OMEGON_PROJECT_ROOT") {
        return PathBuf::from(v);
    }
    dirs::document_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Flynt")
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("omegon_design=info".parse()?),
        )
        .init();

    // First-launch / every-launch skill install. Failure here is logged but
    // not fatal — the extension can still serve helper tools, just without
    // its associated skill prompt available to omegon's session prompt.
    if let Err(e) = skill_install::install_bundled_skill() {
        tracing::warn!("skill install failed: {e} — continuing without it");
    }

    let root = vault_root();
    if let Err(e) = std::fs::create_dir_all(&root) {
        tracing::warn!("vault dir create failed: {} — {e}", root.display());
    }

    tracing::info!("omegon-design ready, vault={}", root.display());

    let ext = extension::DesignExtension::new(root);

    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str());

    match mode {
        Some("--mcp") => {
            omegon_extension::mcp_shim::serve_mcp(ext)
                .await
                .expect("omegon-design MCP server failed");
        }
        Some("--help") | Some("help") | Some("-h") => {
            println!("omegon-design — canvas design helper tools + flynt-design skill installer");
            println!();
            println!("USAGE:");
            println!("  omegon-design                Run as omegon extension (default)");
            println!("  omegon-design --rpc          Run as omegon extension (explicit)");
            println!("  omegon-design --mcp          Run as MCP server");
            println!("  omegon-design --help         Show this help");
        }
        Some("--rpc") | _ => {
            omegon_extension::serve_v2(ext)
                .await
                .expect("omegon-design extension failed");
        }
    }

    Ok(())
}
