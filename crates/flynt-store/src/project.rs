use crate::sqlite::SqliteStore;
use crate::task_file;
use anyhow::{Context, Result};
use chrono::Utc;
use comrak::{Options, markdown_to_html};
use flynt_core::{models::*, parser::parse_document_source, store::ProjectStore};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{debug, info, warn};

/// Project manages the root directory layout:
///
///   <project_root>/
///     .flynt/
///       config.toml    ← sync + preferences
///     **/*.md          ← notes/documents
///
/// Local SQLite state is materialized outside the syncable project whenever
/// `local_runtime.flynt_index_db_path` (or its derived default) resolves to a
/// local app-state directory.
pub struct Project {
    pub root: PathBuf,
    pub store: Arc<SqliteStore>,
    pub config: ProjectConfig,
    /// Set by higher layers (flynt-app's bootstrap) to subscribe to
    /// save events. The save paths fan out to `SaveHook::on_task_saved`
    /// when a task file is written; the desktop's push pipeline uses
    /// this to feed its `PushDebouncer`. None until set; flynt-store
    /// itself stays dep-light.
    pub save_hook: std::sync::OnceLock<Arc<dyn crate::save_hook::SaveHook>>,
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

impl Project {
    fn bookmarks_path(&self) -> PathBuf {
        self.root.join(".flynt").join("bookmarks.toml")
    }

    fn lenses_dir(&self) -> PathBuf {
        self.root.join(".flynt").join("lenses")
    }

    /// Open (or create) a project rooted at `root`.
    pub fn open(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;

        // Auto-migrate from .codex/ → .flynt/ (pre-Flynt projects)
        let old_dir = root.join(".codex");
        let flynt_dir = root.join(".flynt");
        if old_dir.exists() && !flynt_dir.exists() {
            info!("Migrating project config: .codex/ → .flynt/");
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
                info!(
                    "Code repo detected at {:?} — defaulting write_frontmatter to false",
                    root
                );
                IndexingConfig {
                    write_frontmatter: false,
                    scopes: Vec::new(),
                }
            } else {
                IndexingConfig::default()
            };

            let cfg = ProjectConfig {
                project_name: default_name,
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
                tracing::warn!(
                    "Could not create .gitignore at {}: {e} — local state may be committed if git sync is enabled",
                    gitignore.display()
                );
            }
        }

        let db_path = resolve_index_db_path(root, &config.local_runtime);
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let store = Arc::new(SqliteStore::open(&db_path)?);

        info!("Project opened at {:?}, store ready at {:?}", root, db_path);
        let project = Self {
            root: root.to_owned(),
            store,
            config,
            save_hook: std::sync::OnceLock::new(),
        };

        // Migration: every task becomes a file. Legacy sqlite-only tasks
        // get a `.md` written under `Tasks/<board-slug>/`. Idempotent —
        // tasks with task_file_path already set are skipped.
        if let Err(e) = project.migrate_tasks_to_files() {
            warn!("task→file migration failed (continuing): {e}");
        }

        // Ensure a "Default" board exists so the kanban is never empty
        // and convert-to-task always has a target. Idempotent — only
        // creates if zero boards exist.
        if let Err(e) = project.ensure_default_board() {
            warn!("ensure_default_board failed (continuing): {e}");
        }

        Ok(project)
    }

    /// Register a save hook (typically the push pipeline's debouncer).
    /// Idempotent — first set wins. flynt-app calls this once during
    /// bootstrap; subsequent calls are no-ops.
    pub fn install_save_hook(&self, hook: Arc<dyn crate::save_hook::SaveHook>) {
        let _ = self.save_hook.set(hook);
    }

    /// Fire the save hook for a task file at `rel_path`. No-op when no
    /// hook is installed (tests, headless agents). Looks up the doc
    /// from sqlite to confirm it's a task before firing — we never
    /// notify on plain notes.
    fn notify_task_saved(&self, rel_path: &Path) {
        let Some(hook) = self.save_hook.get() else {
            return;
        };
        let Ok(Some(doc)) = self.store.get_document_by_path(rel_path) else {
            return;
        };
        if doc.frontmatter.kind.as_deref() != Some("task") {
            return;
        }
        hook.on_task_saved(doc.id.0);
    }

    /// Create a "Default" minimalist board if no boards exist yet.
    ///
    /// Idempotent. Existing projects with one or more boards are
    /// untouched. Fresh projects get a 2-column (Active, Archive)
    /// board so the kanban + convert-to-task always have a landing
    /// spot. Operator can rename "Default" later.
    pub fn ensure_default_board(&self) -> Result<()> {
        let existing = self.store.list_boards()?;
        if !existing.is_empty() {
            return Ok(());
        }
        let board = Board::minimalist("Default");
        self.store.save_board(&board)?;
        info!("Created Default board for fresh project");
        Ok(())
    }

    /// Index all markdown files under the project root into the SQLite store.
    /// Skips `.flynt/` directory. Idempotent — safe to call on every launch.
    pub fn reindex(&self) -> Result<(usize, Vec<String>)> {
        let mut indexed = 0;
        let mut errors = Vec::new();
        self.walk_markdown(&mut |path| match self.index_file(path) {
            Ok(_) => indexed += 1,
            Err(e) => {
                errors.push(format!("{}: {e}", path.display()));
                debug!("index error: {e}");
            }
        })?;

        // (Removed: reindex_all_projects iterated kind="project" entities
        // and recursively walked their git_backing sub-paths. With the
        // inner Project entity dissolved, the outer reindex above
        // covers everything.)

        info!("Reindex complete: {indexed} files, {} errors", errors.len());
        Ok((indexed, errors))
    }

    /// Parse and upsert a single markdown file into the store.
    pub fn index_file(&self, path: &Path) -> Result<()> {
        let raw = fs::read_to_string(path)?;
        let rel_path = path.strip_prefix(&self.root)?.to_owned();
        let (body, mut frontmatter, links) = parse_document_source(&raw);

        // Derive title: H1 > frontmatter.title > [data].title > filename
        // stem. The `[data].title` step matters for entity-typed docs
        // (tasks especially) — they store title in the entity payload,
        // not at the top level. Without this step, indexed tasks
        // surface the filename slug everywhere.
        let title = extract_h1(&body)
            .or_else(|| frontmatter.title.clone())
            .or_else(|| extract_data_title(&frontmatter.data))
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
        // (or the project-wide default allows it).
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
    /// Save the body of a document while preserving its existing
    /// frontmatter verbatim. The notes view's CodeMirror editor only
    /// shows the body; without this preservation step, every save
    /// would rewrite the file with body-only content, the next
    /// `index_file` would inject default document-shape frontmatter,
    /// and any unknown fields (task `kind` / `[data]` block, custom
    /// metadata, etc.) would silently die on first save.
    ///
    /// `content` is treated as the BODY. Frontmatter from disk is
    /// kept as-is (raw `+++` block). For files without existing
    /// frontmatter, the content is written as-is.
    pub fn save_document_content(&self, rel_path: &Path, content: &str) -> Result<()> {
        self.save_document_content_inner(rel_path, content, true)
    }

    /// Move a plain markdown document to another project-relative path.
    ///
    /// This is intentionally path-based rather than title-based: the agent can
    /// organize a document into a better folder without fabricating a rename.
    /// It keeps the document UUID/frontmatter intact, updates the new path in
    /// the index, and removes the stale old row so UI tree views converge after
    /// filesystem watcher events.
    pub fn move_document_file(&self, from_rel: &Path, to_rel: &Path) -> Result<()> {
        validate_document_relative_path(from_rel, "from_path")?;
        validate_document_relative_path(to_rel, "to_path")?;

        if from_rel == to_rel {
            return Ok(());
        }

        let from_abs = self.root.join(from_rel);
        let to_abs = self.root.join(to_rel);
        if !from_abs.exists() {
            anyhow::bail!("source document does not exist: {}", from_rel.display());
        }
        if to_abs.exists() {
            anyhow::bail!("destination document already exists: {}", to_rel.display());
        }

        let raw = fs::read_to_string(&from_abs)?;
        if contains_single_embed(&raw, ".excalidraw") || contains_single_embed(&raw, ".canvas") {
            anyhow::bail!(
                "move_document_file only moves plain markdown notes; use drawing/canvas specific tools for wrapper documents"
            );
        }

        if let Some(parent) = to_abs.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&from_abs, &to_abs)?;
        self.index_file(&to_abs)?;
        if let Some(old_doc) = self.store.get_document_by_path(from_rel)? {
            self.store.delete_document(&old_doc.id)?;
        }
        Ok(())
    }

    /// Same as [`save_document_content`] but does NOT fire the save hook.
    /// Use this for writes that originate from a remote source (forge
    /// pull, etc.) — without it, a pulled change would immediately
    /// re-trigger the push pipeline and create an infinite sync loop.
    pub fn save_document_content_silent(&self, rel_path: &Path, content: &str) -> Result<()> {
        self.save_document_content_inner(rel_path, content, false)
    }

