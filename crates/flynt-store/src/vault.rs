use anyhow::{Context, Result};
use flynt_core::{
    datum::{Entity, ProjectView},
    models::*,
    parser::parse_document_source,
    store::VaultStore,
};
use chrono::Utc;
use comrak::{markdown_to_html, Options};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{debug, info, warn};
use uuid::Uuid;
use crate::sqlite::SqliteStore;
use crate::sync::ProjectGit;
use crate::task_file;

/// Vault manages the root directory layout:
///
///   <vault_root>/
///     .flynt/
///       config.toml    ← sync + preferences
///     **/*.md          ← notes/documents
///
/// Local SQLite state is materialized outside the syncable vault whenever
/// `local_runtime.flynt_index_db_path` (or its derived default) resolves to a
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationExportReport {
    pub exported: usize,
    pub skipped_private: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicationManifest {
    pub generated_at: String,
    pub documents: Vec<PublicationManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicationManifestEntry {
    pub title: String,
    pub slug: String,
    pub source_path: PathBuf,
    pub output_path: PathBuf,
    pub tags: Vec<String>,
    pub visibility: PublicationVisibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedDocument {
    pub source_path: PathBuf,
    pub output_path: PathBuf,
    pub slug: String,
    pub title: String,
}

/// Report of a project flush operation.
#[derive(Debug, Clone, Default)]
pub struct FlushReport {
    pub tasks_flushed: usize,
    pub files_removed: usize,
    pub commit_oid: Option<String>,
}

impl FlushReport {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.tasks_flushed > 0 {
            parts.push(format!("{} tasks", self.tasks_flushed));
        }
        if self.files_removed > 0 {
            parts.push(format!("{} removed", self.files_removed));
        }
        if parts.is_empty() {
            "no changes".into()
        } else {
            parts.join(", ")
        }
    }
}

enum ImportDisposition {
    Imported,
    Skipped,
}

impl Vault {
    /// Open (or create) a vault rooted at `root`.
    pub fn open(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;

        // Auto-migrate from .codex/ → .flynt/ (pre-Flynt vaults)
        let old_dir = root.join(".codex");
        let flynt_dir = root.join(".flynt");
        if old_dir.exists() && !flynt_dir.exists() {
            info!("Migrating vault config: .codex/ → .flynt/");
            if let Err(e) = fs::rename(&old_dir, &flynt_dir) {
                tracing::warn!("Could not rename .codex/ to .flynt/: {e} — will use .codex/ as-is");
                // Fall through: flynt_dir won't exist, so create_dir_all below will create it
            }
        }
        // Also migrate .codex-local/ → .flynt-local/
        let old_local = root.join(".codex-local");
        let new_local = root.join(".flynt-local");
        if old_local.exists() && !new_local.exists() {
            info!("Migrating local state: .codex-local/ → .flynt-local/");
            let _ = fs::rename(&old_local, &new_local);
        }

        fs::create_dir_all(&flynt_dir)?;

        let config_path = flynt_dir.join("config.toml");
        let config = if config_path.exists() {
            let raw = fs::read_to_string(&config_path)?;
            toml::from_str(&raw)?
        } else {
            let default_name = root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Flynt".to_string());

            let indexing = if looks_like_code_repo(root) {
                info!("Code repo detected at {:?} — defaulting write_frontmatter to false", root);
                IndexingConfig { write_frontmatter: false, scopes: Vec::new() }
            } else {
                IndexingConfig::default()
            };

            let cfg = VaultConfig {
                vault_name: default_name,
                sync: SyncConfig::None,
                appearance: Default::default(),
                local_runtime: Default::default(),
                publication: Default::default(),
                security: Default::default(),
                indexing,
                visualization: Default::default(),
            };
            fs::write(&config_path, toml::to_string(&cfg)?)?;
            cfg
        };

        // Ensure .gitignore exists so local state is never committed
        let gitignore = root.join(".gitignore");
        if !gitignore.exists() {
            if let Err(e) = fs::write(&gitignore, ".flynt-local/\n.DS_Store\n*.swp\n*~\n") {
                tracing::warn!("Could not create .gitignore at {}: {e} — local state may be committed if git sync is enabled", gitignore.display());
            }
        }

        let db_path = resolve_index_db_path(root, &config.local_runtime);
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let store = Arc::new(SqliteStore::open(&db_path)?);

        info!("Vault opened at {:?}, store ready at {:?}", root, db_path);
        let vault = Self { root: root.to_owned(), store, config };

        // Migration: every task becomes a file. Legacy sqlite-only tasks
        // get a `.md` written under `Tasks/<board-slug>/`. Idempotent —
        // tasks with task_file_path already set are skipped.
        if let Err(e) = vault.migrate_tasks_to_files() {
            warn!("task→file migration failed (continuing): {e}");
        }
        Ok(vault)
    }

    /// Index all markdown files under the vault root into the SQLite store.
    /// Skips `.flynt/` directory. Idempotent — safe to call on every launch.
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

        // After indexing documents, discover git-backed projects and reindex
        // their task files into SQLite.
        self.reindex_all_projects(&mut errors);

        info!("Reindex complete: {indexed} files, {} errors", errors.len());
        Ok((indexed, errors))
    }

    /// Discover all project entities with git_backing and reindex their task files.
    fn reindex_all_projects(&self, errors: &mut Vec<String>) {
        use flynt_core::datum::EntityKind;

        let projects = match self.store.list_entities_by_kind(&EntityKind::Project) {
            Ok(p) => p,
            Err(e) => {
                errors.push(format!("failed to list projects: {e}"));
                return;
            }
        };

        for meta in &projects {
            let doc = match self.store.get_document(&meta.id) {
                Ok(Some(d)) => d,
                _ => continue,
            };
            let entity = match &doc.entity {
                Some(e) => e,
                None => continue,
            };
            let view = match ProjectView::from_entity(entity) {
                Some(v) => v,
                None => continue,
            };
            if view.git_backing().is_none() {
                continue;
            }

            match self.reindex_project(entity.id) {
                Ok(n) => {
                    if n > 0 {
                        info!("Reindexed {n} tasks for project '{}'", view.title());
                    }
                }
                Err(e) => {
                    errors.push(format!("project '{}': {e}", view.title()));
                }
            }
        }
    }

    /// Parse and upsert a single markdown file into the store.
    pub fn index_file(&self, path: &Path) -> Result<()> {
        let raw = fs::read_to_string(path)?;
        let rel_path = path.strip_prefix(&self.root)?.to_owned();
        let (body, mut frontmatter, links) = parse_document_source(&raw);

        // Derive title: H1 > frontmatter title > filename stem
        let title = extract_h1(&body)
            .or_else(|| frontmatter.title.clone())
            .unwrap_or_else(|| {
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
        // Scope-aware: only write when the file's path is in a managed scope
        // (or the vault-wide default allows it).
        if frontmatter.id.is_none() {
            frontmatter.id = Some(id.0);
            if self.config.indexing.should_write_frontmatter(&rel_path) {
                if frontmatter.kind.is_none() {
                    if let Some(scope) = self.config.indexing.scope_for_path(&rel_path) {
                        if let Some(ref k) = scope.kind {
                            frontmatter.kind = Some(k.clone());
                        }
                    }
                }
                let new_fm = toml::to_string(&frontmatter).unwrap_or_default();
                let new_raw = format!("+++\n{new_fm}+++\n\n{body}");
                std::fs::write(path, &new_raw)?;
            }
        }

        let now = Utc::now();
        let created_at = existing.as_ref().map(|d| d.created_at).unwrap_or(now);
        let entity = toml::Value::try_from(&frontmatter)
            .ok()
            .and_then(|v| flynt_core::datum::Entity::from_frontmatter(&v));
        let doc = Document {
            id,
            path: rel_path,
            title,
            content: body,
            frontmatter,
            outgoing_links: links,
            created_at,
            updated_at: now,
            entity,
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

    /// Set or clear the `kind` field in a document's TOML frontmatter.
    /// Pass `None` to remove the kind (revert to plain document).
    /// Refuses to modify Discoverable files (not in a managed scope).
    pub fn set_document_kind(&self, rel_path: &Path, kind: Option<&str>) -> Result<()> {
        if !self.config.indexing.should_write_frontmatter(rel_path) {
            anyhow::bail!(
                "Cannot set kind on discoverable file {:?} — configure an indexing scope to manage this path",
                rel_path,
            );
        }
        let abs_path = self.root.join(rel_path);
        let raw = fs::read_to_string(&abs_path)?;

        let updated = if raw.starts_with("+++") {
            // Find the closing +++ by scanning byte offsets directly
            // (handles both LF and CRLF line endings)
            let first_newline = raw.find('\n').ok_or_else(|| anyhow::anyhow!("Malformed frontmatter"))?;
            let search_start = first_newline + 1;
            let mut closing_pos = None;
            for (i, line) in raw[search_start..].split('\n').enumerate() {
                let trimmed = line.trim_end_matches('\r').trim();
                if trimmed == "+++" {
                    // Calculate byte offset from start of raw
                    let offset: usize = raw[search_start..].split('\n')
                        .take(i)
                        .map(|l| l.len() + 1) // +1 for the \n
                        .sum();
                    closing_pos = Some(search_start + offset);
                    break;
                }
            }
            let closing_pos = closing_pos.ok_or_else(|| anyhow::anyhow!("Malformed frontmatter: no closing +++"))?;

            let fm_text = &raw[first_newline + 1..closing_pos];
            let closing_line_end = raw[closing_pos..].find('\n')
                .map(|p| closing_pos + p + 1)
                .unwrap_or(raw.len());
            let after_fm = &raw[closing_line_end..];

            // Remove existing kind line — match only `kind = ` or `kind=` at the
            // start of the line (after optional whitespace), not inside [data] tables
            let mut new_fm = String::new();
            let mut in_table = false;
            for line in fm_text.lines() {
                let stripped = line.trim_start();
                // Track TOML table headers
                if stripped.starts_with('[') {
                    in_table = true;
                }
                // Only remove top-level `kind` key, not inside [data] or other tables
                if !in_table && (stripped.starts_with("kind =") || stripped.starts_with("kind=")) {
                    continue; // drop old kind
                }
                new_fm.push_str(line);
                new_fm.push('\n');
            }
            // Insert new kind if provided (at top level, after first line)
            if let Some(k) = kind {
                let insert_pos = new_fm.find('\n').map(|p| p + 1).unwrap_or(0);
                new_fm.insert_str(insert_pos, &format!("kind = \"{k}\"\n"));
            }
            format!("+++\n{new_fm}+++\n{after_fm}")
        } else {
            // No frontmatter — prepend one
            match kind {
                Some(k) => format!("+++\nkind = \"{k}\"\n+++\n\n{raw}"),
                None => raw, // nothing to do
            }
        };

        fs::write(&abs_path, &updated)?;
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

    /// Persist an internal agent communication as a canonical markdown reference document.
    pub fn store_agent_communication(
        &self,
        channel: &str,
        title: &str,
        content: &str,
    ) -> Result<PathBuf> {
        let now = Utc::now();
        let slug = slugify_title(title);
        let relative_path = PathBuf::from("references/comms")
            .join(channel)
            .join(format!("{}-{}.md", now.format("%Y%m%d%H%M%S"), slug));
        let absolute_path = self.root.join(&relative_path);

        let mut frontmatter = Frontmatter::default();
        frontmatter.id = Some(DocumentId::new().0);
        frontmatter.title = Some(title.to_string());
        frontmatter.source_format = Some("omegon_comm".into());
        frontmatter.source_path = Some(format!("omegon://{channel}"));
        frontmatter.imported_at = Some(now);
        frontmatter.imported_reference = true;
        frontmatter
            .metadata
            .insert("channel".into(), MetadataValue::String(channel.to_string()));
        frontmatter
            .metadata
            .insert("kind".into(), MetadataValue::String("agent_communication".into()));

        let document = Document {
            id: DocumentId(frontmatter.id.expect("frontmatter id set for communication")),
            path: relative_path.clone(),
            title: title.to_string(),
            content: content.to_string(),
            frontmatter,
            outgoing_links: parse_document_source(content).2,
            created_at: now,
            updated_at: now,
            entity: None,
        };

        let canonical = canonical_document_source(&document);
        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&absolute_path, canonical)?;
        self.index_file(&absolute_path)?;
        Ok(relative_path)
    }

    /// Persist a durable memory fact as a canonical markdown knowledge artifact.
    pub fn store_memory_fact(
        &self,
        topic: &str,
        title: &str,
        content: &str,
    ) -> Result<PathBuf> {
        let now = Utc::now();
        let slug = slugify_title(title);
        let relative_path = PathBuf::from("ai/memory")
            .join(slugify_title(topic))
            .join(format!("{}-{}.md", now.format("%Y%m%d%H%M%S"), slug));
        let absolute_path = self.root.join(&relative_path);

        let mut frontmatter = Frontmatter::default();
        frontmatter.id = Some(DocumentId::new().0);
        frontmatter.title = Some(title.to_string());
        frontmatter.source_format = Some("omegon_memory".into());
        frontmatter.source_path = Some(format!("omegon://memory/{topic}"));
        frontmatter.imported_at = Some(now);
        frontmatter.imported_reference = true;
        frontmatter
            .metadata
            .insert("topic".into(), MetadataValue::String(topic.to_string()));
        frontmatter
            .metadata
            .insert("kind".into(), MetadataValue::String("memory_fact".into()));

        let document = Document {
            id: DocumentId(frontmatter.id.expect("frontmatter id set for memory fact")),
            path: relative_path.clone(),
            title: title.to_string(),
            content: content.to_string(),
            frontmatter,
            outgoing_links: parse_document_source(content).2,
            created_at: now,
            updated_at: now,
            entity: None,
        };

        let canonical = canonical_document_source(&document);
        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&absolute_path, canonical)?;
        self.index_file(&absolute_path)?;
        Ok(relative_path)
    }

    /// Import markdown documents from an external directory tree into this vault.
    /// The imported markdown becomes Flynt canonical truth while preserving source provenance.
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
        if frontmatter.title.is_none() {
            frontmatter.title = Some(title.clone());
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
            entity: None,
        };

        let canonical = canonical_document_source(&document);
        if let Some(parent) = absolute_destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&absolute_destination, canonical)?;
        self.index_file(&absolute_destination)?;
        Ok(ImportDisposition::Imported)
    }

    /// Export public knowledge documents into a normalized publish tree suitable for a static site generator.
    pub fn export_publication_tree(&self, output_root: &Path) -> Result<PublicationExportReport> {
        let mut exported = 0usize;
        let mut skipped_private = 0usize;
        let mut errors = Vec::new();
        let mut manifest_entries = Vec::new();
        let mut seen_slugs = std::collections::BTreeSet::new();

        for document in self.store.list_documents()? {
            match self.export_published_document(&document.path, output_root) {
                Ok(Some(published)) => {
                    if !seen_slugs.insert(published.slug.clone()) {
                        errors.push(format!(
                            "{}: duplicate publication slug '{}'",
                            document.path.display(),
                            published.slug
                        ));
                        continue;
                    }
                    exported += 1;
                    let doc_obj = self.store.get_document_by_path(&document.path)?;
                    let vis = doc_obj.as_ref()
                        .map(|d| effective_publication_visibility(d, &self.config.publication))
                        .unwrap_or(PublicationVisibility::Public);
                    manifest_entries.push(PublicationManifestEntry {
                        title: published.title,
                        slug: published.slug,
                        source_path: published.source_path,
                        output_path: published.output_path,
                        tags: document.tags,
                        visibility: vis,
                    });
                }
                Ok(None) => skipped_private += 1,
                Err(err) => errors.push(format!("{}: {err}", document.path.display())),
            }
        }

        fs::create_dir_all(output_root)?;

        // Also export project boards if any projects are publishable
        self.export_project_boards(output_root, &mut manifest_entries, &mut exported, &mut errors);

        let manifest = PublicationManifest {
            generated_at: Utc::now().to_rfc3339(),
            documents: manifest_entries,
        };
        fs::write(
            output_root.join("manifest.json"),
            serde_json::to_string_pretty(&manifest)?,
        )?;

        // Generate NomadNet index page listing all published documents.
        let mut index_mu = String::from(">`!Flynt`\n\n");
        for entry in &manifest.documents {
            if entry.visibility != PublicationVisibility::Unlisted {
                index_mu.push_str(&format!(
                    "`[{}`/page/{}.mu]\n",
                    entry.title, entry.slug
                ));
            }
        }
        fs::write(output_root.join("index.mu"), &index_mu)?;

        Ok(PublicationExportReport { exported, skipped_private, errors })
    }

    /// Export git-backed project boards as static markdown + HTML.
    fn export_project_boards(
        &self,
        output_root: &Path,
        manifest_entries: &mut Vec<PublicationManifestEntry>,
        exported: &mut usize,
        errors: &mut Vec<String>,
    ) {
        use flynt_core::datum::EntityKind;

        let projects = match self.store.list_entities_by_kind(&EntityKind::Project) {
            Ok(p) => p,
            Err(e) => {
                errors.push(format!("failed to list projects for publication: {e}"));
                return;
            }
        };

        for meta in &projects {
            let doc = match self.store.get_document(&meta.id) {
                Ok(Some(d)) => d,
                _ => continue,
            };

            // Only export projects that have publication enabled
            if !doc.frontmatter.publication.enabled {
                continue;
            }
            let visibility = effective_publication_visibility(&doc, &self.config.publication);
            if visibility == PublicationVisibility::Private {
                continue;
            }

            let entity = match &doc.entity {
                Some(e) => e,
                None => continue,
            };
            let view = match ProjectView::from_entity(entity) {
                Some(v) => v,
                None => continue,
            };

            // Find boards for this project
            let boards = match self.store.list_boards() {
                Ok(b) => b,
                Err(_) => continue,
            };
            let project_boards: Vec<_> = boards
                .into_iter()
                .filter(|b| b.project_id == Some(entity.id))
                .collect();

            if project_boards.is_empty() {
                continue;
            }

            let slug = format!("project-{}", slugify_title(view.title()));
            let board_md_path = output_root.join(format!("{slug}.md"));
            let board_html_path = output_root.join(format!("{slug}.html"));
            let board_mu_path = output_root.join(format!("{slug}.mu"));

            match self.render_project_board_markdown(&view, &project_boards) {
                Ok(md) => {
                    let html = self.render_project_board_html(view.title(), &md);
                    let micron = markdown_to_micron(&md);
                    if let Err(e) = fs::write(&board_md_path, &md) {
                        errors.push(format!("project '{}': {e}", view.title()));
                        continue;
                    }
                    if let Err(e) = fs::write(&board_html_path, &html) {
                        errors.push(format!("project '{}' html: {e}", view.title()));
                        continue;
                    }
                    if let Err(e) = fs::write(&board_mu_path, &micron) {
                        errors.push(format!("project '{}' micron: {e}", view.title()));
                        continue;
                    }
                    manifest_entries.push(PublicationManifestEntry {
                        title: format!("{} — Board", view.title()),
                        slug: slug.clone(),
                        source_path: doc.path.clone(),
                        output_path: board_md_path,
                        tags: vec!["project".into(), "board".into()],
                        visibility: PublicationVisibility::Public,
                    });
                    *exported += 1;
                }
                Err(e) => {
                    errors.push(format!("project '{}': {e}", view.title()));
                }
            }
        }
    }

    fn render_project_board_markdown(
        &self,
        view: &ProjectView,
        boards: &[Board],
    ) -> Result<String> {
        use flynt_core::store::TaskFilter;

        let mut md = format!("# {}\n\n", view.title());
        md.push_str(&format!("**Status:** {}\n\n", view.status()));

        for board in boards {
            md.push_str(&format!("## {}\n\n", board.name));

            let tasks = self.store.list_tasks(&TaskFilter {
                board_id: Some(board.id.clone()),
                ..Default::default()
            })?;

            for col in &board.columns {
                let col_tasks: Vec<_> = tasks.iter()
                    .filter(|t| t.column == col.name && t.status != TaskStatus::Archived)
                    .collect();

                md.push_str(&format!("### {} ({})\n\n", col.name, col_tasks.len()));

                if col_tasks.is_empty() {
                    md.push_str("*No tasks*\n\n");
                } else {
                    for task in &col_tasks {
                        let priority = match task.priority {
                            Priority::Critical => " **CRITICAL**",
                            Priority::High => " **HIGH**",
                            Priority::Low => " *low*",
                            Priority::Medium => "",
                        };
                        let status_icon = match task.status {
                            TaskStatus::Done => "- [x]",
                            _ => "- [ ]",
                        };
                        md.push_str(&format!("{status_icon} {}{priority}\n", task.title));
                    }
                    md.push('\n');
                }
            }
        }

        Ok(md)
    }

    fn render_project_board_html(&self, title: &str, markdown: &str) -> String {
        let mut options = Options::default();
        options.extension.table = true;
        options.extension.strikethrough = true;
        options.extension.tasklist = true;
        let html = markdown_to_html(markdown, &options);
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title><style>body{{max-width:860px;margin:0 auto;padding:40px 24px;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;line-height:1.6;background:#0b0f16;color:#d7e0ea}}a{{color:#4cc9f0}}h1{{border-bottom:1px solid #1e293b;padding-bottom:8px}}h2{{color:#94a3b8}}h3{{color:#64748b;font-size:0.95em}}ul{{list-style:none;padding-left:0}}li{{padding:4px 0}}</style></head><body>{html}</body></html>"
        )
    }

    fn export_published_document(&self, relative_path: &Path, output_root: &Path) -> Result<Option<PublishedDocument>> {
        let Some(document) = self.store.get_document_by_path(relative_path)? else {
            return Ok(None);
        };
        let visibility = effective_publication_visibility(&document, &self.config.publication);
        if !document.frontmatter.publication.enabled || visibility == PublicationVisibility::Private {
            return Ok(None);
        }

        let slug = publication_slug(&document);
        let output_path = output_root.join(format!("{slug}.md"));
        let html_path = output_root.join(format!("{slug}.html"));
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let manifest = PublishedDocument {
            source_path: document.path.clone(),
            output_path: output_path.clone(),
            slug: slug.clone(),
            title: document.title.clone(),
        };

        let published_markdown = render_published_markdown(self, &document)?;
        let published_html = render_published_html(self, &document)?;
        let published_micron = render_published_micron(self, &document)?;
        let micron_path = output_root.join(format!("{slug}.mu"));
        fs::write(&output_path, published_markdown)?;
        fs::write(&html_path, published_html)?;
        fs::write(&micron_path, published_micron)?;
        Ok(Some(manifest))
    }

    // ── Project-aware task operations ──────────────────────────────────────

    /// Save a task that belongs to a project board.
    /// Sets the project_id, saves to SQLite, and writes the task file to disk
    /// if the project has git backing.
    pub fn save_project_task(&self, task: &Task, project_id: &Uuid) -> Result<()> {
        self.store.save_task(task)?;
        self.store.set_task_project(&task.id, project_id)?;

        // Write task file to disk if project has git backing
        if let Ok(entity) = self.resolve_project_entity(project_id) {
            if let Some(view) = ProjectView::from_entity(&entity) {
                if let Some(backing) = view.git_backing() {
                    let md = task_file::serialize_task_to_markdown(task, project_id);
                    let rel_path = task_file::task_file_path(backing.sub_path(), &task.id);
                    let abs_path = backing.repo_root(&self.root).join(&rel_path);
                    if let Some(parent) = abs_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(&abs_path, &md)?;

                    // Mark as committed immediately (written to disk)
                    let now = Utc::now();
                    self.store.mark_committed(&[task.id.clone()], &[], now)?;
                }
            }
        }
        Ok(())
    }

    /// Persist a task: write `.md` file to disk + sqlite + remember the
    /// path for future renames. Every task becomes a file in the vault.
    ///
    /// File path: `Tasks/<board-slug>/<title-slug>.md`. If the title
    /// changed since last save (slug differs from stored path), the
    /// previous file is removed so the rename leaves no orphan. On
    /// title-collision within a board, a numeric suffix is appended.
    pub fn save_any_task(&self, task: &Task) -> Result<PathBuf> {
        // Step 1: resolve the desired path from the current title +
        // board name. Board name is needed for the directory.
        let board_slug = match self.store.get_board(&task.board_id)? {
            Some(b) => slugify_title(&b.name),
            None => "unfiled".into(),
        };
        let title_slug = slugify_title(&task.title);
        let desired_dir = self.root.join("Tasks").join(&board_slug);
        let mut desired_rel = PathBuf::from("Tasks").join(&board_slug).join(format!("{title_slug}.md"));

        // Step 2: if there's an existing file path stored for this task
        // and the title slug changed, plan a rename. The previous file
        // gets removed after the new one is written so a crash mid-
        // operation leaves the operator with both rather than neither.
        let prior = self.store.task_file_path(&task.id)?;
        let prior_abs = prior.as_ref().map(|p| self.root.join(p));

        // Step 3: collision handling. Two tasks on the same board with
        // the same title slug get -2, -3, ... suffix. Prior path is
        // its own file → not a collision.
        let mut abs = self.root.join(&desired_rel);
        let mut suffix = 2;
        while abs.exists() && Some(&abs) != prior_abs.as_ref() {
            // Confirm it's a different task (don't treat our own file
            // as a collision if it happens to live at this exact path
            // already).
            if let Ok(raw) = std::fs::read_to_string(&abs)
                && let Ok(existing) = task_file::parse_task_from_markdown(&raw)
                && existing.id == task.id
            {
                break;
            }
            desired_rel = PathBuf::from("Tasks")
                .join(&board_slug)
                .join(format!("{title_slug}-{suffix}.md"));
            abs = self.root.join(&desired_rel);
            suffix += 1;
        }

        // Step 4: write file. project_id passed to the serializer is
        // only used for the [data.project] frontmatter; for non-project
        // tasks we use the task id as a stand-in (the serializer needs
        // a uuid arg but doesn't fail on synthetic values).
        std::fs::create_dir_all(&desired_dir)
            .with_context(|| format!("create Tasks dir {}", desired_dir.display()))?;
        let project_id_for_fm = uuid::Uuid::nil();
        let md = task_file::serialize_task_to_markdown(task, &project_id_for_fm);
        std::fs::write(&abs, &md)
            .with_context(|| format!("write task file {}", abs.display()))?;

        // Step 5: remove the prior file if we renamed.
        if let Some(prior_path) = prior_abs.as_ref()
            && prior_path != &abs
            && prior_path.exists()
        {
            let _ = std::fs::remove_file(prior_path);
        }

        // Step 6: persist sqlite + remember the path.
        self.store.save_task(task)?;
        let rel_str = desired_rel.to_string_lossy().to_string();
        self.store.set_task_file_path(&task.id, &rel_str)?;

        Ok(desired_rel)
    }

    /// One-shot migration: any task in sqlite without a `task_file_path`
    /// gets a markdown file written and its path recorded. Called from
    /// `Vault::open` so freshly-rebuilt vaults converge to the
    /// every-task-is-a-file invariant on next launch.
    fn migrate_tasks_to_files(&self) -> Result<usize> {
        let unfiled = self.store.tasks_without_file()?;
        let mut migrated = 0;
        for tid in unfiled {
            let Some(task) = self.store.get_task(&tid)? else { continue };
            match self.save_any_task(&task) {
                Ok(_) => migrated += 1,
                Err(e) => warn!("migrate task {} to file: {e}", tid.0),
            }
        }
        if migrated > 0 {
            info!("migrated {migrated} task(s) from sqlite-only to .md files");
        }
        Ok(migrated)
    }

    /// Delete a task that belongs to a project. Records the deletion for
    /// git cleanup and removes the file from disk if it exists.
    pub fn delete_project_task(&self, task_id: &TaskId, project_id: &Uuid) -> Result<()> {
        // Record deletion for git tracking
        self.store.record_project_deletion(&task_id.0, "task", project_id)?;

        // Remove file from disk if project has git backing
        if let Ok(entity) = self.resolve_project_entity(project_id) {
            if let Some(view) = ProjectView::from_entity(&entity) {
                if let Some(backing) = view.git_backing() {
                    let rel_path = task_file::task_file_path(backing.sub_path(), task_id);
                    let abs_path = backing.repo_root(&self.root).join(&rel_path);
                    if abs_path.exists() {
                        fs::remove_file(&abs_path)?;
                    }
                }
            }
        }

        self.store.delete_task(task_id)?;
        Ok(())
    }

    // ── Project git backing ───────────────────────────────────────────────

    /// Flush dirty tasks and documents for a git-backed project to disk as
    /// markdown files. For ExternalRepo projects, also stages and commits.
    ///
    /// Returns a report of what was flushed.
    pub fn flush_project(&self, project_id: Uuid) -> Result<FlushReport> {
        let project_entity = self.resolve_project_entity(&project_id)?;
        let view = ProjectView::from_entity(&project_entity)
            .context("entity is not a project")?;
        let backing = view.git_backing()
            .context("project has no git_backing configured")?;
        let commit_config = view.commit_config();

        let data_root = backing.data_root(&self.root);

        // Ensure directories exist
        fs::create_dir_all(data_root.join("tasks"))?;

        let mut report = FlushReport::default();

        // 1. Write dirty tasks to disk
        let dirty_tasks = self.store.list_dirty_tasks(&project_id)?;
        let mut committed_task_ids = Vec::new();
        for task in &dirty_tasks {
            let md = task_file::serialize_task_to_markdown(task, &project_id);
            let rel_path = task_file::task_file_path(backing.sub_path(), &task.id);
            let abs_path = backing.repo_root(&self.root).join(&rel_path);
            if let Some(parent) = abs_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&abs_path, &md)?;
            committed_task_ids.push(task.id.clone());
            report.tasks_flushed += 1;
        }

        // 2. Handle pending deletions
        let deletions = self.store.list_pending_deletions(&project_id)?;
        let mut deleted_ids = Vec::new();
        for (entity_id, entity_kind) in &deletions {
            if entity_kind == "task" {
                let rel_path = task_file::task_file_path(
                    backing.sub_path(),
                    &TaskId(*entity_id),
                );
                let abs_path = backing.repo_root(&self.root).join(&rel_path);
                if abs_path.exists() {
                    fs::remove_file(&abs_path)?;
                    report.files_removed += 1;
                }
            }
            deleted_ids.push(*entity_id);
        }

        // 3. Mark as committed in SQLite
        let now = Utc::now();
        self.store.mark_committed(&committed_task_ids, &[], now)?;
        if !deleted_ids.is_empty() {
            self.store.mark_deletions_committed(&deleted_ids)?;
        }

        // 4. For ExternalRepo, also do the git commit
        if !backing.is_vault_repo() && (report.tasks_flushed > 0 || report.files_removed > 0) {
            let pg = ProjectGit::open(&backing, &self.root)?;
            let prefix = commit_config.message_prefix
                .unwrap_or_else(|| "[flynt]".into());
            let msg = format!("{prefix} flush {}", report.summary());
            if let Some(oid) = pg.commit(&msg)? {
                report.commit_oid = Some(oid.to_string());
            }
        }

        info!(
            "Flushed project {project_id}: {} tasks, {} removals",
            report.tasks_flushed, report.files_removed
        );
        Ok(report)
    }

    /// Re-index task files from a project's sub-path into SQLite.
    /// Called on vault open and after git pulls to sync disk -> DB.
    pub fn reindex_project(&self, project_id: Uuid) -> Result<usize> {
        let project_entity = self.resolve_project_entity(&project_id)?;
        let view = ProjectView::from_entity(&project_entity)
            .context("entity is not a project")?;
        let backing = view.git_backing()
            .context("project has no git_backing configured")?;

        let tasks_dir = backing.data_root(&self.root).join("tasks");

        if !tasks_dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        for entry in walkdir::WalkDir::new(&tasks_dir)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.path().extension().map(|x| x == "md").unwrap_or(false)
            })
        {
            let raw = fs::read_to_string(entry.path())?;
            match task_file::parse_task_from_markdown(&raw) {
                Ok(task) => {
                    self.store.save_task(&task)?;
                    count += 1;
                }
                Err(e) => {
                    warn!("Failed to parse task file {}: {e}", entry.path().display());
                }
            }
        }
        info!("Reindexed {count} tasks for project {project_id}");
        Ok(count)
    }

    /// Look up a project entity from SQLite by its UUID.
    fn resolve_project_entity(&self, project_id: &Uuid) -> Result<Entity> {
        let doc = self.store
            .get_document(&DocumentId(*project_id))?
            .context("project document not found in store")?;
        doc.entity.context("project document has no entity")
    }

    /// Write a new config to disk. Does not update `self.config` (the in-memory
    /// value is managed by callers via signals). Call this from the settings view.
    pub fn save_config(&self, config: &VaultConfig) -> Result<()> {
        let config_path = self.root.join(".flynt").join("config.toml");
        fs::write(&config_path, toml::to_string_pretty(config)?)?;
        Ok(())
    }

    // ── Rename document + update links ────────────────────────────────────────

    /// Rename a document: moves the file on disk, updates frontmatter title,
    /// and rewrites all wikilinks across the vault that pointed to the old name.
    /// Returns the number of files updated.
    pub fn rename_document(&self, old_path: &Path, new_title: &str) -> Result<usize> {
        use std::path::PathBuf;

        let abs_old = self.root.join(old_path);
        if !abs_old.exists() {
            anyhow::bail!("source file does not exist: {}", old_path.display());
        }

        // Read current content
        let raw = fs::read_to_string(&abs_old)?;

        // Derive the old title for link matching
        let (_body, old_fm, _links) = flynt_core::parser::parse_document_source(&raw);
        let old_title = old_fm.title.clone().unwrap_or_else(|| {
            old_path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });

        // Build the new filename from the new title
        let new_filename = format!("{}.md", new_title);
        let new_path = old_path.parent()
            .map(|p| p.join(&new_filename))
            .unwrap_or_else(|| PathBuf::from(&new_filename));

        // Update the frontmatter title in the document content
        let updated_content = if raw.contains("+++") {
            // Replace title in TOML frontmatter
            let new_raw = if let Some(title_line_start) = raw.find("title = \"") {
                let before = &raw[..title_line_start];
                let after_title = &raw[title_line_start..];
                if let Some(end_quote) = after_title[9..].find('"') {
                    format!("{}title = \"{}\"{}", before, new_title, &after_title[9 + end_quote + 1..])
                } else {
                    raw.clone()
                }
            } else {
                raw.clone()
            };
            new_raw
        } else {
            raw.clone()
        };

        // Write updated content to new path
        let abs_new = self.root.join(&new_path);
        fs::write(&abs_new, &updated_content)?;

        // Remove old file (if path changed)
        if old_path != new_path {
            fs::remove_file(&abs_old)?;
        }

        // Now scan all markdown files and update wikilinks
        let old_title_lower = old_title.to_lowercase();
        let old_stem = old_path.file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        let mut files_updated = 0;
        self.walk_markdown(&mut |path| {
            // Skip the renamed file itself
            let rel = path.strip_prefix(&self.root).unwrap_or(path);
            if rel == new_path { return; }

            let Ok(content) = fs::read_to_string(path) else { return; };

            // Check if this file contains wikilinks to the old name
            let mut new_content = content.clone();
            let mut changed = false;

            // Replace [[Old Title]] → [[New Title]]
            let patterns = [
                format!("[[{}]]", old_title),
                format!("[[{}]]", old_stem),
            ];
            for pat in &patterns {
                if new_content.contains(pat.as_str()) {
                    new_content = new_content.replace(
                        pat.as_str(),
                        &format!("[[{}]]", new_title),
                    );
                    changed = true;
                }
            }

            // Replace [[Old Title|display]] → [[New Title|display]]
            // and [[old_stem|display]] → [[New Title|display]]
            for old_ref in [&old_title, &old_stem.to_string()] {
                let pipe_prefix = format!("[[{}|", old_ref);
                while let Some(start) = new_content.find(&pipe_prefix) {
                    if let Some(end) = new_content[start..].find("]]") {
                        let display = &new_content[start + pipe_prefix.len()..start + end].to_string();
                        let replacement = format!("[[{}|{}]]", new_title, display);
                        new_content = format!(
                            "{}{}{}",
                            &new_content[..start],
                            replacement,
                            &new_content[start + end + 2..]
                        );
                        changed = true;
                    } else {
                        break;
                    }
                }
            }

            // Case-insensitive matching for wikilinks
            if !changed {
                // Try case-insensitive match
                let content_lower = new_content.to_lowercase();
                for old_ref in [&old_title_lower, &old_stem] {
                    let pat_lower = format!("[[{}]]", old_ref);
                    if content_lower.contains(&pat_lower) {
                        // Find and replace preserving surrounding case
                        let mut result = String::new();
                        let mut remaining = new_content.as_str();
                        let remaining_lower = content_lower.as_str();
                        let mut offset = 0;
                        while let Some(pos) = remaining_lower[offset..].find(&pat_lower) {
                            result.push_str(&remaining[..offset + pos]);
                            result.push_str(&format!("[[{}]]", new_title));
                            let skip = offset + pos + pat_lower.len();
                            remaining = &remaining[skip..];
                            offset = 0;
                            changed = true;
                        }
                        if changed {
                            result.push_str(remaining);
                            new_content = result;
                        }
                    }
                }
            }

            if changed {
                if fs::write(path, &new_content).is_ok() {
                    files_updated += 1;
                }
            }
        })?;

        // Reindex to update SQLite
        self.reindex()?;

        info!(
            "Renamed '{}' → '{}', updated {} file(s)",
            old_title, new_title, files_updated
        );

        Ok(files_updated)
    }

    // ── Tag management ───────────────────────────────────────────────────────

    /// List all unique tags across the vault with document counts.
    pub fn list_tags(&self) -> Result<Vec<(String, usize)>> {
        let docs = self.store.list_documents()?;
        let mut tag_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for doc in &docs {
            for tag in &doc.tags {
                *tag_counts.entry(tag.clone()).or_default() += 1;
            }
        }
        let mut tags: Vec<(String, usize)> = tag_counts.into_iter().collect();
        tags.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        Ok(tags)
    }

    /// Rename a tag across all documents in the vault.
    /// Parses frontmatter as structured data, modifies, re-serializes.
    /// Returns the number of files updated.
    pub fn rename_tag(&self, old_tag: &str, new_tag: &str) -> Result<usize> {
        let mut files_updated = 0;
        self.walk_markdown(&mut |path| {
            let Ok(content) = fs::read_to_string(path) else { return; };

            let (_body, fm, _links) = flynt_core::parser::parse_document_source(&content);
            if !fm.tags.iter().any(|t| t == old_tag) { return; }

            // Replace in the tags array by rebuilding it
            let mut new_tags = fm.tags.clone();
            for tag in &mut new_tags {
                if tag == old_tag { *tag = new_tag.to_string(); }
            }
            // Deduplicate
            new_tags.sort();
            new_tags.dedup();

            if let Some(new_content) = replace_frontmatter_tags(&content, &new_tags) {
                if fs::write(path, &new_content).is_ok() {
                    files_updated += 1;
                }
            }
        })?;
        self.reindex()?;
        info!("Renamed tag '{}' → '{}', updated {} file(s)", old_tag, new_tag, files_updated);
        Ok(files_updated)
    }

    /// Delete a tag from all documents. Returns the number of files updated.
    pub fn delete_tag(&self, tag: &str) -> Result<usize> {
        let mut files_updated = 0;
        self.walk_markdown(&mut |path| {
            let Ok(content) = fs::read_to_string(path) else { return; };

            let (_body, fm, _links) = flynt_core::parser::parse_document_source(&content);
            if !fm.tags.iter().any(|t| t == tag) { return; }

            let new_tags: Vec<String> = fm.tags.into_iter().filter(|t| t != tag).collect();

            if let Some(new_content) = replace_frontmatter_tags(&content, &new_tags) {
                if fs::write(path, &new_content).is_ok() {
                    files_updated += 1;
                }
            }
        })?;
        self.reindex()?;
        info!("Deleted tag '{}', updated {} file(s)", tag, files_updated);
        Ok(files_updated)
    }

    /// Merge multiple tags into one. Returns the number of files updated.
    pub fn merge_tags(&self, source_tags: &[&str], target_tag: &str) -> Result<usize> {
        let mut total = 0;
        for src in source_tags {
            if *src != target_tag {
                total += self.rename_tag(src, target_tag)?;
            }
        }
        Ok(total)
    }

    // ── Notifications (git-synced) ──────────────────────────────────────────

    /// Write a notification to the pending queue. Git sync will push it to other devices.
    pub fn push_notification(&self, notif: &flynt_core::models::Notification) -> Result<()> {
        let dir = self.root.join(".flynt/notifications/pending");
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", notif.id));
        fs::write(&path, serde_json::to_string_pretty(notif)?)?;
        Ok(())
    }

    /// Read all pending notifications (not yet delivered on this device).
    pub fn pending_notifications(&self) -> Result<Vec<flynt_core::models::Notification>> {
        let dir = self.root.join(".flynt/notifications/pending");
        if !dir.exists() { return Ok(vec![]); }
        let mut result = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.path().extension().map(|e| e == "json").unwrap_or(false) {
                match fs::read_to_string(entry.path()) {
                    Ok(raw) => match serde_json::from_str(&raw) {
                        Ok(notif) => result.push(notif),
                        Err(e) => warn!("malformed notification {}: {e}", entry.path().display()),
                    },
                    Err(e) => warn!("unreadable notification {}: {e}", entry.path().display()),
                }
            }
        }
        Ok(result)
    }

    /// Mark a notification as delivered — moves from pending to delivered.
    /// Safe against concurrent access: ignores missing files.
    pub fn mark_notification_delivered(&self, id: &uuid::Uuid) -> Result<()> {
        let pending = self.root.join(format!(".flynt/notifications/pending/{id}.json"));
        let delivered_dir = self.root.join(".flynt/notifications/delivered");
        fs::create_dir_all(&delivered_dir)?;

        // Read + parse; if file is already gone, another device handled it
        let raw = match fs::read_to_string(&pending) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };

        let mut notif: flynt_core::models::Notification = serde_json::from_str(&raw)?;
        notif.delivered_at = Some(chrono::Utc::now());

        // Write delivered first, then remove pending (crash-safe order)
        let dest = delivered_dir.join(format!("{id}.json"));
        fs::write(&dest, serde_json::to_string_pretty(&notif)?)?;

        // Remove pending — ignore NotFound (race with another device)
        match fs::remove_file(&pending) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Scan tasks for decay and due date notifications. Returns only NEW notifications
    /// (skips tasks that already have a pending or delivered notification).
    pub fn check_task_notifications(&self) -> Result<Vec<flynt_core::models::Notification>> {
        use flynt_core::models::*;

        let tasks = self.store.list_tasks(&flynt_core::store::TaskFilter::default())?;
        let vault_name = self.config.vault_name.clone();
        let today = chrono::Local::now().date_naive();

        // Collect task IDs that already have pending/delivered notifications to avoid duplicates
        let existing_task_ids: std::collections::HashSet<String> = self
            .pending_notifications()
            .unwrap_or_default()
            .iter()
            .filter_map(|n| n.task_id.as_ref().map(|t| t.0.to_string()))
            .collect();

        let mut notifications = Vec::new();

        for task in &tasks {
            if matches!(task.status, TaskStatus::Done | TaskStatus::Archived) {
                continue;
            }
            if existing_task_ids.contains(&task.id.0.to_string()) {
                continue; // already notified
            }

            // Due date notification — task due today or overdue
            if let Some(due) = task.due_date {
                if due <= today {
                    let days = (today - due).num_days();
                    let body = if days == 0 {
                        format!("\"{}\" is due today", task.title)
                    } else {
                        format!("\"{}\" is {} day(s) overdue", task.title, days)
                    };
                    notifications.push(
                        Notification::new(NotificationKind::DueDate, &task.title, body, &vault_name)
                            .for_task(task.id.clone()),
                    );
                }
            }

            // Decay notification — task is fading
            if task.is_fading() && !task.should_auto_archive() {
                notifications.push(
                    Notification::new(
                        NotificationKind::Decay,
                        &task.title,
                        format!("\"{}\" is losing relevance. Touch it or let it archive.", task.title),
                        &vault_name,
                    )
                    .for_task(task.id.clone()),
                );
            }
        }

        Ok(notifications)
    }

    fn walk_markdown(&self, cb: &mut impl FnMut(&Path)) -> Result<()> {
        let flynt_dir = self.root.join(".flynt");
        for entry in walkdir::WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.path() != flynt_dir && !is_hidden(e))
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
    let body = document.content.trim_end();
    format!("+++\n{frontmatter}\n+++\n\n{body}\n")
}

