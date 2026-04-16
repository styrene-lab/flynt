use anyhow::Result;
use codex_core::{
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

        for document in self.store.list_documents()? {
            match self.export_published_document(&document.path, output_root) {
                Ok(Some(published)) => {
                    exported += 1;
                    manifest_entries.push(PublicationManifestEntry {
                        title: published.title,
                        slug: published.slug,
                        source_path: published.source_path,
                        output_path: published.output_path,
                        tags: document.tags,
                        visibility: PublicationVisibility::Public,
                    });
                }
                Ok(None) => skipped_private += 1,
                Err(err) => errors.push(format!("{}: {err}", document.path.display())),
            }
        }

        let manifest = PublicationManifest {
            generated_at: Utc::now().to_rfc3339(),
            documents: manifest_entries,
        };
        fs::create_dir_all(output_root)?;
        fs::write(
            output_root.join("manifest.json"),
            serde_json::to_string_pretty(&manifest)?,
        )?;

        Ok(PublicationExportReport { exported, skipped_private, errors })
    }

    fn export_published_document(&self, relative_path: &Path, output_root: &Path) -> Result<Option<PublishedDocument>> {
        let Some(document) = self.store.get_document_by_path(relative_path)? else {
            return Ok(None);
        };
        if !document.frontmatter.publication.enabled
            || document.frontmatter.publication.visibility == PublicationVisibility::Private
        {
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
        fs::write(&output_path, published_markdown)?;
        fs::write(&html_path, published_html)?;
        Ok(Some(manifest))
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
    format!("+++\n{frontmatter}\n+++\n\n{}", document.content)
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
        .unwrap_or_else(|| slugify_title(&document.title))
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

#[derive(Clone, Copy)]
enum PublicationRender {
    Markdown,
    Html,
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
            };
            rendered.push_str(&format!("[{label}]({href})"));
        } else {
            rendered.push_str(display.unwrap_or(target));
        }
    }

    rendered.push_str(remaining);
    Ok(rendered)
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
    use codex_core::{models::{LocalRuntimeConfig, MetadataValue}, store::VaultStore};
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

        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(output_root.join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["documents"].as_array().unwrap().len(), 2);
        assert_eq!(manifest["documents"][0]["slug"], "design");
    }
}
