use anyhow::Result;
use codex_store::vault::Vault;
use std::{path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("codex_agent=info".parse()?),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str());

    let vault_root = std::env::var("CODEX_VAULT")
        .map(PathBuf::from)
        .ok()
        .unwrap_or_else(|| {
            dirs::document_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("Codex")
        });

    std::fs::create_dir_all(&vault_root)?;
    let vault = Arc::new(Vault::open(&vault_root)?);

    tracing::info!("codex-agent ready, vault={}", vault_root.display());

    let ext = codex_agent::extension::CodexExtension::new(vault);

    match mode {
        Some("--mcp") => {
            // MCP server mode — compatible with Claude Code, Cursor, etc.
            omegon_extension::mcp_shim::serve_mcp(ext)
                .await
                .expect("codex MCP server failed");
        }
        Some("--help") | Some("help") | Some("-h") => {
            println!("codex-agent — vault document and task tools for omegon");
            println!();
            println!("USAGE:");
            println!("  codex-agent                Run as omegon extension (default, v2 protocol)");
            println!("  codex-agent --rpc          Run as omegon extension (explicit)");
            println!("  codex-agent --mcp          Run as MCP server (Claude Code, Cursor, etc.)");
            println!("  codex-agent --help         Show this help");
            println!();
            println!("ENVIRONMENT:");
            println!("  CODEX_VAULT                Vault directory (default: ~/Documents/Codex)");
        }
        Some("--rpc") | _ => {
            // Default: run as omegon extension (v2 bidirectional protocol)
            omegon_extension::serve_v2(ext)
                .await
                .expect("codex extension failed");
        }
    }

    Ok(())
}