fn slugify_title(title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '-' })
        .collect();
    let slug = slug
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() { "note".into() } else { slug }
}

fn import_destination_path(relative_source_path: &Path) -> PathBuf {
    PathBuf::from("references/imported").join(relative_source_path)
}

fn publication_slug(document: &Document) -> String {
    document
        .frontmatter
        .publication
        .slug
        .clone()
        .filter(|slug| !slug.trim().is_empty())
        .unwrap_or_else(|| slugify_title(&document.title))
}

fn effective_publication_visibility(
    document: &Document,
    policy: &PublicationPolicy,
) -> PublicationVisibility {
    let mut visibility = policy.default_visibility;

    for rule in &policy.rules {
        let tag_match = rule
            .match_tag
            .as_ref()
            .map(|tag| document.frontmatter.tags.iter().any(|doc_tag| doc_tag == tag))
            .unwrap_or(false);
        let path_match = rule
            .match_path_prefix
            .as_ref()
            .map(|prefix| document.path.starts_with(prefix))
            .unwrap_or(false);

        if tag_match || path_match {
            visibility = rule.visibility;
        }
    }

    if document.frontmatter.publication.visibility != PublicationVisibility::Private {
        document.frontmatter.publication.visibility
    } else {
        visibility
    }
}

fn render_published_markdown(vault: &Vault, document: &Document) -> Result<String> {
    let body = rewrite_wikilinks_for_publication(vault, &document.content, PublicationRender::Markdown)?;
    let mut frontmatter = document.frontmatter.clone();
    frontmatter.imported_reference = false;
    frontmatter.source_path = None;
    frontmatter.source_format = None;
    frontmatter.imported_at = None;
    let frontmatter = toml::to_string(&frontmatter).unwrap_or_default();
    Ok(format!("+++\n{frontmatter}\n+++\n\n{body}"))
}

