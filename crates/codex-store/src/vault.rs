use anyhow::Result;
use codex_core::{
    models::*,
    parser::parse_document_source,
    store::VaultStore,
};
use chrono::Utc;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{debug, info};
use crate::sqlite::SqliteStore;

/// Vault manages the root directory layout:
///
///   <vault_root>/
///     .codex/
///       state.db       ← SQLite index
///       config.toml    ← sync + preferences
///     **/*.md          ← notes/documents
///
pub struct Vault {
    pub root: PathBuf,
    pub store: Arc<SqliteStore>,
    pub config: VaultConfig,
}

impl Vault {
    /// Open (or create) a vault rooted at `root`.
    pub fn open(root: &Path) -> Result<Self> {
        let codex_dir = root.join(".codex");
        fs::create_dir_all(&codex_dir)?;

        let db_path = codex_dir.join("state.db");
        let store = Arc::new(SqliteStore::open(&db_path)?);

        let config_path = codex_dir.join("config.toml");
        let config = if config_path.exists() {
            let raw = fs::read_to_string(&config_path)?;
            toml::from_str(&raw)?
        } else {
            let default_name = root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "codex".to_string());
            let cfg = VaultConfig {
                vault_name: default_name,
                sync: SyncConfig::None,
                appearance: Default::default(),
            };
            fs::write(&config_path, toml::to_string(&cfg)?)?;
            cfg
        };

        info!("Vault opened at {:?}, store ready", root);
        Ok(Self { root: root.to_owned(), store, config })
    }

    /// Index all markdown files under the vault root into the SQLite store.
    /// Skips `.codex/` directory. Idempotent — safe to call on every launch.
    pub fn reindex(&self) -> Result<(usize, Vec<String>)> {
        let mut indexed = 0;
        let mut errors = Vec::new();
        self.walk_markdown(&mut |path| {
            match self.index_file(path) {
                Ok(_) => indexed += 1,
                Err(e) => {
                    errors.push(format!("{}: {e}", path.display()));
                    debug!("index error: {e}");
                }
            }
        })?;
        info!("Reindex complete: {indexed} files, {} errors", errors.len());
        Ok((indexed, errors))
    }

    /// Parse and upsert a single markdown file into the store.
    pub fn index_file(&self, path: &Path) -> Result<()> {
        let raw = fs::read_to_string(path)?;
        let rel_path = path.strip_prefix(&self.root)?.to_owned();
        let (body, mut frontmatter, links) = parse_document_source(&raw);

        // Derive title: first H1 or filename stem
        let title = extract_h1(&body).unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled".to_string())
        });

        // Resolve stable ID: frontmatter > existing DB record > new UUID (written back to file)
        let existing = self.store.get_document_by_path(&rel_path)?;
        let id = frontmatter
            .id
            .map(DocumentId)
            .or_else(|| existing.as_ref().map(|d| d.id.clone()))
            .unwrap_or_else(DocumentId::new);

        // If the file has no id in frontmatter, write it back so it survives a DB wipe.
        if frontmatter.id.is_none() {
            frontmatter.id = Some(id.0);
            let new_fm = toml::to_string(&frontmatter).unwrap_or_default();
            let new_raw = format!("+++\n{new_fm}+++\n\n{body}");
            std::fs::write(path, &new_raw)?;
        }

        let now = Utc::now();
        let created_at = existing.as_ref().map(|d| d.created_at).unwrap_or(now);
        let doc = Document {
            id,
            path: rel_path,
            title,
            content: body,
            frontmatter,
            outgoing_links: links,
            created_at,
            updated_at: now,
        };
        self.store.save_document(&doc)?;
        Ok(())
    }

    /// Write updated markdown content back to disk and re-index.
    /// Preserves the existing frontmatter UUID so document identity is stable.
    pub fn save_document_content(&self, rel_path: &Path, content: &str) -> Result<()> {
        let abs_path = self.root.join(rel_path);
        if let Some(parent) = abs_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&abs_path, content)?;
        self.index_file(&abs_path)
    }

    /// Write updated markdown content to a new file path and index it.
    pub fn create_document(&self, rel_path: &Path, title: &str) -> Result<()> {
        let abs_path = self.root.join(rel_path);
        if let Some(parent) = abs_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if !abs_path.exists() {
            fs::write(&abs_path, format!("# {title}\n"))?;
        }
        self.index_file(&abs_path)
    }

    /// Write a new config to disk. Does not update `self.config` (the in-memory
    /// value is managed by callers via signals). Call this from the settings view.
    pub fn save_config(&self, config: &VaultConfig) -> Result<()> {
        let config_path = self.root.join(".codex").join("config.toml");
        fs::write(&config_path, toml::to_string_pretty(config)?)?;
        Ok(())
    }

    fn walk_markdown(&self, cb: &mut impl FnMut(&Path)) -> Result<()> {
        let codex_dir = self.root.join(".codex");
        for entry in walkdir::WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.path() != codex_dir && !is_hidden(e))
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.path().extension().map(|x| x == "md").unwrap_or(false)
            })
        {
            cb(entry.path());
        }
        Ok(())
    }
}

fn extract_h1(body: &str) -> Option<String> {
    for line in body.lines() {
        if let Some(stripped) = line.strip_prefix("# ") {
            let title = stripped.trim().to_string();
            if !title.is_empty() {
                return Some(title);
            }
        }
    }
    None
}

fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry.file_name().to_str().map(|s| s.starts_with('.')).unwrap_or(false)
}