    fn save_document_content_inner(
        &self,
        rel_path: &Path,
        content: &str,
        notify: bool,
    ) -> Result<()> {
        let abs_path = self.root.join(rel_path);
        if let Some(parent) = abs_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let existing_fm = fs::read_to_string(&abs_path)
            .ok()
            .and_then(|raw| extract_raw_frontmatter_block(&raw));
        let to_write = match existing_fm {
            Some(fm_block) => {
                // Trim leading newlines from body; we'll add exactly
                // one separator after the closing fence.
                let body = content.trim_start_matches('\n');
                format!("{fm_block}\n\n{body}")
            }
            None => content.to_string(),
        };
        fs::write(&abs_path, &to_write)?;
        self.index_file(&abs_path)?;
        if notify {
            self.notify_task_saved(rel_path);
        }
        Ok(())
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
            let first_newline = raw
                .find('\n')
                .ok_or_else(|| anyhow::anyhow!("Malformed frontmatter"))?;
            let search_start = first_newline + 1;
            let mut closing_pos = None;
            for (i, line) in raw[search_start..].split('\n').enumerate() {
                let trimmed = line.trim_end_matches('\r').trim();
                if trimmed == "+++" {
                    // Calculate byte offset from start of raw
                    let offset: usize = raw[search_start..]
                        .split('\n')
                        .take(i)
                        .map(|l| l.len() + 1) // +1 for the \n
                        .sum();
                    closing_pos = Some(search_start + offset);
                    break;
                }
            }
            let closing_pos = closing_pos
                .ok_or_else(|| anyhow::anyhow!("Malformed frontmatter: no closing +++"))?;

            let fm_text = &raw[first_newline + 1..closing_pos];
            let closing_line_end = raw[closing_pos..]
                .find('\n')
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

    /// Set a single field inside the `[data]` table of a document's
    /// frontmatter, preserving formatting/order/comments via `toml_edit`.
    ///
    /// Used by the metadata-strip pickers — the operator clicks a pill,
    /// picks a new value, this writes through. Schema enforcement (which
    /// values are valid for which fields) is the picker's job; we just
    /// land the bytes and reindex.
    ///
    /// `value` is a pre-built `toml_edit::Value` so callers can pass
    /// strings, integers (priority), arrays (tags), inline tables, etc.
    /// without us baking in a per-field shape.
    ///
    /// Refuses on discoverable files — same posture as `set_document_kind`.
    pub fn set_data_field(
        &self,
        rel_path: &Path,
        key: &str,
        value: toml_edit::Value,
    ) -> Result<()> {
        self.set_data_field_inner(rel_path, key, value, true)
    }

    /// Same as [`set_data_field`] but does NOT fire the save hook.
    /// Use this for writes that originate from a remote source (forge
    /// pull, etc.) — without it, a pulled field change would
    /// immediately re-trigger the push pipeline.
    pub fn set_data_field_silent(
        &self,
        rel_path: &Path,
        key: &str,
        value: toml_edit::Value,
    ) -> Result<()> {
        self.set_data_field_inner(rel_path, key, value, false)
    }

    fn set_data_field_inner(
        &self,
        rel_path: &Path,
        key: &str,
        value: toml_edit::Value,
        notify: bool,
    ) -> Result<()> {
        if !self.config.indexing.should_write_frontmatter(rel_path) {
            anyhow::bail!(
                "Cannot edit data field on discoverable file {:?} — configure an indexing scope to manage this path",
                rel_path,
            );
        }

        let abs_path = self.root.join(rel_path);
        let raw = fs::read_to_string(&abs_path)
            .with_context(|| format!("read {}", abs_path.display()))?;

        // Pull the frontmatter block (`+++\n...\n+++`) and the body.
        // We need the raw bytes for the body so we don't mangle it on
        // round-trip — toml_edit only operates on the frontmatter.
        let fm_block = extract_raw_frontmatter_block(&raw).ok_or_else(|| {
            anyhow::anyhow!("file has no TOML frontmatter: {}", rel_path.display())
        })?;
        // The block returned includes the closing `+++` line. Strip the
        // delimiters to feed toml_edit just the inner TOML.
        let fm_inner = fm_block
            .trim_start_matches("+++\n")
            .trim_start_matches("+++\r\n")
            .trim_end_matches("+++")
            .trim_end_matches('\n')
            .trim_end_matches('\r');

        let mut doc: toml_edit::DocumentMut = fm_inner
            .parse()
            .with_context(|| format!("parse frontmatter for {}", rel_path.display()))?;

        // Ensure [data] table exists — create empty if absent. Operators
        // converting a plain note to a task hit this path: no [data] yet,
        // we materialize it on the first field write.
        if doc.get("data").is_none() {
            doc.insert("data", toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let data = doc["data"]
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("[data] is not a table in {}", rel_path.display()))?;
        data.insert(key, toml_edit::Item::Value(value));

        // Reassemble: new frontmatter + body. The body starts immediately
        // after the closing `+++` line (matches what extract_raw_frontmatter_block
        // returned).
        let body_start = fm_block.len();
        let body = &raw[body_start..];
        let body = body.strip_prefix('\n').unwrap_or(body);
        let new_raw = format!("+++\n{}+++\n{}", doc, body);

        fs::write(&abs_path, &new_raw).with_context(|| format!("write {}", abs_path.display()))?;
        self.index_file(&abs_path)?;
        if notify {
            // Fan out to the save hook (push debouncer when installed).
            // No-op when no hook is registered (tests, headless agents).
            self.notify_task_saved(rel_path);
        }
        Ok(())
    }

    /// Replace the `[publication]` table for a document while preserving
    /// unrelated frontmatter fields and the markdown body.
    pub fn set_publication_config(
        &self,
        rel_path: &Path,
        publication: &PublicationConfig,
    ) -> Result<()> {
        self.set_publication_config_inner(rel_path, publication, true)
    }

    /// Same as [`set_publication_config`] but does NOT fire the save hook.
    pub fn set_publication_config_silent(
        &self,
        rel_path: &Path,
        publication: &PublicationConfig,
    ) -> Result<()> {
        self.set_publication_config_inner(rel_path, publication, false)
    }

    fn set_publication_config_inner(
        &self,
        rel_path: &Path,
        publication: &PublicationConfig,
        notify: bool,
    ) -> Result<()> {
        if !self.config.indexing.should_write_frontmatter(rel_path) {
            anyhow::bail!(
                "Cannot edit publication settings on discoverable file {:?} — configure an indexing scope to manage this path",
                rel_path,
            );
        }

        let abs_path = self.root.join(rel_path);
        let raw = fs::read_to_string(&abs_path)
            .with_context(|| format!("read {}", abs_path.display()))?;

        let (mut doc, body) = if let Some(fm_block) = extract_raw_frontmatter_block(&raw) {
            let fm_inner = fm_block
                .trim_start_matches("+++\n")
                .trim_start_matches("+++\r\n")
                .trim_end_matches("+++")
                .trim_end_matches('\n')
                .trim_end_matches('\r');
            let doc: toml_edit::DocumentMut = fm_inner
                .parse()
                .with_context(|| format!("parse frontmatter for {}", rel_path.display()))?;
            let body = &raw[fm_block.len()..];
            let body = body.strip_prefix('\n').unwrap_or(body).to_string();
            (doc, body)
        } else {
            (toml_edit::DocumentMut::new(), raw)
        };

        let publication_toml =
            toml::to_string(publication).context("serialize publication config")?;
        let publication_doc: toml_edit::DocumentMut = publication_toml
            .parse()
            .context("parse publication config as editable TOML")?;
        doc.insert(
            "publication",
            toml_edit::Item::Table(publication_doc.as_table().clone()),
        );

        let new_raw = format!("+++\n{}+++\n{}", doc, body);
        fs::write(&abs_path, &new_raw).with_context(|| format!("write {}", abs_path.display()))?;
        self.index_file(&abs_path)?;
        if notify {
            self.notify_task_saved(rel_path);
        }
        Ok(())
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
        frontmatter.metadata.insert(
            "kind".into(),
            MetadataValue::String("agent_communication".into()),
        );

        let document = Document {
            id: DocumentId(
                frontmatter
                    .id
                    .expect("frontmatter id set for communication"),
            ),
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
    pub fn store_memory_fact(&self, topic: &str, title: &str, content: &str) -> Result<PathBuf> {
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

    /// Import markdown documents from an external directory tree into this project.
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
                e.file_type().is_file() && e.path().extension().map(|x| x == "md").unwrap_or(false)
            })
        {
            match self.import_markdown_file(source_root, entry.path()) {
                Ok(ImportDisposition::Imported) => imported += 1,
                Ok(ImportDisposition::Skipped) => skipped += 1,
                Err(err) => errors.push(format!("{}: {err}", entry.path().display())),
            }
        }

        Ok(ImportReport {
            imported,
            skipped,
            errors,
        })
    }

    fn import_markdown_file(
        &self,
        source_root: &Path,
        source_path: &Path,
    ) -> Result<ImportDisposition> {
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
                    let vis = doc_obj
                        .as_ref()
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

        // (Removed: export_project_boards iterated kind="project" entities
        // and rendered board snapshots. With the inner Project entity
        // dissolved, board export — if revived — should iterate boards
        // directly, not via project entities.)

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
                index_mu.push_str(&format!("`[{}`/page/{}.mu]\n", entry.title, entry.slug));
            }
        }
        fs::write(output_root.join("index.mu"), &index_mu)?;

        Ok(PublicationExportReport {
            exported,
            skipped_private,
            errors,
        })
    }

    fn export_published_document(
        &self,
        relative_path: &Path,
        output_root: &Path,
    ) -> Result<Option<PublishedDocument>> {
        let Some(document) = self.store.get_document_by_path(relative_path)? else {
            return Ok(None);
        };
        let visibility = effective_publication_visibility(&document, &self.config.publication);
        if !document.frontmatter.publication.enabled || visibility == PublicationVisibility::Private
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
        let published_micron = render_published_micron(self, &document)?;
        let micron_path = output_root.join(format!("{slug}.mu"));
        fs::write(&output_path, published_markdown)?;
        fs::write(&html_path, published_html)?;
        fs::write(&micron_path, published_micron)?;
        Ok(Some(manifest))
    }

    // save_project_task removed: with the inner Project entity dissolved,
    // every task lives at the project root via save_any_task.

    /// Apply a TaskPatch then write the task to disk via persist_task.
    /// Use instead of `store.update_task` whenever the caller wants the
    /// file representation refreshed (almost always — the file is the
    /// canonical surface for the operator now). Returns false if no
    /// task with that id exists.
    pub fn update_any_task(&self, id: &TaskId, patch: &flynt_models::TaskPatch) -> Result<bool> {
        let Some(mut task) = self.store.get_task(id)? else {
            return Ok(false);
        };
        // Apply the patch in-place — duplicates the inner logic of
        // SqliteStore::update_task to keep that contract a sqlite-only
        // concern. If patch grows new fields, mirror them here.
        if let Some(v) = &patch.column {
            task.column = v.clone();
        }
        if let Some(v) = &patch.title {
            task.title = v.clone();
        }
        if let Some(v) = &patch.description {
            task.description = v.clone();
        }
        if let Some(v) = patch.priority {
            task.priority = v;
        }
        if let Some(v) = patch.status {
            task.status = v;
        }
        if let Some(v) = &patch.tags {
            task.tags = v.clone();
        }
        if let Some(v) = patch.due_date {
            task.due_date = v;
        }
        if let Some(v) = &patch.external_refs {
            task.external_refs = v.clone();
        }
        if let Some(v) = &patch.document_refs {
            task.document_refs = v.clone();
        }
        if let Some(v) = patch.position {
            task.position = v;
        }
        if let Some(v) = patch.decay {
            task.decay = v;
        }
        if let Some(v) = patch.design_node_id {
            task.design_node_id = v;
        }
        if let Some(v) = &patch.openspec_change {
            task.openspec_change = v.clone();
        }
        if let Some(v) = &patch.engagement_id {
            task.engagement_id = v.clone();
        }
        if let Some(v) = &patch.execution {
            task.execution = v.clone();
        }
        task.updated_at = Utc::now();
        self.persist_task(&task)?;
        Ok(true)
    }