fn render_published_html(vault: &Vault, document: &Document) -> Result<String> {
    let body = rewrite_wikilinks_for_publication(vault, &document.content, PublicationRender::Html)?;
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    let html = markdown_to_html(&body, &options);
    Ok(format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><style>body{{max-width:860px;margin:0 auto;padding:40px 24px;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;line-height:1.6;background:#0b0f16;color:#d7e0ea}}a{{color:#4cc9f0}}pre,code{{background:#111826;border-radius:6px}}pre{{padding:12px;overflow:auto}}blockquote{{border-left:3px solid #29465b;padding-left:12px;color:#9fb1c1}}</style></head><body><main>{}</main></body></html>",
        document.title, html
    ))
}

fn render_published_micron(vault: &Vault, document: &Document) -> Result<String> {
    let body = rewrite_wikilinks_for_publication(vault, &document.content, PublicationRender::Micron)?;
    let micron = markdown_to_micron(&body);
    Ok(format!("# title: {}\n\n{micron}", document.title))
}

/// Convert markdown text to Micron markup for NomadNet pages.
fn markdown_to_micron(markdown: &str) -> String {
    let mut out = String::new();
    let mut in_code_block = false;

    for line in markdown.lines() {
        if in_code_block {
            if line.starts_with("```") {
                in_code_block = false;
                out.push_str("``\n");
            } else {
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }

        if line.starts_with("```") {
            in_code_block = true;
            out.push_str("`=\n");
            continue;
        }

        // Headings: # → >, ## → >>, etc.
        if let Some(rest) = line.strip_prefix("### ") {
            out.push_str(&format!(">>>`!{rest}`\n"));
        } else if let Some(rest) = line.strip_prefix("## ") {
            out.push_str(&format!(">>`!{rest}`\n"));
        } else if let Some(rest) = line.strip_prefix("# ") {
            out.push_str(&format!(">`!{rest}`\n"));
        } else if line == "---" || line == "***" || line == "___" {
            out.push_str("-\n");
        } else if let Some(rest) = line.strip_prefix("> ") {
            // Blockquote — indent
            out.push_str(&format!("  {}\n", convert_inline_micron(rest)));
        } else if let Some(rest) = line.strip_prefix("- [x] ") {
            out.push_str(&format!("[x] {}\n", convert_inline_micron(rest)));
        } else if let Some(rest) = line.strip_prefix("- [ ] ") {
            out.push_str(&format!("[ ] {}\n", convert_inline_micron(rest)));
        } else {
            out.push_str(&convert_inline_micron(line));
            out.push('\n');
        }
    }

    if in_code_block {
        out.push_str("``\n");
    }

    out
}

/// Convert inline markdown formatting to Micron within a single line.
fn convert_inline_micron(line: &str) -> String {
    let mut result = String::new();
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            // Bold: **text**
            '*' if chars.peek() == Some(&'*') => {
                chars.next(); // consume second *
                let inner = take_until_marker(&mut chars, "**");
                result.push_str(&format!("`!{inner}`"));
            }
            // Italic: *text*
            '*' => {
                let inner = take_until_marker(&mut chars, "*");
                result.push_str(&format!("`*{inner}`"));
            }
            // Strikethrough: ~~text~~ — no micron equivalent, render plain
            '~' if chars.peek() == Some(&'~') => {
                chars.next();
                let inner = take_until_marker(&mut chars, "~~");
                result.push_str(&inner);
            }
            // Backtick: either a Micron link `[...`...] or inline code `text`
            '`' => {
                if chars.peek() == Some(&'[') {
                    // Already a Micron link — pass through verbatim until closing ]
                    result.push('`');
                    let rest = take_until_marker(&mut chars, "]");
                    result.push_str(&rest);
                    result.push(']');
                } else {
                    let inner = take_until_marker(&mut chars, "`");
                    result.push_str(&format!("`={inner}`"));
                }
            }
            // Markdown links: [text](url) — but NOT micron links `[text`url]
            '[' => {
                let text = take_until_marker(&mut chars, "]");
                if chars.peek() == Some(&'(') {
                    chars.next(); // consume (
                    let url = take_until_marker(&mut chars, ")");
                    result.push_str(&format!("`[{text}`{url}]"));
                } else {
                    // Not a link, just brackets
                    result.push('[');
                    result.push_str(&text);
                    result.push(']');
                }
            }
            _ => result.push(c),
        }
    }

    result
}

