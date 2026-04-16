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
///       config.toml    ← sync + preferences
///     **/*.md          ← notes/documents
///
/// Local SQLite state is materialized outside the syncable vault whenever
/// `local_runtime.codex_index_db_path` (or its derived default) resolves to a
/// local app-state directory.
pub struct Vault {
    pub root: PathBuf,
    pub store: Arc<SqliteStore>,
    pub config: VaultConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub imported: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

enum ImportDisposition {
    Imported,
    Skipped,
}

impl Vault {
    /// Open (or create) a vault rooted at `root`.
    pub fn open(root: &Path) -> Result<Self> {
        let codex_dir = root.join(".codex");
        fs::create_dir_all(&codex_dir)?;

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
                local_runtime: Default::default(),
            };
            fs::write(&config_path, toml::to_string(&cfg)?)?;
            cfg
        };

        let db_path = resolve_index_db_path(root, &config.local_runtime);
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let store = Arc::new(SqliteStore::open(&db_path)?);

        info!("Vault opened at {:?}, store ready at {:?}", root, db_path);
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

    /// Import markdown documents from an external directory tree into this vault.
    /// The imported markdown becomes Codex canonical truth while preserving source provenance.
    pub fn import_markdown_tree(&self, source_root: &Path) -> Result<ImportReport> {
        let mut imported = 0usize;
        let mut skipped = 0usize;
        let mut errors = Vec::new();

        for entry in walkdir::WalkDir::new(source_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_hidden(e))
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.path().extension().map(|x| x == "md").unwrap_or(false)
            })
        {
            match self.import_markdown_file(source_root, entry.path()) {
                Ok(ImportDisposition::Imported) => imported += 1,
                Ok(ImportDisposition::Skipped) => skipped += 1,
                Err(err) => errors.push(format!("{}: {err}", entry.path().display())),
            }
        }

        Ok(ImportReport { imported, skipped, errors })
    }

    fn import_markdown_file(&self, source_root: &Path, source_path: &Path) -> Result<ImportDisposition> {
        let relative = source_path.strip_prefix(source_root)?;
        let destination = import_destination_path(relative);
        let absolute_destination = self.root.join(&destination);

        if absolute_destination.exists() {
            return Ok(ImportDisposition::Skipped);
        }

        let raw = fs::read_to_string(source_path)?;
        let (body, mut frontmatter, links) = parse_document_source(&raw);
        let title = extract_h1(&body).unwrap_or_else(|| {
            source_path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled".to_string())
        });
        let now = Utc::now();

        if frontmatter.id.is_none() {
            frontmatter.id = Some(DocumentId::new().0);
        }
        if frontmatter.source_format.is_none() {
            frontmatter.source_format = Some("markdown".into());
        }
        if frontmatter.source_path.is_none() {
            frontmatter.source_path = Some(source_path.display().to_string());
        }
        if frontmatter.imported_at.is_none() {
            frontmatter.imported_at = Some(now);
        }
        frontmatter.imported_reference = true;

        let document = Document {
            id: DocumentId(frontmatter.id.expect("frontmatter id set during import")),
            path: destination.clone(),
            title: title.clone(),
            content: body,
            frontmatter: frontmatter.clone(),
            outgoing_links: links,
            created_at: now,
            updated_at: now,
        };

        let canonical = canonical_document_source(&document);
        if let Some(parent) = absolute_destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&absolute_destination, canonical)?;
        self.index_file(&absolute_destination)?;
        Ok(ImportDisposition::Imported)
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

fn canonical_document_source(document: &Document) -> String {
    let frontmatter = toml::to_string(&document.frontmatter).unwrap_or_default();
    format!("+++\n{frontmatter}+++\n\n{}", document.content)
}

fn import_destination_path(relative_source_path: &Path) -> PathBuf {
    PathBuf::from("references/imported").join(relative_source_path)
}

fn resolve_index_db_path(root: &Path, runtime: &LocalRuntimeConfig) -> PathBuf {
    if let Some(path) = runtime.codex_index_db_path.as_ref().filter(|path| path.is_absolute()) {
        return path.clone();
    }

    let local_state_root = runtime
        .local_state_root
        .as_ref()
        .filter(|path| path.is_absolute())
        .cloned()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| root.join(".codex-local"))
        .join("codex");

    local_state_root.join("codex-index.db")
}

fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry.file_name().to_str().map(|s| s.starts_with('.')).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{import_destination_path, resolve_index_db_path, Vault};
    use codex_core::models::{LocalRuntimeConfig, MetadataValue};
    use tempfile::TempDir;

    #[test]
    fn uses_explicit_absolute_index_db_path_when_configured() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        let explicit = tmp.path().join("state/custom-index.db");

        let resolved = resolve_index_db_path(
            &root,
            &LocalRuntimeConfig {
                codex_index_db_path: Some(explicit.clone()),
                ..Default::default()
            },
        );

        assert_eq!(resolved, explicit);
    }

    #[test]
    fn derives_index_db_under_local_state_root_when_only_root_is_configured() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        let local_state_root = tmp.path().join("state-root");

        let resolved = resolve_index_db_path(
            &root,
            &LocalRuntimeConfig {
                local_state_root: Some(local_state_root.clone()),
                ..Default::default()
            },
        );

        assert_eq!(resolved, local_state_root.join("codex/codex-index.db"));
    }

    #[test]
    fn imports_markdown_tree_into_references_with_provenance_and_links() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        let source_root = tmp.path().join("obsidian");
        std::fs::create_dir_all(source_root.join("notes")).unwrap();
        std::fs::write(
            source_root.join("notes/design.md"),
            "+++
tags = [\"design\"]
owner = \"alpharius\"
+++

# Design

See [[roadmap]].\n",
        )
        .unwrap();

        let vault = Vault::open(&vault_root).unwrap();
        let report = vault.import_markdown_tree(&source_root).unwrap();
        assert_eq!(report.imported, 1);
        assert!(report.errors.is_empty());

        let imported_rel = import_destination_path(std::path::Path::new("notes/design.md"));
        let imported_doc = vault.store.get_document_by_path(&imported_rel).unwrap().unwrap();
        assert_eq!(imported_doc.title, "Design");
        assert_eq!(imported_doc.outgoing_links.len(), 1);
        assert_eq!(imported_doc.outgoing_links[0].target, "roadmap");
        assert_eq!(imported_doc.frontmatter.source_format.as_deref(), Some("markdown"));
        assert_eq!(
            imported_doc.frontmatter.source_path.as_deref(),
            Some(source_root.join("notes/design.md").display().to_string().as_str())
        );
        assert!(imported_doc.frontmatter.imported_reference);
        assert!(imported_doc.frontmatter.id.is_some());
        assert_eq!(
            imported_doc.frontmatter.metadata.get("owner"),
            Some(&MetadataValue::String("alpharius".into()))
        );

        let imported_meta = vault.store.get_document_by_path(&imported_rel).unwrap().unwrap();
        assert_eq!(imported_meta.path, imported_rel);
    }
}
