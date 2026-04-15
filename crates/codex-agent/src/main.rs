use anyhow::Result;
use clap::Parser;
use codex_store::vault::Vault;
use omegon_extension::ExtensionServe;
use std::{path::PathBuf, sync::Arc};

#[derive(Parser)]
#[command(name = "codex-agent", about = "Codex vault tools for Omegon")]
struct Args {
    /// Path to the vault root directory
    #[arg(long, env = "CODEX_VAULT")]
    vault: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("codex_agent=info".parse()?),
        )
        .init();

    let args = Args::parse();

    let vault_root = args.vault.unwrap_or_else(|| {
        dirs::document_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("Codex")
    });

    std::fs::create_dir_all(&vault_root)?;
    let vault = Arc::new(Vault::open(&vault_root)?);

    tracing::info!("codex-agent ready, vault={}", vault_root.display());

    let ext = codex_agent::extension::CodexExtension::new(vault);
    ExtensionServe::new(ext).run().await?;
    Ok(())
}