/// Consume chars until the marker string is found, returning the content before it.
fn take_until_marker(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, marker: &str) -> String {
    let mut buf = String::new();
    let marker_chars: Vec<char> = marker.chars().collect();

    loop {
        match chars.peek() {
            None => break,
            Some(&c) if c == marker_chars[0] => {
                if marker_chars.len() == 1 {
                    chars.next();
                    break;
                }
                // Check for multi-char marker
                let mut matched = vec![c];
                chars.next();
                let mut all_match = true;
                for &mc in &marker_chars[1..] {
                    match chars.peek() {
                        Some(&nc) if nc == mc => {
                            matched.push(nc);
                            chars.next();
                        }
                        _ => {
                            all_match = false;
                            break;
                        }
                    }
                }
                if all_match {
                    break;
                } else {
                    buf.extend(matched);
                }
            }
            _ => {
                buf.push(chars.next().unwrap());
            }
        }
    }

    buf
}

#[derive(Clone, Copy)]
enum PublicationRender {
    Markdown,
    Html,
    Micron,
}

fn rewrite_wikilinks_for_publication(vault: &Vault, body: &str, mode: PublicationRender) -> Result<String> {
    let mut rendered = String::new();
    let mut remaining = body;

    while let Some(start) = remaining.find("[[") {
        rendered.push_str(&remaining[..start]);
        let after = &remaining[start + 2..];
        let Some(end) = after.find("]]" ) else {
            rendered.push_str(&remaining[start..]);
            return Ok(rendered);
        };
        let inner = &after[..end];
        remaining = &after[end + 2..];

        let (target_part, display) = if let Some(pipe) = inner.find('|') {
            (&inner[..pipe], Some(&inner[pipe + 1..]))
        } else {
            (inner, None)
        };
        let (target, anchor) = if let Some(hash) = target_part.find('#') {
            (&target_part[..hash], Some(&target_part[hash + 1..]))
        } else {
            (target_part, None)
        };

        if let Some(linked) = vault.store.find_document_by_slug(target)? {
            let Some(linked_doc) = vault.store.get_document(&linked.id)? else {
                rendered.push_str(display.unwrap_or(target));
                continue;
            };
            if !linked_doc.frontmatter.publication.enabled
                || linked_doc.frontmatter.publication.visibility == PublicationVisibility::Private
            {
                rendered.push_str(display.unwrap_or(target));
                continue;
            }
            let slug = publication_slug(&linked_doc);
            let label = display.unwrap_or(&linked_doc.title);
            match mode {
                PublicationRender::Micron => {
                    let href = format!("/page/{slug}.mu");
                    rendered.push_str(&format!("`[{label}`{href}]"));
                }
                _ => {
                    let href = match mode {
                        PublicationRender::Markdown => {
                            if let Some(anchor) = anchor {
                                format!("/{slug}#{}", slugify_title(anchor))
                            } else {
                                format!("/{slug}")
                            }
                        }
                        PublicationRender::Html => {
                            if let Some(anchor) = anchor {
                                format!("/{slug}.html#{}", slugify_title(anchor))
                            } else {
                                format!("/{slug}.html")
                            }
                        }
                        PublicationRender::Micron => unreachable!(),
                    };
                    rendered.push_str(&format!("[{label}]({href})"));
                }
            }
        } else {
            rendered.push_str(display.unwrap_or(target));
        }
    }

    rendered.push_str(remaining);
    Ok(rendered)
}