    /// Single entry point for persisting a task. Writes the `.md` file
    /// under `Tasks/<board-slug>/<title-slug>.md` and the sqlite row,
    /// and records the file path for future renames. The previous
    /// project_id parameter is gone — every task lives in the
    /// project root, no inner-project routing needed.
    pub fn persist_task(&self, task: &Task) -> Result<()> {
        self.save_any_task(task).map(|_| ())
    }

    /// Delete a task: remove the sqlite row, delete the on-disk `.md`
    /// file if known, and fan out to the save hook so the push pipeline
    /// can drop debouncer entries, status, and sync mappings.
    ///
    /// Returns true if the task existed and was removed. The hook fires
    /// only on the existed-and-deleted path — a no-op delete (task
    /// already gone) doesn't trigger sync cleanup since there's nothing
    /// to clean.
    pub fn delete_task(&self, id: &TaskId) -> Result<bool> {
        // Need to look up the file path before deleting the sqlite row,
        // since the path is stored in task_files alongside the task.
        let existed = self.store.get_task(id)?.is_some();
        if !existed {
            return Ok(false);
        }
        let path = self.store.task_file_path(id)?;
        self.store.delete_task(id)?;
        if let Some(rel) = path {
            let abs = self.root.join(rel.as_str());
            // Best-effort: a missing file is fine (operator may have
            // already deleted it manually); a permissions error gets
            // logged but doesn't fail the delete since the DB row is
            // already gone.
            if let Err(e) = std::fs::remove_file(&abs) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(error = %e, path = %abs.display(), "delete_task: removing file");
                }
            }
        }
        if let Some(hook) = self.save_hook.get() {
            hook.on_task_deleted(id.0);
        }
        Ok(true)
    }

    /// Persist a task: write `.md` file to disk + sqlite + remember the
    /// path for future renames. Every task becomes a file in the project.
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
        let mut desired_rel = PathBuf::from("Tasks")
            .join(&board_slug)
            .join(format!("{title_slug}.md"));

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

        // Step 4: write file.
        std::fs::create_dir_all(&desired_dir)
            .with_context(|| format!("create Tasks dir {}", desired_dir.display()))?;
        let md = task_file::serialize_task_to_markdown(task);
        std::fs::write(&abs, &md).with_context(|| format!("write task file {}", abs.display()))?;

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
    /// `Project::open` so freshly-rebuilt projects converge to the
    /// every-task-is-a-file invariant on next launch.
    fn migrate_tasks_to_files(&self) -> Result<usize> {
        let unfiled = self.store.tasks_without_file()?;
        let mut migrated = 0;
        for tid in unfiled {
            let Some(task) = self.store.get_task(&tid)? else {
                continue;
            };
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

    /// Write a new config to disk. Does not update `self.config` (the in-memory
    /// value is managed by callers via signals). Call this from the settings view.
    pub fn save_config(&self, config: &ProjectConfig) -> Result<()> {
        let config_path = self.root.join(".flynt").join("config.toml");
        fs::write(&config_path, toml::to_string_pretty(config)?)?;
        Ok(())
    }

    pub fn load_bookmarks(&self) -> Result<BookmarkFile> {
        let path = self.bookmarks_path();
        if !path.exists() {
            return Ok(BookmarkFile::default());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("read bookmarks from {}", path.display()))?;
        let bookmarks: BookmarkFile = toml::from_str(&raw)
            .with_context(|| format!("parse bookmarks from {}", path.display()))?;
        Ok(bookmarks)
    }

    pub fn save_bookmarks(&self, bookmarks: &BookmarkFile) -> Result<()> {
        let path = self.bookmarks_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, toml::to_string_pretty(bookmarks)?)?;
        Ok(())
    }

    pub fn add_bookmark(
        &self,
        title: impl Into<String>,
        target: BookmarkTarget,
    ) -> Result<Bookmark> {
        let mut file = self.load_bookmarks()?;
        let target_key = target.stable_key();
        if let Some(existing) = file
            .bookmarks
            .iter_mut()
            .find(|bookmark| bookmark.target.stable_key() == target_key)
        {
            existing.title = title.into();
            let bookmark = existing.clone();
            self.save_bookmarks(&file)?;
            return Ok(bookmark);
        }

        let bookmark = Bookmark {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.into(),
            target,
            created_at: Utc::now(),
        };
        file.bookmarks.push(bookmark.clone());
        self.save_bookmarks(&file)?;
        Ok(bookmark)
    }

    pub fn remove_bookmark(&self, id: &str) -> Result<bool> {
        let mut file = self.load_bookmarks()?;
        let before = file.bookmarks.len();
        file.bookmarks.retain(|bookmark| bookmark.id != id);
        let removed = file.bookmarks.len() != before;
        if removed {
            self.save_bookmarks(&file)?;
        }
        Ok(removed)
    }

    pub fn load_lenses(&self) -> Result<Vec<(PathBuf, ProjectLens)>> {
        let dir = self.lenses_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut lenses = Vec::new();
        for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            let raw =
                fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
            let lens: ProjectLens =
                toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
            let rel = path.strip_prefix(&self.root).unwrap_or(&path).to_path_buf();
            lenses.push((rel, lens));
        }
        lenses.sort_by(|(_, a), (_, b)| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        Ok(lenses)
    }

    pub fn save_lens(&self, lens: &ProjectLens) -> Result<PathBuf> {
        let dir = self.lenses_dir();
        fs::create_dir_all(&dir)?;
        let filename = format!("{}.toml", slugify_title(&lens.title));
        let path = dir.join(filename);
        fs::write(&path, toml::to_string_pretty(lens)?)?;
        Ok(path.strip_prefix(&self.root).unwrap_or(&path).to_path_buf())
    }

    // ── Rename document + update links ────────────────────────────────────────

    /// Rename a document: moves the file on disk, updates frontmatter title,
    /// and rewrites all wikilinks across the project that pointed to the old name.
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
            old_path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });

        // Build the new filename from the new title
        let new_filename = format!("{}.md", new_title);
        let new_path = old_path
            .parent()
            .map(|p| p.join(&new_filename))
            .unwrap_or_else(|| PathBuf::from(&new_filename));

        // Update the frontmatter title in the document content
        let updated_content = if raw.contains("+++") {
            // Replace title in TOML frontmatter
            let new_raw = if let Some(title_line_start) = raw.find("title = \"") {
                let before = &raw[..title_line_start];
                let after_title = &raw[title_line_start..];
                if let Some(end_quote) = after_title[9..].find('"') {
                    format!(
                        "{}title = \"{}\"{}",
                        before,
                        new_title,
                        &after_title[9 + end_quote + 1..]
                    )
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
        let old_stem = old_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        let mut files_updated = 0;
        self.walk_markdown(&mut |path| {
            // Skip the renamed file itself
            let rel = path.strip_prefix(&self.root).unwrap_or(path);
            if rel == new_path {
                return;
            }

            let Ok(content) = fs::read_to_string(path) else {
                return;
            };

            // Check if this file contains wikilinks to the old name
            let mut new_content = content.clone();
            let mut changed = false;

            // Replace [[Old Title]] → [[New Title]]
            let patterns = [format!("[[{}]]", old_title), format!("[[{}]]", old_stem)];
            for pat in &patterns {
                if new_content.contains(pat.as_str()) {
                    new_content = new_content.replace(pat.as_str(), &format!("[[{}]]", new_title));
                    changed = true;
                }
            }

            // Replace [[Old Title|display]] → [[New Title|display]]
            // and [[old_stem|display]] → [[New Title|display]]
            for old_ref in [&old_title, &old_stem.to_string()] {
                let pipe_prefix = format!("[[{}|", old_ref);
                while let Some(start) = new_content.find(&pipe_prefix) {
                    if let Some(end) = new_content[start..].find("]]") {
                        let display =
                            &new_content[start + pipe_prefix.len()..start + end].to_string();
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

    /// List all unique tags across the project with document counts.
    pub fn list_tags(&self) -> Result<Vec<(String, usize)>> {
        let docs = self.store.list_documents()?;
        let mut tag_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for doc in &docs {
            for tag in &doc.tags {
                *tag_counts.entry(tag.clone()).or_default() += 1;
            }
        }
        let mut tags: Vec<(String, usize)> = tag_counts.into_iter().collect();
        tags.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        Ok(tags)
    }

    /// Rename a tag across all documents in the project.
    /// Parses frontmatter as structured data, modifies, re-serializes.
    /// Returns the number of files updated.
    pub fn rename_tag(&self, old_tag: &str, new_tag: &str) -> Result<usize> {
        let mut files_updated = 0;
        self.walk_markdown(&mut |path| {
            let Ok(content) = fs::read_to_string(path) else {
                return;
            };

            let (_body, fm, _links) = flynt_core::parser::parse_document_source(&content);
            if !fm.tags.iter().any(|t| t == old_tag) {
                return;
            }

            // Replace in the tags array by rebuilding it
            let mut new_tags = fm.tags.clone();
            for tag in &mut new_tags {
                if tag == old_tag {
                    *tag = new_tag.to_string();
                }
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
        info!(
            "Renamed tag '{}' → '{}', updated {} file(s)",
            old_tag, new_tag, files_updated
        );
        Ok(files_updated)
    }

    /// Delete a tag from all documents. Returns the number of files updated.
    pub fn delete_tag(&self, tag: &str) -> Result<usize> {
        let mut files_updated = 0;
        self.walk_markdown(&mut |path| {
            let Ok(content) = fs::read_to_string(path) else {
                return;
            };

            let (_body, fm, _links) = flynt_core::parser::parse_document_source(&content);
            if !fm.tags.iter().any(|t| t == tag) {
                return;
            }

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
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut result = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .map(|e| e == "json")
                .unwrap_or(false)
            {
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
        let pending = self
            .root
            .join(format!(".flynt/notifications/pending/{id}.json"));
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

        let tasks = self
            .store
            .list_tasks(&flynt_core::store::TaskFilter::default())?;
        let project_name = self.config.project_name.clone();
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
                        Notification::new(
                            NotificationKind::DueDate,
                            &task.title,
                            body,
                            &project_name,
                        )
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
                        format!(
                            "\"{}\" is losing relevance. Touch it or let it archive.",
                            task.title
                        ),
                        &project_name,
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
                e.file_type().is_file() && e.path().extension().map(|x| x == "md").unwrap_or(false)
            })
        {
            cb(entry.path());
        }
        Ok(())
    }
}

/// If `raw` starts with a `+++` frontmatter block, return that block
/// (including the opening and closing fences) verbatim. Otherwise
/// return None. Used by save_document_content to preserve unknown
/// frontmatter fields when re-writing a document body.
fn extract_raw_frontmatter_block(raw: &str) -> Option<String> {
    let r = raw
        .strip_prefix("+++\n")
        .or_else(|| raw.strip_prefix("+++\r\n"))?;
    let mut offset = 0usize;
    for line in r.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r').trim();
        if trimmed == "+++" {
            // Length of the frontmatter section: prefix "+++\n" + r
            // up through this closing line (without trailing newline).
            let prefix_len = raw.len() - r.len();
            let end_in_r = offset + line.len()
                - (line.len() - line.trim_end_matches('\n').trim_end_matches('\r').len());
            let block = &raw[..prefix_len + end_in_r];
            return Some(block.to_string());
        }
        offset += line.len();
    }
    None
}

fn validate_document_relative_path(path: &Path, label: &str) -> Result<()> {
    if path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        anyhow::bail!("{label} must be a project-relative path without '..'");
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
        anyhow::bail!("{label} must end in .md");
    }
    Ok(())
}

fn contains_single_embed(raw: &str, suffix: &str) -> bool {
    let body = if let Some(rest) = raw.strip_prefix("+++\n") {
        if let Some(end) = rest.find("\n+++") {
            rest[end + 4..].trim()
        } else {
            raw.trim()
        }
    } else {
        raw.trim()
    };
    let lines: Vec<&str> = body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    lines.len() == 1 && {
        let line = lines[0].trim();
        line.starts_with("![[") && line.ends_with(&format!("{suffix}]]"))
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

/// Pull a title out of the entity payload (`[data].title`).
///
/// Entity-typed documents (tasks especially) store their title in the
/// `[data]` block rather than at the top level of frontmatter. Without
/// this fallback, indexed tasks fall through to the filename stem —
/// "flynt-dogfood-ami-bake-pipeline-produces-drifted-manifest" instead
/// of the human title — and the notes view, sidebar, and search all
/// surface the slug.
fn extract_data_title(data: &Option<toml::Value>) -> Option<String> {
    let data = data.as_ref()?;
    let table = data.as_table()?;
    let title = table.get("title")?.as_str()?.trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

fn canonical_document_source(document: &Document) -> String {
    let frontmatter = toml::to_string(&document.frontmatter).unwrap_or_default();
    let body = document.content.trim_end();
    format!("+++\n{frontmatter}\n+++\n\n{body}\n")
}

fn slugify_title(title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
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
            .map(|tag| {
                document
                    .frontmatter
                    .tags
                    .iter()
                    .any(|doc_tag| doc_tag == tag)
            })
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

fn render_published_markdown(project: &Project, document: &Document) -> Result<String> {
    let body =
        rewrite_wikilinks_for_publication(project, &document.content, PublicationRender::Markdown)?;
    let mut frontmatter = document.frontmatter.clone();
    frontmatter.imported_reference = false;
    frontmatter.source_path = None;
    frontmatter.source_format = None;
    frontmatter.imported_at = None;
    let frontmatter = toml::to_string(&frontmatter).unwrap_or_default();
    Ok(format!("+++\n{frontmatter}\n+++\n\n{body}"))
}

fn render_published_html(project: &Project, document: &Document) -> Result<String> {
    let body =
        rewrite_wikilinks_for_publication(project, &document.content, PublicationRender::Html)?;
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

fn render_published_micron(project: &Project, document: &Document) -> Result<String> {
    let body =
        rewrite_wikilinks_for_publication(project, &document.content, PublicationRender::Micron)?;
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

fn rewrite_wikilinks_for_publication(
    project: &Project,
    body: &str,
    mode: PublicationRender,
) -> Result<String> {
    let mut rendered = String::new();
    let mut remaining = body;

    while let Some(start) = remaining.find("[[") {
        rendered.push_str(&remaining[..start]);
        let after = &remaining[start + 2..];
        let Some(end) = after.find("]]") else {
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

        if let Some(linked) = project.store.find_document_by_slug(target)? {
            let Some(linked_doc) = project.store.get_document(&linked.id)? else {
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
    if let Some(path) = runtime
        .flynt_index_db_path
        .as_ref()
        .filter(|path| path.is_absolute())
    {
        return path.clone();
    }

    if let Some(local_state_root) = runtime
        .local_state_root
        .as_ref()
        .filter(|path| path.is_absolute())
    {
        return local_state_root.join("flynt").join("flynt-index.db");
    }

    root.join(".flynt-local")
        .join("flynt")
        .join("flynt-index.db")
}

fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.'))
        .unwrap_or(false)
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
    if !content.starts_with("+++") {
        return None;
    }
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
    let tags_serialized: Vec<String> = new_tags
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect();
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
    let body_start = if content[after_fm..].starts_with('\n') {
        after_fm + 1
    } else {
        after_fm
    };
    Some(format!("+++\n{}+++\n{}", new_fm, &content[body_start..]))
}

#[cfg(test)]
mod tests {
    use super::{
        Project, canonical_document_source, import_destination_path, markdown_to_micron,
        resolve_index_db_path,
    };
    use chrono::Utc;
    use flynt_core::{
        models::{
            BookmarkTarget, Document, DocumentId, Frontmatter, LensColumn, LensFilter,
            LensFilterOp, LensLayout, LensSort, LensSortDirection, LensSource, LocalRuntimeConfig,
            MetadataValue, ProjectLens, PublicationConfig, PublicationRule, PublicationVisibility,
        },
        store::ProjectStore,
    };
    use std::{fs, path::PathBuf};
    use tempfile::TempDir;

    // ── set_data_field ──────────────────────────────────────────────

    fn write_task_file(project: &Project, rel: &str) -> std::path::PathBuf {
        let path = std::path::PathBuf::from(rel);
        let abs = project.root.join(&path);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(
            &abs,
            r#"+++
id = "550e8400-e29b-41d4-a716-446655440000"
kind = "task"

[data]
title = "Original title"
board = "550e8400-e29b-41d4-a716-446655440001"
column = "Active"
priority = 2
status = "todo"
position = 0
+++

Original body content.
"#,
        )
        .unwrap();
        project.index_file(&abs).unwrap();
        path
    }

    // ── save hook ────────────────────────────────────────────────────

    /// Recording hook for tests — pushes every notified id into a Vec.
    struct RecordingHook {
        saved: std::sync::Mutex<Vec<uuid::Uuid>>,
        deleted: std::sync::Mutex<Vec<uuid::Uuid>>,
    }
    impl crate::save_hook::SaveHook for RecordingHook {
        fn on_task_saved(&self, task_id: uuid::Uuid) {
            self.saved.lock().unwrap().push(task_id);
        }
        fn on_task_deleted(&self, task_id: uuid::Uuid) {
            self.deleted.lock().unwrap().push(task_id);
        }
    }
    impl RecordingHook {
        fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self {
                saved: std::sync::Mutex::new(Vec::new()),
                deleted: std::sync::Mutex::new(Vec::new()),
            })
        }
        fn fired(&self) -> Vec<uuid::Uuid> {
            self.saved.lock().unwrap().clone()
        }
        fn deleted(&self) -> Vec<uuid::Uuid> {
            self.deleted.lock().unwrap().clone()
        }
    }

    #[test]
    fn set_data_field_fires_save_hook_for_task_files() {
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let hook = RecordingHook::new();
        project.install_save_hook(hook.clone());

        let path = write_task_file(&project, "Tasks/x/t.md");
        project
            .set_data_field(&path, "status", toml_edit::Value::from("in_progress"))
            .unwrap();

        let fired = hook.fired();
        assert_eq!(fired.len(), 1, "hook fires exactly once per set_data_field");
        // The fired id is the document/task UUID from frontmatter.
        let expected = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(fired[0], expected);
    }

    #[test]
    fn save_hook_does_not_fire_for_plain_notes() {
        // save_document_content on a non-task `.md` shouldn't fire
        // the task-saved hook — push pipeline only cares about tasks.
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let hook = RecordingHook::new();
        project.install_save_hook(hook.clone());

        let path = std::path::PathBuf::from("note.md");
        let abs = project.root.join(&path);
        std::fs::write(&abs, "# Just a plain note\n\nBody.\n").unwrap();
        project.index_file(&abs).unwrap();
        project
            .save_document_content(&path, "Updated body")
            .unwrap();

        assert!(
            hook.fired().is_empty(),
            "no task-saved fire for plain notes"
        );
    }

    #[test]
    fn save_hook_fires_on_save_document_content_for_tasks() {
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let hook = RecordingHook::new();
        project.install_save_hook(hook.clone());

        let path = write_task_file(&project, "Tasks/x/t.md");
        project
            .save_document_content(&path, "New description")
            .unwrap();

        assert_eq!(
            hook.fired().len(),
            1,
            "save_document_content fires once for task files"
        );
    }

    #[test]
    fn save_hook_install_is_idempotent() {
        // First install wins; second call is a no-op (OnceLock).
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let hook1 = RecordingHook::new();
        let hook2 = RecordingHook::new();
        project.install_save_hook(hook1.clone());
        project.install_save_hook(hook2.clone());

        let path = write_task_file(&project, "Tasks/x/t.md");
        project
            .set_data_field(&path, "status", toml_edit::Value::from("done"))
            .unwrap();

        assert_eq!(hook1.fired().len(), 1, "first hook still fires");
        assert!(hook2.fired().is_empty(), "second hook never installed");
    }

    #[test]
    fn set_data_field_silent_does_not_fire_save_hook() {
        // The pull direction (future: forge → local) needs to write
        // without re-triggering push. Without a silent variant, a
        // pulled field change would immediately fan out to the push
        // pipeline and create an infinite sync loop.
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let hook = RecordingHook::new();
        project.install_save_hook(hook.clone());

        let path = write_task_file(&project, "Tasks/x/t.md");
        project
            .set_data_field_silent(&path, "status", toml_edit::Value::from("done"))
            .unwrap();

        assert!(hook.fired().is_empty(), "silent variant must not fire");
        // But the write actually happened.
        let raw = std::fs::read_to_string(project.root.join(&path)).unwrap();
        assert!(
            raw.contains("status = \"done\""),
            "write still happened: {raw}"
        );
    }

    #[test]
    fn save_document_content_silent_does_not_fire_save_hook() {
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let hook = RecordingHook::new();
        project.install_save_hook(hook.clone());

        let path = write_task_file(&project, "Tasks/x/t.md");
        project
            .save_document_content_silent(&path, "Pulled body from upstream")
            .unwrap();

        assert!(hook.fired().is_empty(), "silent variant must not fire");
        let raw = std::fs::read_to_string(project.root.join(&path)).unwrap();
        assert!(
            raw.contains("Pulled body from upstream"),
            "body written: {raw}"
        );
    }

    #[test]
    fn delete_task_fires_hook_and_removes_file() {
        use flynt_models::task::{BoardId, Priority, Task, TaskId, TaskStatus};

        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let hook = RecordingHook::new();
        project.install_save_hook(hook.clone());

        let board_id = BoardId(uuid::Uuid::new_v4());
        let board = flynt_core::models::Board::minimalist("Default");
        let board = flynt_core::models::Board {
            id: board_id.clone(),
            ..board
        };
        project.store.save_board(&board).unwrap();

        let task_id = TaskId(uuid::Uuid::new_v4());
        let task = Task {
            id: task_id.clone(),
            board_id,
            column: "Active".into(),
            title: "Delete me".into(),
            description: "Body".into(),
            priority: Priority::Medium,
            status: TaskStatus::Todo,
            tags: vec![],
            document_refs: vec![],
            external_refs: vec![],
            due_date: None,
            position: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            decay: flynt_models::task::DecayRate::Natural,
            last_touched_at: None,
            design_node_id: None,
            execution: None,
            openspec_change: None,
            engagement_id: None,
        };
        let written_path = project.save_any_task(&task).unwrap();
        let abs = project.root.join(&written_path);
        assert!(abs.exists(), "task file written");

        let removed = project.delete_task(&task_id).unwrap();
        assert!(removed, "task existed and was removed");
        assert!(!abs.exists(), "task file gone");
        assert_eq!(
            hook.deleted(),
            vec![task_id.0],
            "on_task_deleted fired once"
        );
        assert!(
            project.store.get_task(&task_id).unwrap().is_none(),
            "sqlite row gone"
        );
    }

    #[test]
    fn delete_task_missing_id_returns_false_and_no_hook() {
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let hook = RecordingHook::new();
        project.install_save_hook(hook.clone());

        let bogus = flynt_models::task::TaskId(uuid::Uuid::new_v4());
        let removed = project.delete_task(&bogus).unwrap();
        assert!(!removed, "no-op delete returns false");
        assert!(hook.deleted().is_empty(), "no hook fire for no-op delete");
    }

    #[test]
    fn project_without_save_hook_save_succeeds() {
        // No hook installed — saves work normally, just no fan-out.
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let path = write_task_file(&project, "Tasks/x/t.md");
        project
            .set_data_field(&path, "status", toml_edit::Value::from("done"))
            .unwrap();
        // No panic, no error. The point of the test is that we don't
        // require a hook for set_data_field to function.
    }

    #[test]
    fn set_data_field_updates_status_preserves_body() {
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let path = write_task_file(&project, "Tasks/x/t.md");

        project
            .set_data_field(&path, "status", toml_edit::Value::from("in_progress"))
            .unwrap();

        let raw = std::fs::read_to_string(project.root.join(&path)).unwrap();
        assert!(raw.contains("status = \"in_progress\""), "{raw}");
        assert!(
            raw.contains("Original body content"),
            "body preserved: {raw}"
        );
        assert!(
            !raw.contains("status = \"todo\""),
            "old status removed: {raw}"
        );
    }

    #[test]
    fn set_data_field_updates_priority_int() {
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let path = write_task_file(&project, "Tasks/x/t.md");

        project
            .set_data_field(&path, "priority", toml_edit::Value::from(4i64))
            .unwrap();
        let raw = std::fs::read_to_string(project.root.join(&path)).unwrap();
        assert!(raw.contains("priority = 4"), "{raw}");
    }

    #[test]
    fn set_data_field_updates_tags_array() {
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let path = write_task_file(&project, "Tasks/x/t.md");

        let mut tags = toml_edit::Array::new();
        tags.push("infra");
        tags.push("pipeline");
        project
            .set_data_field(&path, "tags", toml_edit::Value::Array(tags))
            .unwrap();

        let raw = std::fs::read_to_string(project.root.join(&path)).unwrap();
        assert!(raw.contains("tags = [\"infra\", \"pipeline\"]"), "{raw}");
    }

    #[test]
    fn set_data_field_preserves_unrelated_fields() {
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let path = write_task_file(&project, "Tasks/x/t.md");

        project
            .set_data_field(&path, "status", toml_edit::Value::from("done"))
            .unwrap();
        let raw = std::fs::read_to_string(project.root.join(&path)).unwrap();

        // Title, board, column, priority, position all survive.
        assert!(raw.contains("title = \"Original title\""), "{raw}");
        assert!(raw.contains("column = \"Active\""), "{raw}");
        assert!(raw.contains("priority = 2"), "{raw}");
        assert!(raw.contains("position = 0"), "{raw}");
    }

    #[test]
    fn set_data_field_creates_data_table_when_absent() {
        // Edge: a fresh document with frontmatter but no [data] block.
        // Setting a data field should materialize the table inline.
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();

        let path = std::path::PathBuf::from("note.md");
        let abs = project.root.join(&path);
        std::fs::write(
            &abs,
            "+++\nid = \"550e8400-e29b-41d4-a716-446655440000\"\nkind = \"task\"\n+++\n\nbody\n",
        )
        .unwrap();
        project.index_file(&abs).unwrap();

        project
            .set_data_field(&path, "status", toml_edit::Value::from("todo"))
            .unwrap();
        let raw = std::fs::read_to_string(&abs).unwrap();
        assert!(raw.contains("[data]"), "[data] table created: {raw}");
        assert!(raw.contains("status = \"todo\""), "{raw}");
        assert!(raw.contains("body"), "body preserved: {raw}");
    }

    #[test]
    fn set_data_field_reindexes_so_changes_show_up_immediately() {
        // Mutating a field should refresh the indexed Document — the
        // notes view's doc_data signal observes that update.
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let path = write_task_file(&project, "Tasks/x/t.md");

        project
            .set_data_field(&path, "title", toml_edit::Value::from("Renamed"))
            .unwrap();

        let doc = project.store.get_document_by_path(&path).unwrap().unwrap();
        assert_eq!(
            doc.title, "Renamed",
            "indexed title reflects the new [data].title"
        );
    }

    #[test]
    fn fresh_project_open_creates_default_board() {
        // Convert-to-task and the kanban both need a board to land
        // tasks on. ensure_default_board materializes a "Default"
        // minimalist (Active / Archive) board on first open.
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let boards = project.store.list_boards().unwrap();
        assert_eq!(boards.len(), 1, "fresh project has the Default board");
        assert_eq!(boards[0].name, "Default");
        assert_eq!(boards[0].columns.len(), 2);
        assert_eq!(boards[0].columns[0].name, "Active");
        assert_eq!(boards[0].columns[1].name, "Archive");
    }

    #[test]
    fn ensure_default_board_is_idempotent_on_reopen() {
        // Reopen the same project — Default board doesn't get
        // duplicated. Operator-created boards likewise survive untouched.
        let tmp = TempDir::new().unwrap();
        {
            let project = Project::open(tmp.path()).unwrap();
            // Add a sibling board so we exercise both branches.
            project
                .store
                .save_board(&flynt_core::models::Board::minimalist("Personal"))
                .unwrap();
        }
        let project = Project::open(tmp.path()).unwrap();
        let boards = project.store.list_boards().unwrap();
        assert_eq!(boards.len(), 2, "no duplicate Default on reopen");
        let names: Vec<&str> = boards.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"Default"));
        assert!(names.contains(&"Personal"));
    }

    #[test]
    fn convert_to_task_via_set_kind_and_data_fields() {
        // Reproduces the path the palette's "Convert to Task" command
        // takes: existing plain note → set_document_kind("task") →
        // set_data_field for each required field. Verifies the
        // resulting frontmatter parses cleanly as a task and the body
        // survives.
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        let board = project
            .store
            .list_boards()
            .unwrap()
            .into_iter()
            .find(|b| b.name == "Default")
            .expect("Default board materialized by ensure_default_board");

        let path = std::path::PathBuf::from("note.md");
        let abs = project.root.join(&path);
        std::fs::write(&abs, "# Investigate the indexer\n\nNotes about the bug.\n").unwrap();
        project.index_file(&abs).unwrap();

        // Apply the same sequence the palette command runs.
        project.set_document_kind(&path, Some("task")).unwrap();
        project
            .set_data_field(
                &path,
                "title",
                toml_edit::Value::from("Investigate the indexer"),
            )
            .unwrap();
        project
            .set_data_field(
                &path,
                "board",
                toml_edit::Value::from(board.id.0.to_string()),
            )
            .unwrap();
        project
            .set_data_field(&path, "column", toml_edit::Value::from("Active"))
            .unwrap();
        project
            .set_data_field(&path, "status", toml_edit::Value::from("todo"))
            .unwrap();
        project
            .set_data_field(&path, "priority", toml_edit::Value::from(2_i64))
            .unwrap();
        project
            .set_data_field(&path, "position", toml_edit::Value::from(0_i64))
            .unwrap();

        let raw = std::fs::read_to_string(&abs).unwrap();
        // Sanity: kind set, [data] populated, body preserved.
        assert!(raw.contains("kind = \"task\""), "{raw}");
        assert!(raw.contains("title = \"Investigate the indexer\""));
        assert!(raw.contains("status = \"todo\""));
        assert!(raw.contains("priority = 2"));
        assert!(
            raw.contains("Notes about the bug."),
            "body preserved: {raw}"
        );

        // The indexed Document picks up the [data].title (via the title
        // resolution chain we fixed earlier) — this is the user-facing
        // "task title shows correctly in the tab bar" guarantee.
        let doc = project.store.get_document_by_path(&path).unwrap().unwrap();
        assert_eq!(doc.title, "Investigate the indexer");
    }

    #[test]
    fn ensure_default_board_skips_when_other_boards_exist() {
        // Project that has a board (but not Default) — we don't add
        // Default after the fact. The "fresh project" semantic is
        // "had zero boards at open time."
        let tmp = TempDir::new().unwrap();
        let project = Project::open(tmp.path()).unwrap();
        // Replace the auto-created Default with a custom one.
        for b in project.store.list_boards().unwrap() {
            project.store.delete_board(&b.id).unwrap();
        }
        project
            .store
            .save_board(&flynt_core::models::Board::default_sprint("Sprint 1"))
            .unwrap();
        // Now run ensure_default_board directly — should be a no-op.
        project.ensure_default_board().unwrap();
        let boards = project.store.list_boards().unwrap();
        assert_eq!(
            boards.len(),
            1,
            "no Default re-added when other board exists"
        );
        assert_eq!(boards[0].name, "Sprint 1");
    }

    #[test]
    fn task_files_index_with_data_title_not_filename_slug() {
        // Tasks store their title in [data].title, not at the top level
        // of frontmatter. Without the data-title fallback step in
        // index_file, the indexer fell through to the filename stem,
        // surfacing slugs like "ami-bake-pipeline-produces-drifted-
        // manifest" everywhere in the UI instead of the human title.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("project");
        let project = Project::open(&root).unwrap();

        let task_path =
            PathBuf::from("Tasks/sprint/ami-bake-pipeline-produces-drifted-manifest.md");
        let abs = project.root.join(&task_path);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        // Real shape produced by `task_file::serialize_task_to_markdown`:
        // title in [data], empty body for description-less tasks.
        std::fs::write(
            &abs,
            r#"+++
id = "550e8400-e29b-41d4-a716-446655440000"
kind = "task"

[data]
title = "AMI bake pipeline produces drifted manifest"
board = "550e8400-e29b-41d4-a716-446655440001"
column = "Backlog"
priority = 2
status = "todo"
position = 0
+++
"#,
        )
        .unwrap();

        project.index_file(&abs).unwrap();

        let indexed = project
            .store
            .get_document_by_path(&task_path)
            .unwrap()
            .expect("task is indexed");
        assert_eq!(
            indexed.title, "AMI bake pipeline produces drifted manifest",
            "task title should come from [data].title, not filename slug"
        );
    }

    #[test]
    fn frontmatter_top_level_title_still_wins_over_data_title() {
        // Document with both top-level title and [data].title — top
        // level wins, matching prior behavior. Guards against regressing
        // the resolution order when adding the data-title step.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("project");
        let project = Project::open(&root).unwrap();

        let doc_path = PathBuf::from("note.md");
        let abs = project.root.join(&doc_path);
        std::fs::write(
            &abs,
            r#"+++
id = "550e8400-e29b-41d4-a716-446655440000"
title = "Top level wins"
kind = "design_node"

[data]
title = "Data block loses"
+++

Body
"#,
        )
        .unwrap();

        project.index_file(&abs).unwrap();

        let indexed = project
            .store
            .get_document_by_path(&doc_path)
            .unwrap()
            .unwrap();
        assert_eq!(indexed.title, "Top level wins");
    }

    #[test]
    fn empty_data_title_falls_through_to_filename() {
        // Edge: [data].title is an empty string. The fallback should
        // skip it (treating empty as None) and continue to the filename
        // stem — otherwise we'd index a doc with title="".
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("project");
        let project = Project::open(&root).unwrap();

        let doc_path = PathBuf::from("Tasks/sprint/empty-title.md");
        let abs = project.root.join(&doc_path);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(
            &abs,
            r#"+++
id = "550e8400-e29b-41d4-a716-446655440000"
kind = "task"

[data]
title = ""
board = "550e8400-e29b-41d4-a716-446655440001"
column = "Backlog"
priority = 2
status = "todo"
position = 0
+++
"#,
        )
        .unwrap();

        project.index_file(&abs).unwrap();
        let indexed = project
            .store
            .get_document_by_path(&doc_path)
            .unwrap()
            .unwrap();
        assert_eq!(indexed.title, "empty-title");
    }

    #[test]
    fn uses_explicit_absolute_index_db_path_when_configured() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("project");
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
        let root = tmp.path().join("project");
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
        let project_root = tmp.path().join("project");
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

        let project = Project::open(&project_root).unwrap();
        let report = project.import_markdown_tree(&source_root).unwrap();
        assert_eq!(report.imported, 1);
        assert!(report.errors.is_empty());

        let imported_rel = import_destination_path(std::path::Path::new("notes/design.md"));
        let imported_doc = project
            .store
            .get_document_by_path(&imported_rel)
            .unwrap()
            .unwrap();
        assert_eq!(imported_doc.title, "Design");
        assert_eq!(imported_doc.outgoing_links.len(), 1);
        assert_eq!(imported_doc.outgoing_links[0].target, "roadmap");
        assert_eq!(
            imported_doc.frontmatter.source_format.as_deref(),
            Some("markdown")
        );
        assert_eq!(
            imported_doc.frontmatter.source_path.as_deref(),
            Some(
                source_root
                    .join("notes/design.md")
                    .display()
                    .to_string()
                    .as_str()
            )
        );
        assert!(imported_doc.frontmatter.imported_reference);
        assert!(imported_doc.frontmatter.id.is_some());
        assert_eq!(
            imported_doc.frontmatter.metadata.get("owner"),
            Some(&MetadataValue::String("alpharius".into()))
        );

        let imported_meta = project
            .store
            .get_document_by_path(&imported_rel)
            .unwrap()
            .unwrap();
        assert_eq!(imported_meta.path, imported_rel);
    }

    #[test]
    fn stores_agent_communication_under_references_comms_with_metadata_and_links() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        let project = Project::open(&project_root).unwrap();

        let relative_path = project
            .store_agent_communication("vox", "Standup Recall", "See [[design]].")
            .unwrap();

        assert!(relative_path.starts_with("references/comms/vox"));
        let doc = project
            .store
            .get_document_by_path(&relative_path)
            .unwrap()
            .unwrap();
        assert_eq!(doc.title, "Standup Recall");
        assert_eq!(
            doc.frontmatter.source_format.as_deref(),
            Some("omegon_comm")
        );
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
        let project_root = tmp.path().join("project");
        let project = Project::open(&project_root).unwrap();

        let relative_path = project
            .store_memory_fact("storage", "Canonical vs Local", "Supports [[design]].")
            .unwrap();

        assert!(relative_path.starts_with("ai/memory/storage"));
        let doc = project
            .store
            .get_document_by_path(&relative_path)
            .unwrap()
            .unwrap();
        assert_eq!(doc.title, "Canonical vs Local");
        assert_eq!(
            doc.frontmatter.source_format.as_deref(),
            Some("omegon_memory")
        );
        assert_eq!(
            doc.frontmatter.source_path.as_deref(),
            Some("omegon://memory/storage")
        );
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
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let output_root = tmp.path().join("published");
        let project = Project::open(&project_root).unwrap();
        assert!(project.store.list_documents().unwrap().is_empty());

        for (name, title) in [("alpha.md", "Same"), ("beta.md", "Same")] {
            let path = project_root.join(name);
            std::fs::write(
                &path,
                format!(
                    "+++\ntitle = \"{title}\"\n[publication]\nenabled = true\nvisibility = \"public\"\n+++\n\n# {title}\n"
                ),
            )
            .unwrap();
            project.index_file(&path).unwrap();
        }

        let report = project.export_publication_tree(&output_root).unwrap();
        assert_eq!(report.exported, 1);
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].contains("duplicate publication slug"));
    }

    #[test]
    fn publication_policy_rules_can_publish_selected_subset() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();
        assert!(project.store.list_documents().unwrap().is_empty());

        let public_path = project_root.join("docs/public.md");
        std::fs::create_dir_all(public_path.parent().unwrap()).unwrap();
        std::fs::write(
            &public_path,
            "+++\ntitle = \"Public\"\ntags = [\"public\"]\n[publication]\nenabled = true\n+++\n\n# Public\n",
        )
        .unwrap();
        project.index_file(&public_path).unwrap();

        let private_path = project_root.join("private.md");
        std::fs::write(
            &private_path,
            "+++\ntitle = \"Private\"\n[publication]\nenabled = true\n+++\n\n# Private\n",
        )
        .unwrap();
        project.index_file(&private_path).unwrap();

        let mut config = project.config.clone();
        config.publication.default_visibility = PublicationVisibility::Private;
        config.publication.rules = vec![PublicationRule {
            match_tag: Some("public".into()),
            match_path_prefix: None,
            visibility: PublicationVisibility::Public,
        }];
        project.save_config(&config).unwrap();
        let filtered_project = Project::open(&project_root).unwrap();

        let report = filtered_project
            .export_publication_tree(&tmp.path().join("published"))
            .unwrap();
        assert_eq!(report.exported, 1);
        assert_eq!(report.skipped_private, 1);
    }

    #[test]
    fn set_publication_config_preserves_frontmatter_and_body() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();
        let path = PathBuf::from("Publish Me.md");
        let raw = "+++\ntitle = \"Publish Me\"\ntags = [\"docs\"]\ncustom = \"keep\"\n\n[data]\nstatus = \"draft\"\n+++\n\n# Publish Me\n\nBody stays here.\n";
        fs::write(project.root.join(&path), raw).unwrap();
        project.index_file(&project.root.join(&path)).unwrap();

        let publication = PublicationConfig {
            enabled: true,
            slug: Some("publish-me".into()),
            visibility: PublicationVisibility::Unlisted,
            target: None,
            collections: vec!["guides".into(), "release".into()],
        };
        project.set_publication_config(&path, &publication).unwrap();

        let updated = fs::read_to_string(project.root.join(&path)).unwrap();
        assert!(updated.contains("custom = \"keep\""));
        assert!(updated.contains("[data]\nstatus = \"draft\""));
        assert!(updated.contains("[publication]"));
        assert!(updated.contains("enabled = true"));
        assert!(updated.contains("slug = \"publish-me\""));
        assert!(updated.contains("visibility = \"unlisted\""));
        assert!(updated.contains("collections = [\"guides\", \"release\"]"));
        assert!(updated.contains("# Publish Me\n\nBody stays here."));

        let doc = project.store.get_document_by_path(&path).unwrap().unwrap();
        assert!(doc.frontmatter.publication.enabled);
        assert_eq!(
            doc.frontmatter.publication.visibility,
            PublicationVisibility::Unlisted
        );
        assert_eq!(
            doc.frontmatter.publication.slug.as_deref(),
            Some("publish-me")
        );
    }

    #[test]
    fn bookmarks_round_trip_and_dedupe_by_target() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();

        let note_id = DocumentId(uuid::Uuid::new_v4());
        let note_target = BookmarkTarget::Note {
            document_id: Some(note_id),
            path: PathBuf::from("notes/alpha.md"),
        };
        let first = project
            .add_bookmark("Alpha", note_target.clone())
            .expect("add note bookmark");
        let second = project
            .add_bookmark("Alpha Updated", note_target)
            .expect("update note bookmark");
        assert_eq!(first.id, second.id);

        let search = project
            .add_bookmark(
                "Search: manager failover",
                BookmarkTarget::Search {
                    query: "manager failover".into(),
                },
            )
            .expect("add search bookmark");

        let reloaded = project.load_bookmarks().expect("load bookmarks");
        assert_eq!(reloaded.version, 1);
        assert_eq!(reloaded.bookmarks.len(), 2);
        assert_eq!(reloaded.bookmarks[0].title, "Alpha Updated");
        assert!(matches!(
            reloaded.bookmarks[1].target,
            BookmarkTarget::Search { .. }
        ));

        assert!(
            project
                .remove_bookmark(&search.id)
                .expect("remove bookmark")
        );
        assert_eq!(project.load_bookmarks().unwrap().bookmarks.len(), 1);
    }

    #[test]
    fn lenses_round_trip_as_definition_files() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();

        let lens = ProjectLens {
            title: "Publication Candidates".into(),
            source: LensSource::Documents,
            layout: LensLayout::Table,
            filters: vec![LensFilter {
                field: "tags".into(),
                op: LensFilterOp::Contains,
                value: "publish".into(),
            }],
            columns: vec![
                LensColumn {
                    field: "title".into(),
                    label: None,
                },
                LensColumn {
                    field: "publication.visibility".into(),
                    label: Some("Visibility".into()),
                },
            ],
            sort: vec![LensSort {
                field: "updated_at".into(),
                direction: LensSortDirection::Desc,
            }],
            limit: Some(50),
        };

        let rel_path = project.save_lens(&lens).expect("save lens");
        assert_eq!(
            rel_path,
            PathBuf::from(".flynt/lenses/publication-candidates.toml")
        );
        let raw = fs::read_to_string(project.root.join(&rel_path)).unwrap();
        assert!(raw.contains("title = \"Publication Candidates\""));
        assert!(!raw.contains("rows"));
        assert!(!raw.contains("results"));

        let loaded = project.load_lenses().expect("load lenses");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0, rel_path);
        assert_eq!(loaded[0].1, lens);
    }

    #[test]
    fn exports_public_documents_with_resolved_wikilinks() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        let output_root = tmp.path().join("published");
        let project = Project::open(&project_root).unwrap();

        let roadmap_path = project_root.join("roadmap.md");
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
        project.index_file(&roadmap_path).unwrap();

        let design_path = project_root.join("design.md");
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
        project.index_file(&design_path).unwrap();

        let report = project.export_publication_tree(&output_root).unwrap();
        assert_eq!(report.exported, 2);
        assert!(report.errors.is_empty());

        let published = std::fs::read_to_string(output_root.join("design.md")).unwrap();
        assert!(published.contains("[the roadmap](/roadmap)"));
        assert!(!published.contains("source_path"));

        let html = std::fs::read_to_string(output_root.join("design.html")).unwrap();
        assert!(html.contains("href=\"/roadmap.html\""));

        // Micron export
        let micron = std::fs::read_to_string(output_root.join("design.mu")).unwrap();
        assert!(
            micron.contains("`[the roadmap`/page/roadmap.mu]"),
            "micron wikilink: {micron}"
        );
        assert!(micron.contains(">`!Design`"), "micron heading: {micron}");

        // NomadNet index page
        let index = std::fs::read_to_string(output_root.join("index.mu")).unwrap();
        assert!(
            index.contains("`[Design`/page/design.mu]"),
            "index entry: {index}"
        );
        assert!(
            index.contains("`[Roadmap`/page/roadmap.mu]"),
            "index entry: {index}"
        );

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

    #[test]
    fn design_node_indexing_roundtrip() {
        use flynt_core::datum::EntityKind;

        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();

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

        let design_dir = project_root.join("design");
        std::fs::create_dir_all(&design_dir).unwrap();
        let file_path = design_dir.join("auth-subsystem.md");
        std::fs::write(&file_path, &design_md).unwrap();

        // Reindex the project
        let (indexed, errors) = project.reindex().unwrap();
        assert!(indexed >= 1, "should index at least the design node file");
        assert!(errors.is_empty(), "reindex errors: {errors:?}");

        // Retrieve the document
        let doc_id = flynt_core::models::DocumentId(node_id);
        let doc = project.store.get_document(&doc_id).unwrap();
        assert!(doc.is_some(), "design node document should exist in store");
        let doc = doc.unwrap();

        // Verify entity kind
        assert!(doc.entity.is_some(), "document should have an entity");
        let entity = doc.entity.as_ref().unwrap();
        assert_eq!(
            entity.kind,
            EntityKind::DesignNode,
            "entity kind should be DesignNode"
        );

        // Verify entity fields
        assert_eq!(entity.get_text("status"), Some("exploring"));
        assert_eq!(entity.get_text("parent").unwrap(), parent_id.to_string());
        assert_eq!(entity.get_int("priority"), Some(3));
        assert_eq!(entity.get_text_list("dependencies"), vec!["dep-a", "dep-b"]);
        assert_eq!(
            entity.get_text_list("open_questions"),
            vec!["Which OAuth flow?", "Token rotation?"]
        );

        // Verify it shows up in list_entities_by_kind
        let design_nodes = project
            .store
            .list_entities_by_kind(&EntityKind::DesignNode)
            .unwrap();
        assert!(
            design_nodes.iter().any(|m| m.id == doc_id),
            "design node should appear in kind listing"
        );
    }

    #[test]
    fn set_document_kind_on_existing_frontmatter() {
        use flynt_core::datum::EntityKind;

        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();

        // Create a plain document with frontmatter
        let content =
            "+++\ntitle = \"My Note\"\ntags = [\"test\"]\n+++\n\n# My Note\n\nSome content.\n";
        let path = std::path::PathBuf::from("my-note.md");
        std::fs::write(project_root.join(&path), content).unwrap();
        project.reindex().unwrap();

        // Set kind to design_node
        project
            .set_document_kind(&path, Some("design_node"))
            .unwrap();

        // Re-read the file and verify
        let updated = std::fs::read_to_string(project_root.join(&path)).unwrap();
        assert!(
            updated.contains("kind = \"design_node\""),
            "should contain kind field: {updated}"
        );
        assert!(
            updated.contains("title = \"My Note\""),
            "should preserve title"
        );
        assert!(
            updated.contains("tags = [\"test\"]"),
            "should preserve tags"
        );
        assert!(updated.contains("# My Note"), "should preserve body");

        // Reindex and verify entity kind
        project.reindex().unwrap();
        let docs = project.store.list_documents().unwrap();
        let doc = docs.iter().find(|d| d.title == "My Note").unwrap();
        assert_eq!(doc.entity_kind, Some(EntityKind::DesignNode));
    }

    #[test]
    fn set_document_kind_replaces_existing_kind() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();

        let content = "+++\ntitle = \"Task Doc\"\nkind = \"task\"\n+++\n\nBody\n";
        let path = std::path::PathBuf::from("task-doc.md");
        std::fs::write(project_root.join(&path), content).unwrap();

        // Change kind from task to project
        project.set_document_kind(&path, Some("project")).unwrap();
        let updated = std::fs::read_to_string(project_root.join(&path)).unwrap();
        assert!(
            updated.contains("kind = \"project\""),
            "should have new kind: {updated}"
        );
        assert!(
            !updated.contains("kind = \"task\""),
            "should not have old kind: {updated}"
        );
    }

    #[test]
    fn set_document_kind_clear_removes_kind() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();

        let content = "+++\ntitle = \"Typed\"\nkind = \"design_node\"\n+++\n\nBody\n";
        let path = std::path::PathBuf::from("typed.md");
        std::fs::write(project_root.join(&path), content).unwrap();

        // Clear the kind
        project.set_document_kind(&path, None).unwrap();
        let updated = std::fs::read_to_string(project_root.join(&path)).unwrap();
        assert!(
            !updated.contains("kind ="),
            "should not contain kind: {updated}"
        );
        assert!(
            updated.contains("title = \"Typed\""),
            "should preserve title"
        );
    }

    #[test]
    fn set_document_kind_no_frontmatter_creates_one() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();

        let content = "# Just a note\n\nNo frontmatter here.\n";
        let path = std::path::PathBuf::from("plain.md");
        std::fs::write(project_root.join(&path), content).unwrap();

        project
            .set_document_kind(&path, Some("design_node"))
            .unwrap();
        let updated = std::fs::read_to_string(project_root.join(&path)).unwrap();
        assert!(
            updated.starts_with("+++\n"),
            "should start with frontmatter"
        );
        assert!(
            updated.contains("kind = \"design_node\""),
            "should contain kind"
        );
        assert!(updated.contains("# Just a note"), "should preserve body");
    }

    #[test]
    fn set_document_kind_clear_on_plain_doc_is_noop() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();

        let content = "# Plain\n\nNo kind to clear.\n";
        let path = std::path::PathBuf::from("noop.md");
        std::fs::write(project_root.join(&path), content).unwrap();

        project.set_document_kind(&path, None).unwrap();
        let updated = std::fs::read_to_string(project_root.join(&path)).unwrap();
        // File gets reindexed (frontmatter added by indexer) but no kind field
        assert!(
            !updated.contains("kind ="),
            "should not contain kind: {updated}"
        );
        assert!(updated.contains("# Plain"), "should preserve body");
    }

    #[test]
    fn set_document_kind_preserves_data_table() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();

        let content = "+++\ntitle = \"Design\"\nkind = \"design_node\"\n\n[data]\nstatus = \"exploring\"\npriority = 5\n+++\n\n# Design\n";
        let path = std::path::PathBuf::from("with-data.md");
        std::fs::write(project_root.join(&path), content).unwrap();

        // Change kind — [data] table should be preserved
        project.set_document_kind(&path, Some("project")).unwrap();
        let updated = std::fs::read_to_string(project_root.join(&path)).unwrap();
        assert!(
            updated.contains("kind = \"project\""),
            "new kind: {updated}"
        );
        assert!(
            updated.contains("[data]"),
            "should preserve data table: {updated}"
        );
        assert!(
            updated.contains("status = \"exploring\""),
            "should preserve data fields: {updated}"
        );
        assert!(
            updated.contains("priority = 5"),
            "should preserve data fields: {updated}"
        );
    }

    #[test]
    fn set_document_kind_does_not_match_similar_field_names() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();

        // "kind_of_thing" should NOT be matched as the "kind" field
        let content = "+++\ntitle = \"Test\"\nkind_of_thing = \"misc\"\n+++\n\nBody\n";
        let path = std::path::PathBuf::from("similar.md");
        std::fs::write(project_root.join(&path), content).unwrap();

        project.set_document_kind(&path, Some("project")).unwrap();
        let updated = std::fs::read_to_string(project_root.join(&path)).unwrap();
        assert!(
            updated.contains("kind = \"project\""),
            "should have kind: {updated}"
        );
        assert!(
            updated.contains("kind_of_thing = \"misc\""),
            "should preserve similar field: {updated}"
        );
    }

    #[test]
    fn set_document_kind_does_not_remove_kind_inside_data_table() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();

        // kind inside [data] should be preserved
        let content = "+++\ntitle = \"Node\"\nkind = \"design_node\"\n\n[data]\nkind = \"subtype-a\"\nstatus = \"active\"\n+++\n\nBody\n";
        let path = std::path::PathBuf::from("nested-kind.md");
        std::fs::write(project_root.join(&path), content).unwrap();

        project.set_document_kind(&path, Some("project")).unwrap();
        let updated = std::fs::read_to_string(project_root.join(&path)).unwrap();
        assert!(
            updated.contains("kind = \"project\""),
            "should have new top-level kind: {updated}"
        );
        // The [data] section's kind field should be preserved
        assert!(
            updated.contains("kind = \"subtype-a\""),
            "should preserve kind inside [data]: {updated}"
        );
    }

    #[test]
    fn set_document_kind_handles_crlf_line_endings() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();

        // CRLF line endings
        let content = "+++\r\ntitle = \"CRLF\"\r\nkind = \"task\"\r\n+++\r\n\r\nBody\r\n";
        let path = std::path::PathBuf::from("crlf.md");
        std::fs::write(project_root.join(&path), content).unwrap();

        project
            .set_document_kind(&path, Some("design_node"))
            .unwrap();
        let updated = std::fs::read_to_string(project_root.join(&path)).unwrap();
        assert!(
            updated.contains("kind = \"design_node\""),
            "should have new kind: {updated}"
        );
        assert!(
            !updated.contains("kind = \"task\""),
            "should not have old kind: {updated}"
        );
        assert!(updated.contains("Body"), "should preserve body: {updated}");
    }

    #[test]
    fn graph_edges_from_wikilinks() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = Project::open(&project_root).unwrap();

        // Create two documents with wikilinks between them
        let id_a = uuid::Uuid::new_v4();
        let id_b = uuid::Uuid::new_v4();
        std::fs::write(
            project_root.join("alpha.md"),
            format!("+++\nid = \"{id_a}\"\ntitle = \"Alpha\"\n+++\n\n# Alpha\n\nSee [[Beta]] for details.\n"),
        ).unwrap();
        std::fs::write(
            project_root.join("beta.md"),
            format!("+++\nid = \"{id_b}\"\ntitle = \"Beta\"\n+++\n\n# Beta\n\nRefer back to [[Alpha]].\n"),
        ).unwrap();

        let (indexed, errors) = project.reindex().unwrap();
        assert_eq!(indexed, 2, "should index both files");
        assert!(errors.is_empty());

        // Verify outgoing links are stored
        let doc_a = project
            .store
            .get_document(&flynt_core::models::DocumentId(id_a))
            .unwrap()
            .unwrap();
        assert_eq!(doc_a.outgoing_links.len(), 1, "alpha should link to beta");
        assert_eq!(doc_a.outgoing_links[0].target.to_lowercase(), "beta");

        let doc_b = project
            .store
            .get_document(&flynt_core::models::DocumentId(id_b))
            .unwrap()
            .unwrap();
        assert_eq!(doc_b.outgoing_links.len(), 1, "beta should link to alpha");
        assert_eq!(doc_b.outgoing_links[0].target.to_lowercase(), "alpha");

        // Build graph and verify edges
        let graph = flynt_core::graph::build_graph_payload(&*project.store).unwrap();
        assert!(graph.nodes.len() >= 2, "should have at least 2 nodes");
        assert!(
            !graph.edges.is_empty(),
            "should have wikilink edges, got: {:?}",
            graph.edges
        );

        let wikilink_edges: Vec<_> = graph
            .edges
            .iter()
            .filter(|e| matches!(e.kind, flynt_core::graph::GraphEdgeKind::Wikilink))
            .collect();
        assert_eq!(
            wikilink_edges.len(),
            2,
            "should have 2 wikilink edges (A→B and B→A), got {}",
            wikilink_edges.len()
        );
    }

    // ── save_any_task: every task becomes a file ────────────────────────────

    fn project_with_board() -> (TempDir, Project, flynt_core::models::Board) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let project = Project::open(&root).unwrap();
        let board = flynt_core::models::Board::default_sprint("Sprint 1");
        project.store.save_board(&board).unwrap();
        (tmp, project, board)
    }

    #[test]
    fn save_any_task_writes_file_and_records_path() {
        let (_tmp, project, board) = project_with_board();
        let task = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Auth Rewrite");
        let rel = project.save_any_task(&task).unwrap();
        assert_eq!(
            rel,
            std::path::PathBuf::from("Tasks/sprint-1/auth-rewrite.md")
        );
        assert!(project.root.join(&rel).exists(), "file must exist on disk");
        assert_eq!(
            project.store.task_file_path(&task.id).unwrap().as_deref(),
            Some(rel.to_string_lossy().as_ref()),
        );
    }

    #[test]
    fn save_any_task_renames_file_when_title_changes() {
        let (_tmp, project, board) = project_with_board();
        let mut task = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Initial");
        let first = project.save_any_task(&task).unwrap();
        assert!(project.root.join(&first).exists());

        task.title = "Renamed".into();
        let second = project.save_any_task(&task).unwrap();
        assert_ne!(first, second, "rename must produce a different path");
        assert!(project.root.join(&second).exists(), "new file must exist");
        assert!(
            !project.root.join(&first).exists(),
            "old file must be removed"
        );
    }

    #[test]
    fn save_any_task_handles_collisions_with_numeric_suffix() {
        let (_tmp, project, board) = project_with_board();
        let t1 = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Same Title");
        let t2 = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Same Title");
        let p1 = project.save_any_task(&t1).unwrap();
        let p2 = project.save_any_task(&t2).unwrap();
        assert!(p1.to_string_lossy().ends_with("same-title.md"));
        assert!(p2.to_string_lossy().ends_with("same-title-2.md"));
        assert!(project.root.join(&p1).exists());
        assert!(project.root.join(&p2).exists());
    }

    #[test]
    fn migrate_tasks_to_files_writes_legacy_sqlite_only_tasks() {
        // Simulate a legacy project: task in sqlite without task_file_path.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let project = Project::open(&root).unwrap();
        let board = flynt_core::models::Board::default_sprint("Backlog Board");
        project.store.save_board(&board).unwrap();
        let task = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Legacy task");
        // Bypass save_any_task — write directly via store, mimicking pre-migration state.
        project.store.save_task(&task).unwrap();
        assert!(project.store.task_file_path(&task.id).unwrap().is_none());

        let n = project.migrate_tasks_to_files().unwrap();
        assert_eq!(n, 1);
        let path = project.store.task_file_path(&task.id).unwrap().unwrap();
        assert!(project.root.join(&path).exists());
        assert!(path.starts_with("Tasks/backlog-board/"));
    }

    #[test]
    fn save_document_content_preserves_task_frontmatter() {
        // Regression: notes-view edits used to wipe non-Frontmatter
        // fields (task kind + [data] block) because save_document_content
        // wrote body-only and the reindex injected default doc fields.
        let (_tmp, project, board) = project_with_board();
        let mut t = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Auth Rewrite");
        t.description = "Original body".into();
        let rel = project.save_any_task(&t).unwrap();
        let before = std::fs::read_to_string(project.root.join(&rel)).unwrap();
        assert!(
            before.contains("kind = \"task\""),
            "task fm before edit:\n{before}"
        );
        assert!(before.contains("[data]"), "task fm before edit:\n{before}");

        // Simulate a notes-view body edit.
        project.save_document_content(&rel, "Edited body").unwrap();

        let after = std::fs::read_to_string(project.root.join(&rel)).unwrap();
        assert!(after.contains("kind = \"task\""), "kind survived:\n{after}");
        assert!(after.contains("[data]"), "[data] survived:\n{after}");
        assert!(after.contains("Edited body"), "body updated:\n{after}");
        assert!(!after.contains("Original body"), "old body gone:\n{after}");
    }

    #[test]
    fn save_document_content_preserves_recognized_frontmatter_fields() {
        // Limit of the preservation guarantee: fields the Frontmatter
        // parser recognizes (kind, data, aliases, tags, …) survive a
        // body edit. Truly unknown TOP-level keys flow into metadata
        // via serde flatten and also survive. Unknown nested [tables]
        // do NOT survive because the parser rejects them and
        // index_file rewrites the frontmatter — that's a known
        // limitation, not the case we need to fix for tasks.
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let project = Project::open(&root).unwrap();
        let rel = std::path::PathBuf::from("custom.md");
        let original = "+++\n\
            id = \"11111111-2222-3333-4444-555555555555\"\n\
            title = \"Custom\"\n\
            tags = [\"alpha\", \"beta\"]\n\
            aliases = [\"shorthand\"]\n\
            +++\n\n\
            First body";
        std::fs::write(project.root.join(&rel), original).unwrap();

        project.save_document_content(&rel, "Second body").unwrap();
        let after = std::fs::read_to_string(project.root.join(&rel)).unwrap();
        assert!(after.contains("alpha"), "tags survived:\n{after}");
        assert!(after.contains("shorthand"), "aliases survived:\n{after}");
        assert!(after.contains("Second body"), "body updated:\n{after}");
        assert!(!after.contains("First body"), "old body gone:\n{after}");
    }

    #[test]
    fn extract_raw_frontmatter_block_basic() {
        let raw = "+++\nid = \"x\"\ntitle = \"y\"\n+++\n\nbody";
        let got = super::extract_raw_frontmatter_block(raw).expect("should extract");
        assert!(got.starts_with("+++\n"));
        assert!(got.ends_with("+++"), "block: {got:?}");
        assert!(got.contains("id = \"x\""));
        assert!(!got.contains("body"));
    }

    #[test]
    fn save_any_task_is_idempotent_when_called_twice_with_same_data() {
        let (_tmp, project, board) = project_with_board();
        let task = flynt_core::models::Task::new(board.id.clone(), "Backlog", "Steady");
        let p1 = project.save_any_task(&task).unwrap();
        let p2 = project.save_any_task(&task).unwrap();
        assert_eq!(p1, p2);
        // Only one file under Tasks/sprint-1/
        let entries: Vec<_> = std::fs::read_dir(project.root.join("Tasks/sprint-1"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1, "no duplicate files on resave");
    }
}
