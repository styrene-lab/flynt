/// MCP server for Codex — exposes vault tools to Omegon.
///
/// Transport: stdio (Omegon connects via `command` transport)
/// Capabilities exposed: tools (documents, tasks, boards, search)
///
/// To wire Omegon in, add to ~/.config/omegon/mcp.json:
///   {
///     "codex": {
///       "command": "/path/to/codex-agent",
///       "args": ["--vault", "/path/to/vault"],
///       "transport": "stdio"
///     }
///   }
use anyhow::Result;
use codex_store::vault::Vault;
use rmcp::{ServiceExt, transport::stdio};
use std::{path::PathBuf, sync::Arc};
use crate::tools::CodexToolHandler;

pub async fn run_mcp_server(vault_root: PathBuf) -> Result<()> {
    let vault = Arc::new(Vault::open(&vault_root)?);
    let handler = CodexToolHandler::new(vault);
    let service = handler.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