fn resolve_index_db_path(root: &Path, runtime: &LocalRuntimeConfig) -> PathBuf {
    if let Some(path) = runtime.flynt_index_db_path.as_ref().filter(|path| path.is_absolute()) {
        return path.clone();
    }

    if let Some(local_state_root) = runtime
        .local_state_root
        .as_ref()
        .filter(|path| path.is_absolute())
    {
        return local_state_root.join("flynt").join("flynt-index.db");
    }

    root.join(".flynt-local").join("flynt").join("flynt-index.db")
}

fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry.file_name().to_str().map(|s| s.starts_with('.')).unwrap_or(false)
}

/// Heuristic: directory has `.git` plus at least one build manifest → code repo.
fn looks_like_code_repo(root: &Path) -> bool {
    if !root.join(".git").exists() {
        return false;
    }
    const BUILD_MANIFESTS: &[&str] = &[
        "Cargo.toml",
        "package.json",
        "go.mod",
        "pyproject.toml",
        "Makefile",
        "CMakeLists.txt",
        "pom.xml",
        "build.gradle",
        "Gemfile",
        "mix.exs",
        "flake.nix",
        "deno.json",
    ];
    BUILD_MANIFESTS.iter().any(|m| root.join(m).exists())
}

/// Replace the tags array in a TOML frontmatter document.
/// Returns None if no frontmatter found.
fn replace_frontmatter_tags(content: &str, new_tags: &[String]) -> Option<String> {
    // Find +++...+++ frontmatter block — must be at line boundaries
    // First +++ must be at the very start of the content
    if !content.starts_with("+++") { return None; }
    let first = 0;
    // Second +++ is the next line that is exactly "+++"
    let second = content[3..]
        .lines()
        .scan(3usize, |pos, line| {
            let start = *pos;
            *pos += line.len() + 1; // +1 for newline
            Some((start, line))
        })
        .find(|(_, line)| line.trim() == "+++")
        .map(|(pos, _)| pos)?;
    let fm_text = &content[first + 3..second];

    // Find the tags line
    let tags_serialized: Vec<String> = new_tags.iter().map(|t| format!("\"{}\"", t.replace('"', ""))).collect();
    let new_tags_line = format!("tags = [{}]", tags_serialized.join(", "));

    // Replace the tags = [...] line in the frontmatter
    let mut new_fm = String::new();
    let mut found_tags = false;
    for line in fm_text.lines() {
        if line.trim_start().starts_with("tags") && line.contains('=') {
            new_fm.push_str(&new_tags_line);
            found_tags = true;
        } else {
            new_fm.push_str(line);
        }
        new_fm.push('\n');
    }
    if !found_tags {
        new_fm.push_str(&new_tags_line);
        new_fm.push('\n');
    }

    // Skip past the closing "+++" and its newline
    let after_fm = second + 3;
    let body_start = if content[after_fm..].starts_with('\n') { after_fm + 1 } else { after_fm };
    Some(format!("+++\n{}+++\n{}", new_fm, &content[body_start..]))
}

