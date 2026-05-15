use anyhow::Result;
use flynt_store::project::Project;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Resolve project root from env, accepting the new name first then any
/// legacy fallbacks (in order). Emits a deprecation warning when a
/// legacy name is the one that hits.
fn env_with_fallback(new_name: &str, legacy: &[&str]) -> Option<String> {
    if let Ok(val) = std::env::var(new_name) {
        return Some(val);
    }
    for old in legacy {
        if let Ok(val) = std::env::var(old) {
            tracing::warn!("{old} is deprecated, use {new_name} instead");
            return Some(val);
        }
    }
    None
}

fn looks_like_project_root(path: &Path) -> bool {
    path.join(".flynt").is_dir() || path.join(".git").is_dir()
}

fn default_project_root() -> PathBuf {
    if let Ok(cwd) = std::env::current_dir() {
        if looks_like_project_root(&cwd) {
            return cwd;
        }
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
                .add_directive("flynt_agent=info".parse()?),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str());

    let project_root = env_with_fallback(
        "FLYNT_PROJECT",
        &["OMEGON_PROJECT_ROOT", "FLYNT_VAULT", "CODEX_VAULT"],
    )
        .map(PathBuf::from)
        .unwrap_or_else(default_project_root);

    std::fs::create_dir_all(&project_root)?;
    let project = Arc::new(Project::open(&project_root)?);

    tracing::info!("flynt-agent ready, project={}", project_root.display());

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
            println!("  FLYNT_PROJECT              Project directory (default: cwd if it looks like a project, otherwise ~/Documents/Flynt)");
            println!("                             Also accepts OMEGON_PROJECT_ROOT from the native extension host.");
            println!("                             Legacy aliases (deprecated): FLYNT_VAULT, CODEX_VAULT");
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

#[cfg(test)]
mod tests {
    use super::looks_like_project_root;

    #[test]
    fn recognizes_flynt_project_root() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".flynt")).unwrap();
        assert!(looks_like_project_root(tmp.path()));
    }

    #[test]
    fn recognizes_git_project_root() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        assert!(looks_like_project_root(tmp.path()));
    }

    #[test]
    fn rejects_plain_directory() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!looks_like_project_root(tmp.path()));
    }
}
