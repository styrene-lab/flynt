use anyhow::Result;
use codex_store::vault::Vault;
use omegon_extension::ExtensionServe;
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
    ExtensionServe::new(ext).run().await?;
    Ok(())
}