#[cfg(test)]
mod tests {
    use super::{canonical_document_source, import_destination_path, markdown_to_micron, resolve_index_db_path, Vault};
    use chrono::Utc;
    use flynt_core::{
        models::{Document, DocumentId, Frontmatter, LocalRuntimeConfig, MetadataValue, PublicationRule, PublicationVisibility},
        store::VaultStore,
    };
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn uses_explicit_absolute_index_db_path_when_configured() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("vault");
        let explicit = tmp.path().join("state/custom-index.db");

        let resolved = resolve_index_db_path(
            &root,
            &LocalRuntimeConfig {
                flynt_index_db_path: Some(explicit.clone()),
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

        assert_eq!(resolved, local_state_root.join("flynt/flynt-index.db"));
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

    #[test]
    fn stores_agent_communication_under_references_comms_with_metadata_and_links() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        let vault = Vault::open(&vault_root).unwrap();

        let relative_path = vault
            .store_agent_communication("vox", "Standup Recall", "See [[design]].")
            .unwrap();

        assert!(relative_path.starts_with("references/comms/vox"));
        let doc = vault.store.get_document_by_path(&relative_path).unwrap().unwrap();
        assert_eq!(doc.title, "Standup Recall");
        assert_eq!(doc.frontmatter.source_format.as_deref(), Some("omegon_comm"));
        assert_eq!(doc.frontmatter.source_path.as_deref(), Some("omegon://vox"));
        assert!(doc.frontmatter.imported_reference);
        assert_eq!(
            doc.frontmatter.metadata.get("channel"),
            Some(&MetadataValue::String("vox".into()))
        );
        assert_eq!(doc.outgoing_links.len(), 1);
        assert_eq!(doc.outgoing_links[0].target, "design");
    }

    #[test]
    #[ignore = "metadata flatten roundtrip through TOML→SQLite drops extra keys — needs schema fix"]
    fn stores_memory_fact_under_ai_memory_with_metadata_and_links() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        let vault = Vault::open(&vault_root).unwrap();

        let relative_path = vault
            .store_memory_fact("storage", "Canonical vs Local", "Supports [[design]].")
            .unwrap();

        assert!(relative_path.starts_with("ai/memory/storage"));
        let doc = vault.store.get_document_by_path(&relative_path).unwrap().unwrap();
        assert_eq!(doc.title, "Canonical vs Local");
        assert_eq!(doc.frontmatter.source_format.as_deref(), Some("omegon_memory"));
        assert_eq!(doc.frontmatter.source_path.as_deref(), Some("omegon://memory/storage"));
        assert_eq!(
            doc.frontmatter.metadata.get("topic"),
            Some(&MetadataValue::String("storage".into()))
        );
        assert_eq!(
            doc.frontmatter.metadata.get("kind"),
            Some(&MetadataValue::String("memory_fact".into()))
        );
        assert_eq!(doc.outgoing_links.len(), 1);
        assert_eq!(doc.outgoing_links[0].target, "design");
    }

    #[test]
    fn canonical_document_source_terminates_with_newline() {
        let now = Utc::now();
        let doc = Document {
            id: DocumentId::new(),
            path: PathBuf::from("notes/example.md"),
            title: "Example".into(),
            content: "Body".into(),
            frontmatter: Frontmatter::default(),
            outgoing_links: vec![],
            created_at: now,
            updated_at: now,
            entity: None,
        };

        let rendered = canonical_document_source(&doc);
        assert!(rendered.ends_with('\n'));
        assert!(rendered.contains("\n\nBody\n"));
    }

