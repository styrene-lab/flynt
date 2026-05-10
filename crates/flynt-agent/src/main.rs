use anyhow::Result;
use flynt_store::project::Project;
use std::{path::PathBuf, sync::Arc};

fn env_with_fallback(new_name: &str, old_name: &str) -> Option<String> {
    if let Ok(val) = std::env::var(new_name) {
        return Some(val);
    }
    if let Ok(val) = std::env::var(old_name) {
        tracing::warn!("{old_name} is deprecated, use {new_name} instead");
        return Some(val);
    }
    None
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("flynt_agent=info".parse()?),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str());

    let vault_root = env_with_fallback("FLYNT_VAULT", "CODEX_VAULT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::document_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("Flynt")
        });

    std::fs::create_dir_all(&vault_root)?;
    let project = Arc::new(Project::open(&vault_root)?);

    tracing::info!("flynt-agent ready, project={}", vault_root.display());

    let ext = flynt_agent::extension::FlyntExtension::new(project);

    match mode {
        Some("--mcp") => {
            // MCP server mode — compatible with Claude Code, Cursor, etc.
            omegon_extension::mcp_shim::serve_mcp(ext)
                .await
                .expect("flynt MCP server failed");
        }
        Some("--help") | Some("help") | Some("-h") => {
            println!("flynt-agent — project document and task tools for omegon");
            println!();
            println!("USAGE:");
            println!("  flynt-agent                Run as omegon extension (default, v2 protocol)");
            println!("  flynt-agent --rpc          Run as omegon extension (explicit)");
            println!("  flynt-agent --mcp          Run as MCP server (Claude Code, Cursor, etc.)");
            println!("  flynt-agent --help         Show this help");
            println!();
            println!("ENVIRONMENT:");
            println!("  FLYNT_VAULT                Project directory (default: ~/Documents/Flynt)");
        }
        Some("--rpc") | _ => {
            // Default: run as omegon extension (v2 bidirectional protocol)
            omegon_extension::serve_v2(ext)
                .await
                .expect("flynt extension failed");
        }
    }

    Ok(())
}