    #[test]
    fn publication_export_reports_duplicate_slugs() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_root).unwrap();
        let output_root = tmp.path().join("published");
        let vault = Vault::open(&vault_root).unwrap();
        assert!(vault.store.list_documents().unwrap().is_empty());

        for (name, title) in [("alpha.md", "Same"), ("beta.md", "Same")] {
            let path = vault_root.join(name);
            std::fs::write(
                &path,
                format!(
                    "+++\ntitle = \"{title}\"\n[publication]\nenabled = true\nvisibility = \"public\"\n+++\n\n# {title}\n"
                ),
            )
            .unwrap();
            vault.index_file(&path).unwrap();
        }

        let report = vault.export_publication_tree(&output_root).unwrap();
        assert_eq!(report.exported, 1);
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].contains("duplicate publication slug"));
    }

    #[test]
    fn publication_policy_rules_can_publish_selected_subset() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();
        assert!(vault.store.list_documents().unwrap().is_empty());

        let public_path = vault_root.join("docs/public.md");
        std::fs::create_dir_all(public_path.parent().unwrap()).unwrap();
        std::fs::write(
            &public_path,
            "+++\ntitle = \"Public\"\ntags = [\"public\"]\n[publication]\nenabled = true\n+++\n\n# Public\n",
        )
        .unwrap();
        vault.index_file(&public_path).unwrap();

        let private_path = vault_root.join("private.md");
        std::fs::write(
            &private_path,
            "+++\ntitle = \"Private\"\n[publication]\nenabled = true\n+++\n\n# Private\n",
        )
        .unwrap();
        vault.index_file(&private_path).unwrap();

        let mut config = vault.config.clone();
        config.publication.default_visibility = PublicationVisibility::Private;
        config.publication.rules = vec![PublicationRule {
            match_tag: Some("public".into()),
            match_path_prefix: None,
            visibility: PublicationVisibility::Public,
        }];
        vault.save_config(&config).unwrap();
        let filtered_vault = Vault::open(&vault_root).unwrap();

        let report = filtered_vault
            .export_publication_tree(&tmp.path().join("published"))
            .unwrap();
        assert_eq!(report.exported, 1);
        assert_eq!(report.skipped_private, 1);
    }

    #[test]
    fn exports_public_documents_with_resolved_wikilinks() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        let output_root = tmp.path().join("published");
        let vault = Vault::open(&vault_root).unwrap();

        let roadmap_path = vault_root.join("roadmap.md");
        std::fs::write(
            &roadmap_path,
            "+++
title = \"Roadmap\"
[publication]
enabled = true
visibility = \"public\"
+++

# Roadmap\n",
        )
        .unwrap();
        vault.index_file(&roadmap_path).unwrap();

        let design_path = vault_root.join("design.md");
        std::fs::write(
            &design_path,
            "+++
title = \"Design\"
[publication]
enabled = true
visibility = \"public\"
+++

# Design

See [[roadmap|the roadmap]].\n",
        )
        .unwrap();
        vault.index_file(&design_path).unwrap();

        let report = vault.export_publication_tree(&output_root).unwrap();
        assert_eq!(report.exported, 2);
        assert!(report.errors.is_empty());

        let published = std::fs::read_to_string(output_root.join("design.md")).unwrap();
        assert!(published.contains("[the roadmap](/roadmap)"));
        assert!(!published.contains("source_path"));

        let html = std::fs::read_to_string(output_root.join("design.html")).unwrap();
        assert!(html.contains("href=\"/roadmap.html\""));

        // Micron export
        let micron = std::fs::read_to_string(output_root.join("design.mu")).unwrap();
        assert!(micron.contains("`[the roadmap`/page/roadmap.mu]"), "micron wikilink: {micron}");
        assert!(micron.contains(">`!Design`"), "micron heading: {micron}");

        // NomadNet index page
        let index = std::fs::read_to_string(output_root.join("index.mu")).unwrap();
        assert!(index.contains("`[Design`/page/design.mu]"), "index entry: {index}");
        assert!(index.contains("`[Roadmap`/page/roadmap.mu]"), "index entry: {index}");

        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(output_root.join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["documents"].as_array().unwrap().len(), 2);
        assert_eq!(manifest["documents"][0]["slug"], "design");
    }

    #[test]
    fn markdown_to_micron_converts_formatting() {
        let md = "# Top Heading\n\n## Sub Heading\n\nSome **bold** and *italic* text.\n\n- list item\n- [x] done task\n- [ ] todo task\n\n> a quote\n\n---\n\n```rust\nfn main() {}\n```\n\nA [link](https://example.com) here.\n";
        let mu = markdown_to_micron(md);

        assert!(mu.contains(">`!Top Heading`"), "h1: {mu}");
        assert!(mu.contains(">>`!Sub Heading`"), "h2: {mu}");
        assert!(mu.contains("`!bold`"), "bold: {mu}");
        assert!(mu.contains("`*italic`"), "italic: {mu}");
        assert!(mu.contains("[x] done task"), "done task: {mu}");
        assert!(mu.contains("[ ] todo task"), "todo task: {mu}");
        assert!(mu.contains("  a quote"), "quote: {mu}");
        assert!(mu.contains("-\n"), "hr: {mu}");
        assert!(mu.contains("`=\nfn main() {}\n``"), "code block: {mu}");
        assert!(mu.contains("`[link`https://example.com]"), "link: {mu}");
    }

    // ── Project git-backing tests ────────────────────────────────────────

    fn create_git_backed_project(vault: &Vault) -> uuid::Uuid {
        let project_id = uuid::Uuid::new_v4();
        let sub_path = format!("projects/{}", &project_id.to_string()[..8]);

        let frontmatter = format!(
            r#"+++
id = "{project_id}"
kind = "project"

[data]
title = "Test Project"
status = "active"
columns = ["Backlog", "In Progress", "Done"]

[data.git_backing]
type = "vault_repo"
sub_path = "{sub_path}"

[data.commit_config]
auto_commit_seconds = 0
+++

# Test Project
"#
        );

        // Use a unique path per project to avoid collision
        let rel_path = PathBuf::from(format!("project-{}.md", &project_id.to_string()[..8]));
        let abs_path = vault.root.join(&rel_path);
        std::fs::write(&abs_path, &frontmatter).unwrap();
        vault.index_file(&abs_path).unwrap();

        project_id
    }

    #[test]
    fn flush_project_writes_dirty_tasks_to_disk() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        // Init a git repo so ProjectGit can open it
        git2::Repository::init(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();

        let project_id = create_git_backed_project(&vault);

        // Create a task associated with this project
        let board = flynt_core::models::Board::default_sprint("Sprint");
        vault.store.save_board(&board).unwrap();

        let mut task = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Fix the parser");
        task.description = "Need to fix the TOML parser.".into();
        task.priority = flynt_core::models::Priority::High;

        // Save task with project_id set
        vault.store.save_task(&task).unwrap();

        // Associate task with this project
        vault.store.set_task_project(&task.id, &project_id).unwrap();

        // Flush should write the task file
        let report = vault.flush_project(project_id).unwrap();
        assert_eq!(report.tasks_flushed, 1);

        // Verify the file exists on disk
        let project_entity = vault.store.get_document(&DocumentId(project_id)).unwrap().unwrap();
        let entity = project_entity.entity.unwrap();
        let view = flynt_core::datum::ProjectView::from_entity(&entity).unwrap();
        let backing = view.git_backing().unwrap();
        let task_path = vault_root.join(backing.sub_path()).join("tasks").join(format!("{}.md", task.id.0));
        assert!(task_path.exists(), "task file should exist at {}", task_path.display());

        // Verify content
        let content = std::fs::read_to_string(&task_path).unwrap();
        assert!(content.contains("Fix the parser"));
        assert!(content.contains("kind = \"task\""));
    }

    #[test]
    fn reindex_project_reads_task_files_into_db() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        git2::Repository::init(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();

        let project_id = create_git_backed_project(&vault);

        // Resolve the sub_path from the project entity
        let project_doc = vault.store.get_document(&DocumentId(project_id)).unwrap().unwrap();
        let entity = project_doc.entity.unwrap();
        let view = flynt_core::datum::ProjectView::from_entity(&entity).unwrap();
        let backing = view.git_backing().unwrap();

        // Write a task file manually (simulating a git pull)
        let task_id = uuid::Uuid::new_v4();
        // Create the board first so the FK constraint is satisfied
        let board = flynt_core::models::Board::default_sprint("Sprint");
        vault.store.save_board(&board).unwrap();
        let board_id = board.id.0;
        let task_md = format!(
            r#"+++
id = "{task_id}"
kind = "task"

[data]
title = "Imported task"
project = "{project_id}"
board = "{board_id}"
column = "In Progress"
priority = 3
status = "in_progress"
position = 1
+++

Description from git.
"#
        );
        let tasks_dir = vault_root.join(backing.sub_path()).join("tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(tasks_dir.join(format!("{task_id}.md")), &task_md).unwrap();

        // Reindex should parse the file and insert into SQLite
        let count = vault.reindex_project(project_id).unwrap();
        assert_eq!(count, 1);

        // Verify the task is in the DB
        let stored_task = vault.store.get_task(&flynt_core::models::TaskId(task_id)).unwrap();
        assert!(stored_task.is_some(), "task should be in SQLite after reindex");
        let stored = stored_task.unwrap();
        assert_eq!(stored.title, "Imported task");
        assert_eq!(stored.column, "In Progress");
        assert_eq!(stored.priority, flynt_core::models::Priority::High);
    }

    #[test]
    fn flush_then_reindex_roundtrips() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        git2::Repository::init(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();

        let project_id = create_git_backed_project(&vault);

        let board = flynt_core::models::Board::default_sprint("Sprint");
        vault.store.save_board(&board).unwrap();

        let mut task = flynt_core::models::Task::new(board.id.clone(), "Review", "Write tests");
        task.tags = vec!["test".into()];
        vault.store.save_task(&task).unwrap();
        let original_id = task.id.clone();

        // Associate task with this project
        vault.store.set_task_project(&task.id, &project_id).unwrap();

        // Flush
        let report = vault.flush_project(project_id).unwrap();
        assert_eq!(report.tasks_flushed, 1);

        // Delete from DB to simulate a fresh reindex
        vault.store.delete_task(&original_id).unwrap();
        assert!(vault.store.get_task(&original_id).unwrap().is_none());

        // Reindex from disk
        let count = vault.reindex_project(project_id).unwrap();
        assert_eq!(count, 1);

        // Verify roundtrip
        let restored = vault.store.get_task(&original_id).unwrap().unwrap();
        assert_eq!(restored.title, "Write tests");
        assert_eq!(restored.column, "Review");
        assert_eq!(restored.tags, vec!["test"]);
    }

    #[test]
    fn publication_exports_project_board_state() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        git2::Repository::init(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();
        let output_root = tmp.path().join("published");

        let project_id = create_git_backed_project(&vault);

        // Enable publication on the project document
        let project_doc_path = vault_root.join(
            format!("project-{}.md", &project_id.to_string()[..8])
        );
        let raw = std::fs::read_to_string(&project_doc_path).unwrap();
        let updated = raw.replace(
            "+++\n\n# Test Project",
            "[publication]\nenabled = true\nvisibility = \"public\"\n+++\n\n# Test Project"
        );
        std::fs::write(&project_doc_path, &updated).unwrap();
        vault.index_file(&project_doc_path).unwrap();

        // Create a board for this project
        let board = flynt_core::models::Board::for_project("Sprint 1", project_id);
        vault.store.save_board(&board).unwrap();

        // Create tasks
        let mut task1 = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Design API");
        task1.priority = flynt_core::models::Priority::High;
        vault.save_project_task(&task1, &project_id).unwrap();

        let task2 = flynt_core::models::Task::new(board.id.clone(), "Running", "Build parser");
        vault.save_project_task(&task2, &project_id).unwrap();

        // Export
        let report = vault.export_publication_tree(&output_root).unwrap();
        assert!(report.exported >= 1, "should export at least the project board");
        assert!(report.errors.is_empty(), "errors: {:?}", report.errors);

        // Check board markdown exists and contains task info
        // Collect all exported files and search for board content across all of them
        let all_files: Vec<_> = walkdir::WalkDir::new(&output_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .collect();
        let all_content: String = all_files.iter()
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all_content.contains("Design API"), "should contain task title, got: {}", all_content);
        assert!(all_content.contains("Build parser"), "should contain second task");
        assert!(all_content.contains("**HIGH**"), "should show priority");
    }

    #[test]
    fn design_node_indexing_roundtrip() {
        use flynt_core::datum::EntityKind;

        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();

        let node_id = uuid::Uuid::new_v4();
        let parent_id = uuid::Uuid::new_v4();
        let design_md = format!(
            r#"+++
id = "{node_id}"
kind = "design_node"

[data]
title = "Auth Subsystem"
status = "exploring"
parent = "{parent_id}"
dependencies = ["dep-a", "dep-b"]
open_questions = ["Which OAuth flow?", "Token rotation?"]
priority = 3
+++

## Overview

Design for the authentication subsystem.

## Open Questions
"#
        );

        let design_dir = vault_root.join("design");
        std::fs::create_dir_all(&design_dir).unwrap();
        let file_path = design_dir.join("auth-subsystem.md");
        std::fs::write(&file_path, &design_md).unwrap();

        // Reindex the vault
        let (indexed, errors) = vault.reindex().unwrap();
        assert!(indexed >= 1, "should index at least the design node file");
        assert!(errors.is_empty(), "reindex errors: {errors:?}");

        // Retrieve the document
        let doc_id = flynt_core::models::DocumentId(node_id);
        let doc = vault.store.get_document(&doc_id).unwrap();
        assert!(doc.is_some(), "design node document should exist in store");
        let doc = doc.unwrap();

        // Verify entity kind
        assert!(doc.entity.is_some(), "document should have an entity");
        let entity = doc.entity.as_ref().unwrap();
        assert_eq!(entity.kind, EntityKind::DesignNode, "entity kind should be DesignNode");

        // Verify entity fields
        assert_eq!(entity.get_text("status"), Some("exploring"));
        assert_eq!(entity.get_text("parent").unwrap(), parent_id.to_string());
        assert_eq!(entity.get_int("priority"), Some(3));
        assert_eq!(entity.get_text_list("dependencies"), vec!["dep-a", "dep-b"]);
        assert_eq!(entity.get_text_list("open_questions"), vec!["Which OAuth flow?", "Token rotation?"]);

        // Verify it shows up in list_entities_by_kind
        let design_nodes = vault.store.list_entities_by_kind(&EntityKind::DesignNode).unwrap();
        assert!(design_nodes.iter().any(|m| m.id == doc_id), "design node should appear in kind listing");
    }

    #[test]
    fn set_document_kind_on_existing_frontmatter() {
        use flynt_core::datum::EntityKind;

        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();

        // Create a plain document with frontmatter
        let content = "+++\ntitle = \"My Note\"\ntags = [\"test\"]\n+++\n\n# My Note\n\nSome content.\n";
        let path = std::path::PathBuf::from("my-note.md");
        std::fs::write(vault_root.join(&path), content).unwrap();
        vault.reindex().unwrap();

        // Set kind to design_node
        vault.set_document_kind(&path, Some("design_node")).unwrap();

        // Re-read the file and verify
        let updated = std::fs::read_to_string(vault_root.join(&path)).unwrap();
        assert!(updated.contains("kind = \"design_node\""), "should contain kind field: {updated}");
        assert!(updated.contains("title = \"My Note\""), "should preserve title");
        assert!(updated.contains("tags = [\"test\"]"), "should preserve tags");
        assert!(updated.contains("# My Note"), "should preserve body");

        // Reindex and verify entity kind
        vault.reindex().unwrap();
        let docs = vault.store.list_documents().unwrap();
        let doc = docs.iter().find(|d| d.title == "My Note").unwrap();
        assert_eq!(doc.entity_kind, Some(EntityKind::DesignNode));
    }

    #[test]
    fn set_document_kind_replaces_existing_kind() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();

        let content = "+++\ntitle = \"Task Doc\"\nkind = \"task\"\n+++\n\nBody\n";
        let path = std::path::PathBuf::from("task-doc.md");
        std::fs::write(vault_root.join(&path), content).unwrap();

        // Change kind from task to project
        vault.set_document_kind(&path, Some("project")).unwrap();
        let updated = std::fs::read_to_string(vault_root.join(&path)).unwrap();
        assert!(updated.contains("kind = \"project\""), "should have new kind: {updated}");
        assert!(!updated.contains("kind = \"task\""), "should not have old kind: {updated}");
    }

    #[test]
    fn set_document_kind_clear_removes_kind() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();

        let content = "+++\ntitle = \"Typed\"\nkind = \"design_node\"\n+++\n\nBody\n";
        let path = std::path::PathBuf::from("typed.md");
        std::fs::write(vault_root.join(&path), content).unwrap();

        // Clear the kind
        vault.set_document_kind(&path, None).unwrap();
        let updated = std::fs::read_to_string(vault_root.join(&path)).unwrap();
        assert!(!updated.contains("kind ="), "should not contain kind: {updated}");
        assert!(updated.contains("title = \"Typed\""), "should preserve title");
    }

    #[test]
    fn set_document_kind_no_frontmatter_creates_one() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();

        let content = "# Just a note\n\nNo frontmatter here.\n";
        let path = std::path::PathBuf::from("plain.md");
        std::fs::write(vault_root.join(&path), content).unwrap();

        vault.set_document_kind(&path, Some("design_node")).unwrap();
        let updated = std::fs::read_to_string(vault_root.join(&path)).unwrap();
        assert!(updated.starts_with("+++\n"), "should start with frontmatter");
        assert!(updated.contains("kind = \"design_node\""), "should contain kind");
        assert!(updated.contains("# Just a note"), "should preserve body");
    }

    #[test]
    fn set_document_kind_clear_on_plain_doc_is_noop() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();

        let content = "# Plain\n\nNo kind to clear.\n";
        let path = std::path::PathBuf::from("noop.md");
        std::fs::write(vault_root.join(&path), content).unwrap();

        vault.set_document_kind(&path, None).unwrap();
        let updated = std::fs::read_to_string(vault_root.join(&path)).unwrap();
        // File gets reindexed (frontmatter added by indexer) but no kind field
        assert!(!updated.contains("kind ="), "should not contain kind: {updated}");
        assert!(updated.contains("# Plain"), "should preserve body");
    }

    #[test]
    fn set_document_kind_preserves_data_table() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();

        let content = "+++\ntitle = \"Design\"\nkind = \"design_node\"\n\n[data]\nstatus = \"exploring\"\npriority = 5\n+++\n\n# Design\n";
        let path = std::path::PathBuf::from("with-data.md");
        std::fs::write(vault_root.join(&path), content).unwrap();

        // Change kind — [data] table should be preserved
        vault.set_document_kind(&path, Some("project")).unwrap();
        let updated = std::fs::read_to_string(vault_root.join(&path)).unwrap();
        assert!(updated.contains("kind = \"project\""), "new kind: {updated}");
        assert!(updated.contains("[data]"), "should preserve data table: {updated}");
        assert!(updated.contains("status = \"exploring\""), "should preserve data fields: {updated}");
        assert!(updated.contains("priority = 5"), "should preserve data fields: {updated}");
    }

    #[test]
    fn set_document_kind_does_not_match_similar_field_names() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();

        // "kind_of_thing" should NOT be matched as the "kind" field
        let content = "+++\ntitle = \"Test\"\nkind_of_thing = \"misc\"\n+++\n\nBody\n";
        let path = std::path::PathBuf::from("similar.md");
        std::fs::write(vault_root.join(&path), content).unwrap();

        vault.set_document_kind(&path, Some("project")).unwrap();
        let updated = std::fs::read_to_string(vault_root.join(&path)).unwrap();
        assert!(updated.contains("kind = \"project\""), "should have kind: {updated}");
        assert!(updated.contains("kind_of_thing = \"misc\""), "should preserve similar field: {updated}");
    }

    #[test]
    fn set_document_kind_does_not_remove_kind_inside_data_table() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();

        // kind inside [data] should be preserved
        let content = "+++\ntitle = \"Node\"\nkind = \"design_node\"\n\n[data]\nkind = \"subtype-a\"\nstatus = \"active\"\n+++\n\nBody\n";
        let path = std::path::PathBuf::from("nested-kind.md");
        std::fs::write(vault_root.join(&path), content).unwrap();

        vault.set_document_kind(&path, Some("project")).unwrap();
        let updated = std::fs::read_to_string(vault_root.join(&path)).unwrap();
        assert!(updated.contains("kind = \"project\""), "should have new top-level kind: {updated}");
        // The [data] section's kind field should be preserved
        assert!(updated.contains("kind = \"subtype-a\""), "should preserve kind inside [data]: {updated}");
    }

    #[test]
    fn set_document_kind_handles_crlf_line_endings() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();

        // CRLF line endings
        let content = "+++\r\ntitle = \"CRLF\"\r\nkind = \"task\"\r\n+++\r\n\r\nBody\r\n";
        let path = std::path::PathBuf::from("crlf.md");
        std::fs::write(vault_root.join(&path), content).unwrap();

        vault.set_document_kind(&path, Some("design_node")).unwrap();
        let updated = std::fs::read_to_string(vault_root.join(&path)).unwrap();
        assert!(updated.contains("kind = \"design_node\""), "should have new kind: {updated}");
        assert!(!updated.contains("kind = \"task\""), "should not have old kind: {updated}");
        assert!(updated.contains("Body"), "should preserve body: {updated}");
    }

    #[test]
    fn graph_edges_from_wikilinks() {
        let tmp = TempDir::new().unwrap();
        let vault_root = tmp.path().join("vault");
        std::fs::create_dir_all(&vault_root).unwrap();
        let vault = Vault::open(&vault_root).unwrap();

        // Create two documents with wikilinks between them
        let id_a = uuid::Uuid::new_v4();
        let id_b = uuid::Uuid::new_v4();
        std::fs::write(
            vault_root.join("alpha.md"),
            format!("+++\nid = \"{id_a}\"\ntitle = \"Alpha\"\n+++\n\n# Alpha\n\nSee [[Beta]] for details.\n"),
        ).unwrap();
        std::fs::write(
            vault_root.join("beta.md"),
            format!("+++\nid = \"{id_b}\"\ntitle = \"Beta\"\n+++\n\n# Beta\n\nRefer back to [[Alpha]].\n"),
        ).unwrap();

        let (indexed, errors) = vault.reindex().unwrap();
        assert_eq!(indexed, 2, "should index both files");
        assert!(errors.is_empty());

        // Verify outgoing links are stored
        let doc_a = vault.store.get_document(&flynt_core::models::DocumentId(id_a)).unwrap().unwrap();
        assert_eq!(doc_a.outgoing_links.len(), 1, "alpha should link to beta");
        assert_eq!(doc_a.outgoing_links[0].target.to_lowercase(), "beta");

        let doc_b = vault.store.get_document(&flynt_core::models::DocumentId(id_b)).unwrap().unwrap();
        assert_eq!(doc_b.outgoing_links.len(), 1, "beta should link to alpha");
        assert_eq!(doc_b.outgoing_links[0].target.to_lowercase(), "alpha");

        // Build graph and verify edges
        let graph = flynt_core::graph::build_graph_payload(&*vault.store).unwrap();
        assert!(graph.nodes.len() >= 2, "should have at least 2 nodes");
        assert!(!graph.edges.is_empty(), "should have wikilink edges, got: {:?}", graph.edges);

        let wikilink_edges: Vec<_> = graph.edges.iter()
            .filter(|e| matches!(e.kind, flynt_core::graph::GraphEdgeKind::Wikilink))
            .collect();
        assert_eq!(wikilink_edges.len(), 2, "should have 2 wikilink edges (A→B and B→A), got {}", wikilink_edges.len());
    }

    // ── save_any_task: every task becomes a file ────────────────────────────

    fn vault_with_board() -> (TempDir, Vault, flynt_core::models::Board) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let vault = Vault::open(&root).unwrap();
        let board = flynt_core::models::Board::default_sprint("Sprint 1");
        vault.store.save_board(&board).unwrap();
        (tmp, vault, board)
    }

    #[test]
    fn save_any_task_writes_file_and_records_path() {
        let (_tmp, vault, board) = vault_with_board();
        let task = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Auth Rewrite");
        let rel = vault.save_any_task(&task).unwrap();
        assert_eq!(rel, std::path::PathBuf::from("Tasks/sprint-1/auth-rewrite.md"));
        assert!(vault.root.join(&rel).exists(), "file must exist on disk");
        assert_eq!(
            vault.store.task_file_path(&task.id).unwrap().as_deref(),
            Some(rel.to_string_lossy().as_ref()),
        );
    }

    #[test]
    fn save_any_task_renames_file_when_title_changes() {
        let (_tmp, vault, board) = vault_with_board();
        let mut task = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Initial");
        let first = vault.save_any_task(&task).unwrap();
        assert!(vault.root.join(&first).exists());

        task.title = "Renamed".into();
        let second = vault.save_any_task(&task).unwrap();
        assert_ne!(first, second, "rename must produce a different path");
        assert!(vault.root.join(&second).exists(), "new file must exist");
        assert!(!vault.root.join(&first).exists(), "old file must be removed");
    }

    #[test]
    fn save_any_task_handles_collisions_with_numeric_suffix() {
        let (_tmp, vault, board) = vault_with_board();
        let t1 = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Same Title");
        let t2 = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Same Title");
        let p1 = vault.save_any_task(&t1).unwrap();
        let p2 = vault.save_any_task(&t2).unwrap();
        assert!(p1.to_string_lossy().ends_with("same-title.md"));
        assert!(p2.to_string_lossy().ends_with("same-title-2.md"));
        assert!(vault.root.join(&p1).exists());
        assert!(vault.root.join(&p2).exists());
    }

    #[test]
    fn migrate_tasks_to_files_writes_legacy_sqlite_only_tasks() {
        // Simulate a legacy vault: task in sqlite without task_file_path.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let vault = Vault::open(&root).unwrap();
        let board = flynt_core::models::Board::default_sprint("Backlog Board");
        vault.store.save_board(&board).unwrap();
        let task = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Legacy task");
        // Bypass save_any_task — write directly via store, mimicking pre-migration state.
        vault.store.save_task(&task).unwrap();
        assert!(vault.store.task_file_path(&task.id).unwrap().is_none());

        let n = vault.migrate_tasks_to_files().unwrap();
        assert_eq!(n, 1);
        let path = vault.store.task_file_path(&task.id).unwrap().unwrap();
        assert!(vault.root.join(&path).exists());
        assert!(path.starts_with("Tasks/backlog-board/"));
    }

    #[test]
    fn save_any_task_is_idempotent_when_called_twice_with_same_data() {
        let (_tmp, vault, board) = vault_with_board();
        let task = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Steady");
        let p1 = vault.save_any_task(&task).unwrap();
        let p2 = vault.save_any_task(&task).unwrap();
        assert_eq!(p1, p2);
        // Only one file under Tasks/sprint-1/
        let entries: Vec<_> = std::fs::read_dir(vault.root.join("Tasks/sprint-1")).unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1, "no duplicate files on resave");
    }
}
