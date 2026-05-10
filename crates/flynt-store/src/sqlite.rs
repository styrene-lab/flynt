use anyhow::Result;
use flynt_core::{
    models::*,
    store::{DocumentMetadataFilter, TaskFilter, ProjectStore},
};
use rusqlite::{Connection, params};
use std::{path::Path, sync::Mutex};

/// SQLite-backed `ProjectStore`.
/// The database file lives at `<project_root>/.flynt/state.db`.
/// Documents are stored as markdown files; the DB is an index + task store.
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    pub fn open(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(SCHEMA)?;
        // Apply idempotent migrations (ALTER TABLE ADD COLUMN is a no-op if column exists)
        for migration in MIGRATIONS {
            let _ = conn.execute_batch(migration); // ignore "duplicate column" errors
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Read the on-disk file path for a task (project-relative). None when
    /// the task hasn't been written as a file yet — the migration sweep
    /// at Project::open populates these for legacy sqlite-only tasks.
    pub fn task_file_path(&self, task_id: &TaskId) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let path: Option<String> = conn.query_row(
            "SELECT task_file_path FROM tasks WHERE id = ?1",
            params![task_id.0.to_string()],
            |row| row.get(0),
        ).ok().flatten();
        Ok(path)
    }

    /// Update the on-disk file path for a task. Called by Project after
    /// it writes (or renames) the markdown file.
    pub fn set_task_file_path(&self, task_id: &TaskId, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tasks SET task_file_path = ?1 WHERE id = ?2",
            params![path, task_id.0.to_string()],
        )?;
        Ok(())
    }

    /// All tasks lacking an on-disk file (task_file_path IS NULL).
    /// Used by the migration sweep to find legacy sqlite-only rows.
    pub fn tasks_without_file(&self) -> Result<Vec<TaskId>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id FROM tasks WHERE task_file_path IS NULL",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            let s = r?;
            if let Ok(u) = uuid::Uuid::parse_str(&s) {
                out.push(TaskId(u));
            }
        }
        Ok(out)
    }
}

const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL;

CREATE TABLE IF NOT EXISTS documents (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL UNIQUE,
    title       TEXT NOT NULL,
    content     TEXT NOT NULL,
    frontmatter TEXT NOT NULL DEFAULT '{}',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS document_links (
    source_id   TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    target      TEXT NOT NULL,
    PRIMARY KEY (source_id, target)
);

CREATE TABLE IF NOT EXISTS document_metadata (
    document_id   TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    key           TEXT NOT NULL,
    value_type    TEXT NOT NULL,
    string_value  TEXT,
    protection    TEXT NOT NULL DEFAULT 'plaintext_indexed',
    PRIMARY KEY (document_id, key)
);

CREATE INDEX IF NOT EXISTS idx_document_metadata_key_value
    ON document_metadata (key, string_value);

CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
    title, content, content=documents, content_rowid=rowid
);

CREATE TRIGGER IF NOT EXISTS docs_fts_insert AFTER INSERT ON documents BEGIN
    INSERT INTO documents_fts(rowid, title, content) VALUES (new.rowid, new.title, new.content);
END;
CREATE TRIGGER IF NOT EXISTS docs_fts_delete AFTER DELETE ON documents BEGIN
    INSERT INTO documents_fts(documents_fts, rowid, title, content)
    VALUES('delete', old.rowid, old.title, old.content);
END;
CREATE TRIGGER IF NOT EXISTS docs_fts_update AFTER UPDATE ON documents BEGIN
    INSERT INTO documents_fts(documents_fts, rowid, title, content)
    VALUES('delete', old.rowid, old.title, old.content);
    INSERT INTO documents_fts(rowid, title, content) VALUES (new.rowid, new.title, new.content);
END;

CREATE TABLE IF NOT EXISTS boards (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    columns     TEXT NOT NULL DEFAULT '[]',
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tasks (
    id           TEXT PRIMARY KEY,
    board_id     TEXT NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    column_name  TEXT NOT NULL,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    priority     TEXT NOT NULL DEFAULT 'medium',
    status       TEXT NOT NULL DEFAULT 'todo',
    tags         TEXT NOT NULL DEFAULT '[]',
    document_refs TEXT NOT NULL DEFAULT '[]',
    due_date     TEXT,
    position     INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS conflicts (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL,
    ours        TEXT NOT NULL DEFAULT '',
    theirs      TEXT NOT NULL DEFAULT '',
    detected_at TEXT NOT NULL
);

"#;

/// Idempotent migrations applied after the base schema.
const MIGRATIONS: &[&str] = &[
    // v1: project git backing — dirty tracking columns
    "ALTER TABLE tasks ADD COLUMN project_id TEXT;",
    "ALTER TABLE tasks ADD COLUMN last_committed_at TEXT;",
    "ALTER TABLE documents ADD COLUMN last_committed_at TEXT;",
    // v2: board-project association
    "ALTER TABLE boards ADD COLUMN project_id TEXT;",
    // v3: task decay
    "ALTER TABLE tasks ADD COLUMN decay TEXT NOT NULL DEFAULT '\"natural\"';",
    "ALTER TABLE tasks ADD COLUMN last_touched_at TEXT;",
    // v4: sentry integration prerequisites — external_refs and design_node_id
    // are model fields with #[serde(default)] but were never persisted to
    // SQLite, so they were silently lost across process restarts. The TODOs
    // at row_to_task hardcoded empty/None. See flynt/design/sentry-integration.md.
    "ALTER TABLE tasks ADD COLUMN external_refs TEXT NOT NULL DEFAULT '[]';",
    "ALTER TABLE tasks ADD COLUMN design_node_id TEXT;",
    // v5: sentry integration priority 3 — execution metadata + openspec_change.
    // Mirrors omegon::sentry::types::TaskSpec (model, skill, max_turns, etc.)
    // so the planned FlyntTaskBoard adapter is a thin pass-through. Stored as
    // JSON blob in a single column; openspec_change is a bare string.
    "ALTER TABLE tasks ADD COLUMN execution TEXT;",
    "ALTER TABLE tasks ADD COLUMN openspec_change TEXT;",
    // v6: scribe absorption — engagement scope on tasks. Stored as TEXT
    // (UUID string). Soft-coupled to v7's engagements table: a task may
    // carry an engagement_id whose row doesn't exist (yet, or any more)
    // — callers treat that as "no engagement," not an error.
    "ALTER TABLE tasks ADD COLUMN engagement_id TEXT;",
    // v7: engagements table — multi-repo work scope records. `repos` and
    // `forge` round-trip as JSON blobs (RepoBinding[] and ForgeEndpoint
    // respectively); `partnership_id` is loose so we can defer
    // partnership persistence until something actually needs it.
    r#"CREATE TABLE IF NOT EXISTS engagements (
        id              TEXT PRIMARY KEY,
        partnership_id  TEXT,
        name            TEXT NOT NULL,
        description     TEXT,
        repos           TEXT NOT NULL DEFAULT '[]',
        forge           TEXT NOT NULL,
        status          TEXT NOT NULL DEFAULT '"active"',
        created_at      TEXT NOT NULL,
        updated_at      TEXT NOT NULL
    );"#,
    "CREATE INDEX IF NOT EXISTS idx_engagements_partnership ON engagements (partnership_id);",
    "CREATE INDEX IF NOT EXISTS idx_tasks_engagement ON tasks (engagement_id);",
    // v8: tasks-as-files. Every task gets a `.md` file at a project-relative
    // path; this column stores the latest path so we can rename the file
    // when the title changes (slug-based filenames). NULL means the task
    // has not yet been migrated to disk — the one-shot migration in
    // Project::open populates these.
    "ALTER TABLE tasks ADD COLUMN task_file_path TEXT;",
    // v9: schema cleanup after Vault → Project rename + inner-Project
    // dissolution (commits 53a3aa2..0d3a3c3, 2026-05-10). The dropped
    // surfaces had no remaining read paths — every column listed below
    // was either never-read or skipped-on-read. The runner swallows
    // errors so a fresh DB (where the columns were never added) skips
    // these no-ops cleanly.
    "ALTER TABLE tasks DROP COLUMN project_id;",
    "ALTER TABLE tasks DROP COLUMN last_committed_at;",
    "ALTER TABLE documents DROP COLUMN last_committed_at;",
    "ALTER TABLE boards DROP COLUMN project_id;",
    "DROP TABLE IF EXISTS project_deletions;",
    // v10: engagement.auto_create_issues — drives the push pipeline's
    // first-time mirror-up. Default 0 (local-first) so existing
    // engagements opt OUT of auto-push until the operator flips the
    // flag via engagement_create (or future engagement_update).
    "ALTER TABLE engagements ADD COLUMN auto_create_issues INTEGER NOT NULL DEFAULT 0;",
];

// ── ProjectStore implementation ─────────────────────────────────────────────────

impl ProjectStore for SqliteStore {
    fn get_document(&self, id: &DocumentId) -> Result<Option<Document>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, path, title, content, frontmatter, created_at, updated_at FROM documents WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id.0.to_string()])?;
        let Some(row) = rows.next()? else { return Ok(None) };
        Ok(Some(row_to_document(&conn, row)?))
    }

    fn get_document_by_path(&self, path: &Path) -> Result<Option<Document>> {
        let conn = self.conn.lock().unwrap();
        let path_str = path.to_string_lossy();
        let mut stmt = conn.prepare(
            "SELECT id, path, title, content, frontmatter, created_at, updated_at FROM documents WHERE path = ?1",
        )?;
        let mut rows = stmt.query(params![path_str.as_ref()])?;
        let Some(row) = rows.next()? else { return Ok(None) };
        Ok(Some(row_to_document(&conn, row)?))
    }

    fn list_documents(&self) -> Result<Vec<DocumentMeta>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, path, title, frontmatter, updated_at FROM documents ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], |row| {
            let fm_json: String = row.get(3)?;
            Ok(DocumentMeta {
                id: DocumentId(row.get::<_, String>(0)?.parse().map_err(|e| rusqlite::Error::InvalidParameterName(format!("{e}")))?),
                path: row.get::<_, String>(1)?.into(),
                title: row.get(2)?,
                tags: serde_json::from_str::<Frontmatter>(&fm_json)
                    .unwrap_or_default()
                    .tags,
                metadata: document_metadata_fields_from_frontmatter_json(&fm_json),
                entity_kind: entity_kind_from_frontmatter_json(&fm_json),
                updated_at: row.get::<_, String>(4)?.parse().unwrap_or_else(|_| chrono::Utc::now()),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn find_document_by_slug(&self, slug: &str) -> Result<Option<DocumentMeta>> {
        // Decode %20 etc. and normalise to lowercase for matching
        let decoded = slug.replace("%20", " ");
        let needle  = decoded.to_lowercase();
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"SELECT id, path, title, frontmatter, updated_at FROM documents
               WHERE LOWER(title) = ?1
                  OR LOWER(REPLACE(path, '.md', '')) LIKE '%' || ?1
               LIMIT 1"#,
        )?;
        let mut rows = stmt.query_map(params![needle], |row| {
            let fm_json: String = row.get(3)?;
            Ok(DocumentMeta {
                id: DocumentId(row.get::<_, String>(0)?.parse().map_err(|e| rusqlite::Error::InvalidParameterName(format!("{e}")))? ),
                path: row.get::<_, String>(1)?.into(),
                title: row.get(2)?,
                tags: serde_json::from_str::<Frontmatter>(&fm_json).unwrap_or_default().tags,
                metadata: document_metadata_fields_from_frontmatter_json(&fm_json),
                entity_kind: entity_kind_from_frontmatter_json(&fm_json),
                updated_at: row.get::<_, String>(4)?.parse().unwrap_or_else(|_| chrono::Utc::now()),
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    fn list_documents_by_metadata(&self, filter: &DocumentMetadataFilter) -> Result<Vec<DocumentMeta>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"SELECT d.id, d.path, d.title, d.frontmatter, d.updated_at
               FROM document_metadata m
               JOIN documents d ON d.id = m.document_id
               WHERE m.key = ?1 AND m.string_value = ?2
               ORDER BY d.updated_at DESC"#,
        )?;
        let rows = stmt.query_map(params![filter.field, filter.value], |row| {
            let fm_json: String = row.get(3)?;
            Ok(DocumentMeta {
                id: DocumentId(row.get::<_, String>(0)?.parse().map_err(|e| rusqlite::Error::InvalidParameterName(format!("{e}")))?),
                path: row.get::<_, String>(1)?.into(),
                title: row.get(2)?,
                tags: serde_json::from_str::<Frontmatter>(&fm_json).unwrap_or_default().tags,
                metadata: document_metadata_fields_from_frontmatter_json(&fm_json),
                entity_kind: entity_kind_from_frontmatter_json(&fm_json),
                updated_at: row.get::<_, String>(4)?.parse().unwrap_or_else(|_| chrono::Utc::now()),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn save_document(&self, doc: &Document) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let fm   = serde_json::to_string(&doc.frontmatter)?;
        let path = doc.path.to_string_lossy().to_string();
        let id = doc.id.0.to_string();
        conn.execute(
            r#"DELETE FROM document_links
               WHERE source_id IN (
                   SELECT id FROM documents
                   WHERE (path = ?1 AND id != ?2) OR (id = ?2 AND path != ?1)
               )"#,
            params![path, id],
        )?;
        conn.execute(
            r#"DELETE FROM document_metadata
               WHERE document_id IN (
                   SELECT id FROM documents
                   WHERE (path = ?1 AND id != ?2) OR (id = ?2 AND path != ?1)
               )"#,
            params![path, id],
        )?;
        conn.execute(
            r#"DELETE FROM documents
               WHERE (path = ?1 AND id != ?2) OR (id = ?2 AND path != ?1)"#,
            params![path, id],
        )?;
        conn.execute(
            r#"INSERT INTO documents (id, path, title, content, frontmatter, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
               ON CONFLICT(id) DO UPDATE SET
                 path=excluded.path, title=excluded.title, content=excluded.content,
                 frontmatter=excluded.frontmatter, updated_at=excluded.updated_at"#,
            params![
                doc.id.0.to_string(),
                doc.path.to_string_lossy().as_ref(),
                doc.title,
                doc.content,
                fm,
                doc.created_at.to_rfc3339(),
                doc.updated_at.to_rfc3339(),
            ],
        )?;
        // Refresh outgoing links
        conn.execute("DELETE FROM document_links WHERE source_id = ?1", params![doc.id.0.to_string()])?;
        for link in &doc.outgoing_links {
            conn.execute(
                "INSERT OR IGNORE INTO document_links (source_id, target) VALUES (?1, ?2)",
                params![doc.id.0.to_string(), link.target],
            )?;
        }
        conn.execute("DELETE FROM document_metadata WHERE document_id = ?1", params![doc.id.0.to_string()])?;
        for (key, field) in frontmatter_metadata_fields(&doc.frontmatter) {
            if let Some((value_type, string_value)) = string_indexable_metadata_value(&field.value) {
                conn.execute(
                    "INSERT OR REPLACE INTO document_metadata (document_id, key, value_type, string_value, protection) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        doc.id.0.to_string(),
                        key,
                        value_type,
                        string_value,
                        metadata_protection_label(&field.protection),
                    ],
                )?;
            }
        }
        Ok(())
    }

    fn delete_document(&self, id: &DocumentId) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "DELETE FROM documents WHERE id = ?1",
            params![id.0.to_string()],
        )?;
        Ok(())
    }

    fn search_documents(&self, query: &str) -> Result<Vec<SearchResult>> {
        // Build a prefix-match FTS5 query: "Sty Lab" → "Sty* Lab*"
        let fts_query: String = query
            .split_whitespace()
            .filter(|t| !t.is_empty())
            .map(|t| {
                // Escape any FTS5 special chars that aren't alphanumeric
                let safe: String = t.chars()
                    .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { ' ' })
                    .collect();
                format!("{}*", safe.trim())
            })
            .filter(|t| t.len() > 1)
            .collect::<Vec<_>>()
            .join(" ");
        if fts_query.is_empty() { return Ok(vec![]); }
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"SELECT d.id, d.path, d.title, snippet(documents_fts, 1, '<mark>', '</mark>', '…', 32)
               FROM documents_fts f
               JOIN documents d ON d.rowid = f.rowid
               WHERE documents_fts MATCH ?1
               ORDER BY bm25(documents_fts) LIMIT 50"#,
        )?;
        let results = stmt.query_map(params![fts_query], |row| {
            Ok(SearchResult {
                document_id: DocumentId(row.get::<_, String>(0)?.parse().unwrap_or_default()),
                path: row.get::<_, String>(1)?.into(),
                title: row.get(2)?,
                excerpt: row.get(3)?,
                score: 1.0,
            })
        })?;
        Ok(results.collect::<rusqlite::Result<_>>()?)
    }

    fn list_entities_by_kind(&self, kind: &flynt_core::datum::EntityKind) -> Result<Vec<DocumentMeta>> {
        let conn = self.conn.lock().unwrap();
        let kind_str = kind.as_str();
        // Query frontmatter JSON for the kind field
        let mut stmt = conn.prepare(
            r#"SELECT id, path, title, frontmatter, updated_at FROM documents
               WHERE json_extract(frontmatter, '$.kind') = ?1
               ORDER BY updated_at DESC"#,
        )?;
        let rows = stmt.query_map(params![kind_str], |row| {
            let fm_json: String = row.get(3)?;
            Ok(DocumentMeta {
                id: DocumentId(row.get::<_, String>(0)?.parse().map_err(|e| rusqlite::Error::InvalidParameterName(format!("{e}")))?),
                path: row.get::<_, String>(1)?.into(),
                title: row.get(2)?,
                tags: serde_json::from_str::<Frontmatter>(&fm_json).unwrap_or_default().tags,
                metadata: document_metadata_fields_from_frontmatter_json(&fm_json),
                entity_kind: entity_kind_from_frontmatter_json(&fm_json),
                updated_at: row.get::<_, String>(4)?.parse().unwrap_or_else(|_| chrono::Utc::now()),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn get_backlinks(&self, id: &DocumentId) -> Result<Vec<DocumentMeta>> {
        let conn = self.conn.lock().unwrap();
        // title and path are stored in documents; links reference by target slug
        let mut stmt = conn.prepare(
            r#"SELECT d.id, d.path, d.title, d.frontmatter, d.updated_at
               FROM document_links l
               JOIN documents d ON d.id = l.source_id
               WHERE l.target IN (
                   SELECT REPLACE(REPLACE(path, '.md', ''), '/', '-')
                   FROM documents WHERE id = ?1
               )
               ORDER BY d.updated_at DESC"#,
        )?;
        let rows = stmt.query_map(params![id.0.to_string()], |row| {
            let fm_json: String = row.get(3)?;
            let updated_at: String = row.get(4)?;
            Ok(DocumentMeta {
                id: DocumentId(row.get::<_, String>(0)?.parse().unwrap_or_default()),
                path: row.get::<_, String>(1)?.into(),
                title: row.get(2)?,
                tags: serde_json::from_str::<Frontmatter>(&fm_json).unwrap_or_default().tags,
                metadata: document_metadata_fields_from_frontmatter_json(&fm_json),
                entity_kind: entity_kind_from_frontmatter_json(&fm_json),
                updated_at: updated_at.parse().unwrap_or_else(|_| chrono::Utc::now()),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn get_task(&self, id: &TaskId) -> Result<Option<Task>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, board_id, column_name, title, description, priority, status, tags, document_refs, due_date, position, created_at, updated_at, decay, last_touched_at, external_refs, design_node_id, execution, openspec_change, engagement_id FROM tasks WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id.0.to_string()])?;
        let Some(row) = rows.next()? else { return Ok(None) };
        Ok(Some(row_to_task(row)?))
    }

    fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>> {
        let conn = self.conn.lock().unwrap();
        // Build a parameterized query. Predicates AND together. Tags are
        // matched via SQLite's json_each — task must contain ALL filter tags
        // (intersection). Status compares against the JSON-encoded form
        // ("todo" / "in_progress" / etc., quotes included) since that's
        // exactly how save_task writes it.
        let mut conds = vec!["1=1".to_string()];
        let mut values: Vec<String> = Vec::new();
        if let Some(ref bid) = filter.board_id {
            conds.push(format!("board_id = ?{}", values.len() + 1));
            values.push(bid.0.to_string());
        }
        if let Some(ref col) = filter.column {
            conds.push(format!("column_name = ?{}", values.len() + 1));
            values.push(col.clone());
        }
        if let Some(status) = filter.status {
            // serde_json::to_string yields a quoted form like "\"todo\""
            // — the same form save_task writes into the column. Match
            // by string equality, no JSON parsing needed.
            conds.push(format!("status = ?{}", values.len() + 1));
            values.push(serde_json::to_string(&status)?);
        }
        if let Some(ref eng) = filter.engagement_id {
            conds.push(format!("engagement_id = ?{}", values.len() + 1));
            values.push(eng.0.to_string());
        }
        for tag in &filter.tags {
            conds.push(format!(
                "EXISTS (SELECT 1 FROM json_each(tags) WHERE value = ?{})",
                values.len() + 1
            ));
            values.push(tag.clone());
        }
        let sql = format!(
            "SELECT id, board_id, column_name, title, description, priority, status, tags, document_refs, due_date, position, created_at, updated_at, decay, last_touched_at, external_refs, design_node_id, execution, openspec_change, engagement_id FROM tasks WHERE {} ORDER BY position ASC",
            conds.join(" AND ")
        );
        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            values.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params_refs.as_slice(), row_to_task)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn save_task(&self, task: &Task) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let execution_json = task
            .execution
            .as_ref()
            .filter(|e| !e.is_empty())
            .map(|e| serde_json::to_string(e))
            .transpose()?;
        conn.execute(
            r#"INSERT INTO tasks (id, board_id, column_name, title, description, priority, status, tags, document_refs, due_date, position, created_at, updated_at, decay, last_touched_at, external_refs, design_node_id, execution, openspec_change, engagement_id)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)
               ON CONFLICT(id) DO UPDATE SET
                 board_id=excluded.board_id, column_name=excluded.column_name,
                 title=excluded.title, description=excluded.description,
                 priority=excluded.priority, status=excluded.status,
                 tags=excluded.tags, document_refs=excluded.document_refs,
                 due_date=excluded.due_date, position=excluded.position,
                 updated_at=excluded.updated_at,
                 decay=excluded.decay, last_touched_at=excluded.last_touched_at,
                 external_refs=excluded.external_refs, design_node_id=excluded.design_node_id,
                 execution=excluded.execution, openspec_change=excluded.openspec_change,
                 engagement_id=excluded.engagement_id"#,
            params![
                task.id.0.to_string(),
                task.board_id.0.to_string(),
                task.column,
                task.title,
                task.description,
                serde_json::to_string(&task.priority)?,
                serde_json::to_string(&task.status)?,
                serde_json::to_string(&task.tags)?,
                serde_json::to_string(&task.document_refs)?,
                task.due_date.map(|d| d.to_string()),
                task.position,
                task.created_at.to_rfc3339(),
                task.updated_at.to_rfc3339(),
                serde_json::to_string(&task.decay)?,
                task.last_touched_at.map(|t| t.to_rfc3339()),
                serde_json::to_string(&task.external_refs)?,
                task.design_node_id.map(|u| u.to_string()),
                execution_json,
                task.openspec_change,
                task.engagement_id.as_ref().map(|e| e.0.to_string()),
            ],
        )?;
        Ok(())
    }

    fn update_task(&self, id: &TaskId, patch: &flynt_models::TaskPatch) -> Result<bool> {
        if patch.is_empty() {
            // No-op patch — caller didn't provide any changes. Still return
            // true if the task exists (for caller convenience), false if not.
            return Ok(self.get_task(id)?.is_some());
        }

        // Read-modify-write strategy: load the existing task, merge the
        // patch onto it, write back. Keeps the SQL simple (one UPDATE that
        // touches every column) at the cost of one extra SELECT. Tasks are
        // small; this is fine.
        let mut task = match self.get_task(id)? {
            Some(t) => t,
            None => return Ok(false),
        };

        if let Some(v) = &patch.column { task.column = v.clone(); }
        if let Some(v) = &patch.title { task.title = v.clone(); }
        if let Some(v) = &patch.description { task.description = v.clone(); }
        if let Some(v) = patch.priority.clone() { task.priority = v; }
        if let Some(v) = patch.status.clone() { task.status = v; }
        if let Some(v) = &patch.tags { task.tags = v.clone(); }
        if let Some(v) = patch.due_date { task.due_date = v; }
        if let Some(v) = &patch.external_refs { task.external_refs = v.clone(); }
        if let Some(v) = &patch.document_refs { task.document_refs = v.clone(); }
        if let Some(v) = patch.position { task.position = v; }
        if let Some(v) = patch.decay.clone() { task.decay = v; }
        if let Some(v) = patch.design_node_id { task.design_node_id = v; }
        if let Some(v) = &patch.openspec_change { task.openspec_change = v.clone(); }
        if let Some(v) = &patch.engagement_id { task.engagement_id = v.clone(); }
        if let Some(v) = &patch.execution { task.execution = v.clone(); }
        task.updated_at = chrono::Utc::now();

        self.save_task(&task)?;
        Ok(true)
    }

    fn delete_task(&self, id: &TaskId) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "DELETE FROM tasks WHERE id = ?1",
            params![id.0.to_string()],
        )?;
        Ok(())
    }

    fn get_board(&self, id: &BoardId) -> Result<Option<Board>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, columns, created_at FROM boards WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id.0.to_string()])?;
        let Some(row) = rows.next()? else { return Ok(None) };
        Ok(Some(row_to_board(row)?))
    }

    fn list_boards(&self) -> Result<Vec<Board>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT id, name, columns, created_at FROM boards ORDER BY name ASC")?;
        let rows = stmt.query_map([], row_to_board)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn save_board(&self, board: &Board) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"INSERT INTO boards (id, name, columns, created_at)
               VALUES (?1, ?2, ?3, ?4)
               ON CONFLICT(id) DO UPDATE SET name=excluded.name, columns=excluded.columns"#,
            params![
                board.id.0.to_string(),
                board.name,
                serde_json::to_string(&board.columns)?,
                board.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    fn delete_board(&self, id: &BoardId) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Cascade: remove all tasks belonging to this board first.
        conn.execute(
            "DELETE FROM tasks WHERE board_id = ?1",
            params![id.0.to_string()],
        )?;
        conn.execute(
            "DELETE FROM boards WHERE id = ?1",
            params![id.0.to_string()],
        )?;
        Ok(())
    }

    // ── Engagements ──────────────────────────────────────────────────────────

    fn get_engagement(
        &self,
        id: &flynt_models::engagement::EngagementId,
    ) -> Result<Option<flynt_models::engagement::Engagement>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, partnership_id, name, description, repos, forge, status, auto_create_issues, created_at, updated_at
             FROM engagements WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id.0.to_string()])?;
        let Some(row) = rows.next()? else { return Ok(None) };
        Ok(Some(row_to_engagement(row)?))
    }

    fn list_engagements(&self) -> Result<Vec<flynt_models::engagement::Engagement>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, partnership_id, name, description, repos, forge, status, auto_create_issues, created_at, updated_at
             FROM engagements ORDER BY name ASC",
        )?;
        let rows = stmt.query_map([], row_to_engagement)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn save_engagement(&self, engagement: &flynt_models::engagement::Engagement) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"INSERT INTO engagements (id, partnership_id, name, description, repos, forge, status, auto_create_issues, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
               ON CONFLICT(id) DO UPDATE SET
                 partnership_id     = excluded.partnership_id,
                 name               = excluded.name,
                 description        = excluded.description,
                 repos              = excluded.repos,
                 forge              = excluded.forge,
                 status             = excluded.status,
                 auto_create_issues = excluded.auto_create_issues,
                 updated_at         = excluded.updated_at"#,
            params![
                engagement.id.0.to_string(),
                engagement.partnership_id.as_ref().map(|p| p.0.to_string()),
                engagement.name,
                engagement.description,
                serde_json::to_string(&engagement.repos)?,
                serde_json::to_string(&engagement.forge)?,
                serde_json::to_string(&engagement.status)?,
                engagement.auto_create_issues as i64,
                engagement.created_at.to_rfc3339(),
                engagement.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    fn delete_engagement(
        &self,
        id: &flynt_models::engagement::EngagementId,
    ) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM engagements WHERE id = ?1",
            params![id.0.to_string()],
        )?;
        // Tasks keep their engagement_id pointing at a now-missing row;
        // callers treat dangling refs as "no engagement" per the trait
        // doc. We don't NULL them out here because that's a policy
        // decision (cascade vs orphan) the caller may want to override.
        Ok(n > 0)
    }

}

// ── Row deserializers ─────────────────────────────────────────────────────────

fn row_to_document(conn: &Connection, row: &rusqlite::Row<'_>) -> rusqlite::Result<Document> {
    let fm_json: String = row.get(4)?;
    let created_at: String = row.get(5)?;
    let updated_at: String = row.get(6)?;
    let path_str: String = row.get(1)?;
    let source_id: String = row.get(0)?;
    let frontmatter: Frontmatter = serde_json::from_str(&fm_json).unwrap_or_default();
    let mut link_stmt = conn.prepare("SELECT target FROM document_links WHERE source_id = ?1 ORDER BY target ASC")?;
    let outgoing_links = link_stmt
        .query_map(params![source_id], |row| {
            let target: String = row.get(0)?;
            Ok(WikiLink {
                target,
                display: None,
                anchor: None,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let entity = entity_from_frontmatter(&frontmatter);
    Ok(Document {
        id: DocumentId(source_id.parse().unwrap_or_default()),
        path: path_str.into(),
        title: row.get(2)?,
        content: row.get(3)?,
        outgoing_links,
        frontmatter,
        created_at: created_at.parse().unwrap_or_else(|_| chrono::Utc::now()),
        updated_at: updated_at.parse().unwrap_or_else(|_| chrono::Utc::now()),
        entity,
    })
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let priority: String = row.get(5)?;
    let status: String = row.get(6)?;
    let tags_json: String = row.get(7)?;
    let refs_json: String = row.get(8)?;
    let due: Option<String> = row.get(9)?;
    let created_at: String = row.get(11)?;
    let updated_at: String = row.get(12)?;
    let decay: Option<String> = row.get(13)?;
    let last_touched: Option<String> = row.get(14)?;
    let parse_uuid = |s: String| -> rusqlite::Result<uuid::Uuid> {
        s.parse().map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
    };
    let parse_dt = |s: String| -> chrono::DateTime<chrono::Utc> {
        s.parse().unwrap_or_else(|_| chrono::Utc::now())
    };
    Ok(Task {
        id: TaskId(parse_uuid(row.get(0)?)?),
        board_id: BoardId(parse_uuid(row.get(1)?)?),
        column: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        priority: serde_json::from_str(&priority).unwrap_or_default(),
        status: serde_json::from_str(&status).unwrap_or_default(),
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        document_refs: serde_json::from_str(&refs_json).unwrap_or_default(),
        due_date: due.and_then(|s| s.parse().ok()),
        position: row.get(10)?,
        created_at: parse_dt(created_at),
        updated_at: parse_dt(updated_at),
        decay: decay.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
        last_touched_at: last_touched.and_then(|s| s.parse().ok()),
        external_refs: {
            let json: String = row.get(15)?;
            serde_json::from_str(&json).unwrap_or_default()
        },
        design_node_id: {
            let id_str: Option<String> = row.get(16)?;
            id_str.and_then(|s| s.parse().ok())
        },
        execution: {
            let json: Option<String> = row.get(17)?;
            json.and_then(|s| serde_json::from_str(&s).ok())
        },
        openspec_change: row.get(18)?,
        engagement_id: {
            let id_str: Option<String> = row.get(19)?;
            id_str
                .and_then(|s| uuid::Uuid::parse_str(&s).ok())
                .map(flynt_models::engagement::EngagementId)
        },
    })
}

fn row_to_engagement(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<flynt_models::engagement::Engagement> {
    use flynt_models::engagement::{Engagement, EngagementId, EngagementStatus, PartnershipId};
    let id: String = row.get(0)?;
    let partnership_id: Option<String> = row.get(1)?;
    let repos_json: String = row.get(4)?;
    let forge_json: String = row.get(5)?;
    let status_json: String = row.get(6)?;
    let auto_create: i64 = row.get(7)?;
    let created_at: String = row.get(8)?;
    let updated_at: String = row.get(9)?;
    Ok(Engagement {
        id: EngagementId(id.parse().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?),
        partnership_id: partnership_id
            .and_then(|s| s.parse().ok())
            .map(PartnershipId),
        name: row.get(2)?,
        description: row.get(3)?,
        repos: serde_json::from_str(&repos_json).unwrap_or_default(),
        forge: serde_json::from_str(&forge_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
        })?,
        status: serde_json::from_str(&status_json).unwrap_or(EngagementStatus::Active),
        auto_create_issues: auto_create != 0,
        created_at: created_at.parse().unwrap_or_else(|_| chrono::Utc::now()),
        updated_at: updated_at.parse().unwrap_or_else(|_| chrono::Utc::now()),
    })
}

fn row_to_board(row: &rusqlite::Row<'_>) -> rusqlite::Result<Board> {
    let cols_json: String = row.get(2)?;
    let created_at: String = row.get(3)?;
    let id: String = row.get(0)?;
    Ok(Board {
        id: BoardId(id.parse().map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?),
        name: row.get(1)?,
        columns: serde_json::from_str(&cols_json).unwrap_or_default(),
        created_at: created_at.parse().unwrap_or_else(|_| chrono::Utc::now()),
    })
}

fn entity_kind_from_frontmatter_json(frontmatter_json: &str) -> Option<flynt_core::datum::EntityKind> {
    serde_json::from_str::<Frontmatter>(frontmatter_json)
        .ok()?
        .kind
        .map(|k| flynt_core::datum::EntityKind::from_str(&k))
}

fn entity_from_frontmatter(fm: &Frontmatter) -> Option<flynt_core::datum::Entity> {
    let kind_str = fm.kind.as_deref()?;
    let kind = flynt_core::datum::EntityKind::from_str(kind_str);
    let id = fm.id.unwrap_or_else(uuid::Uuid::new_v4);
    let mut fields = std::collections::BTreeMap::new();
    if let Some(toml::Value::Table(data)) = &fm.data {
        for (k, v) in data {
            fields.insert(k.clone(), flynt_core::datum::Datum::from(v.clone()));
        }
    }
    Some(flynt_core::datum::Entity {
        id,
        kind,
        fields,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    })
}

fn document_metadata_fields_from_frontmatter_json(frontmatter_json: &str) -> MetadataFieldMap {
    serde_json::from_str::<Frontmatter>(frontmatter_json)
        .unwrap_or_default()
        .metadata
        .into_iter()
        .map(|(key, value)| {
            (
                key,
                MetadataField {
                    value,
                    protection: MetadataProtection::PlaintextIndexed,
                },
            )
        })
        .collect()
}

fn frontmatter_metadata_fields(frontmatter: &Frontmatter) -> MetadataFieldMap {
    frontmatter
        .metadata
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                MetadataField {
                    value: value.clone(),
                    protection: MetadataProtection::PlaintextIndexed,
                },
            )
        })
        .collect()
}

fn string_indexable_metadata_value(value: &MetadataValue) -> Option<(&'static str, String)> {
    match value {
        MetadataValue::Null => None,
        MetadataValue::Bool(value) => Some(("bool", value.to_string())),
        MetadataValue::Integer(value) => Some(("integer", value.to_string())),
        MetadataValue::Float(value) => Some(("float", value.to_string())),
        MetadataValue::String(value) => Some(("string", value.clone())),
        MetadataValue::StringList(values) => Some(("string_list", values.join("\u{001f}"))),
    }
}

fn metadata_protection_label(protection: &MetadataProtection) -> &'static str {
    match protection {
        MetadataProtection::PlaintextIndexed => "plaintext_indexed",
        MetadataProtection::EncryptedOpaque => "encrypted_opaque",
    }
}

#[cfg(test)]
mod tests {
    //! Adversarial / end-to-end tests for the sentry-integration changes.
    //! Each test below probes a specific concern from the assessment pass:
    //! cold-restart persistence, migration safety on existing data, filter
    //! correctness against a populated DB, and TaskPatch field-level
    //! independence.

    use super::*;
    use flynt_core::{
        models::{BoardId, ExecutionSpec, Priority, Task, TaskId, TaskPatch, TaskStatus},
        store::TaskFilter,
    };

    fn fresh_store() -> (tempfile::TempDir, SqliteStore) {
        let tmp = tempfile::TempDir::new().unwrap();
        let db = tmp.path().join("flynt-index.db");
        let store = SqliteStore::open(&db).unwrap();
        (tmp, store)
    }

    fn seed_board(store: &SqliteStore) -> BoardId {
        let mut board = flynt_core::models::Board::default_sprint("Sentry");
        board.columns = vec![
            flynt_core::models::Column { name: "Backlog".into(), wip_limit: None },
            flynt_core::models::Column { name: "Scheduled".into(), wip_limit: None },
            flynt_core::models::Column { name: "Running".into(), wip_limit: Some(1) },
        ];
        let bid = board.id.clone();
        store.save_board(&board).unwrap();
        bid
    }

    #[test]
    fn task_with_execution_and_openspec_persists_across_reopen() {
        // Cold restart durability: write a fully populated task, drop the
        // store, reopen the same DB, and verify every field survives.
        // Catches the entire class of "I forgot to add a column to SELECT
        // or save_task" bugs.
        let tmp = tempfile::TempDir::new().unwrap();
        let db = tmp.path().join("flynt-index.db");

        let task_id = TaskId(uuid::Uuid::new_v4());
        let board_id;

        // ── First open: write
        {
            let store = SqliteStore::open(&db).unwrap();
            board_id = seed_board(&store);

            let mut env = std::collections::BTreeMap::new();
            env.insert("API_TOKEN".into(), "redacted".into());
            let mut t = Task::new(board_id.clone(), "Scheduled", "Recurring scan");
            t.id = task_id.clone();
            t.tags = vec!["sentry".into(), "recurring".into()];
            t.external_refs = vec!["cron:0 */4 * * *".into(), "webhook:gh-pr".into()];
            t.design_node_id = Some(uuid::Uuid::new_v4());
            t.openspec_change = Some("auth-rewrite".into());
            t.execution = Some(ExecutionSpec {
                model: Some("anthropic:claude-sonnet-4-6".into()),
                max_turns: Some(20),
                env,
                ..Default::default()
            });
            store.save_task(&t).unwrap();
        }

        // ── Second open: read
        let store = SqliteStore::open(&db).unwrap();
        let loaded = store.get_task(&task_id).unwrap().expect("task exists");
        assert_eq!(loaded.tags, vec!["sentry", "recurring"]);
        assert_eq!(loaded.external_refs, vec!["cron:0 */4 * * *", "webhook:gh-pr"]);
        assert!(loaded.design_node_id.is_some());
        assert_eq!(loaded.openspec_change.as_deref(), Some("auth-rewrite"));
        let exec = loaded.execution.expect("execution preserved");
        assert_eq!(exec.model.as_deref(), Some("anthropic:claude-sonnet-4-6"));
        assert_eq!(exec.max_turns, Some(20));
        assert_eq!(exec.env.get("API_TOKEN").map(String::as_str), Some("redacted"));
    }

    #[test]
    fn migration_v9_drops_inner_project_surfaces_from_legacy_db() {
        // Simulates a database created by pre-rename code: a fresh SqliteStore
        // open runs all MIGRATIONS in order (v1..v9), so by the time we
        // inspect the schema, v9 should have already dropped the legacy
        // columns and table. This guards against a regression where someone
        // adds a v10 migration and forgets that v9's idempotency relies on
        // the runner swallowing errors.
        let tmp = tempfile::TempDir::new().unwrap();
        let db = tmp.path().join("flynt-index.db");

        // Bootstrap with the full migration chain and seed some data.
        let task_id = TaskId(uuid::Uuid::new_v4());
        {
            let store = SqliteStore::open(&db).unwrap();
            let board_id = seed_board(&store);
            let mut t = Task::new(board_id, "Backlog", "Survives migration");
            t.id = task_id.clone();
            store.save_task(&t).unwrap();
        }

        // Inspect raw schema using a fresh connection (bypass SqliteStore).
        let conn = rusqlite::Connection::open(&db).unwrap();

        // boards.project_id, tasks.project_id, tasks.last_committed_at,
        // documents.last_committed_at must all be gone.
        for (table, col) in [
            ("boards", "project_id"),
            ("tasks", "project_id"),
            ("tasks", "last_committed_at"),
            ("documents", "last_committed_at"),
        ] {
            let cols: Vec<String> = conn
                .prepare(&format!("PRAGMA table_info({table});"))
                .unwrap()
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            assert!(
                !cols.contains(&col.to_string()),
                "expected {table}.{col} to be dropped after v9, got cols: {cols:?}"
            );
        }

        // project_deletions table must be gone.
        let row: rusqlite::Result<i64> = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'project_deletions'",
            [],
            |r| r.get(0),
        );
        assert_eq!(row.unwrap(), 0, "project_deletions table should be dropped");

        // Data we wrote pre-inspection still readable — schema cleanup must
        // not have eaten task data.
        drop(conn);
        let store = SqliteStore::open(&db).unwrap();
        assert!(store.get_task(&task_id).unwrap().is_some(), "task survives v9");
    }

    #[test]
    fn migration_v9_drops_legacy_data_without_eating_task_or_board_rows() {
        // Stronger guard than the previous test: simulate a *pre-v9* database
        // (the legacy columns populated, project_deletions table populated)
        // and prove v9 strips those without harming the task/board rows that
        // share the schema. The Phase 2 dissolution had been writing NULL into
        // boards.project_id but old DBs may carry real values from before.
        let tmp = tempfile::TempDir::new().unwrap();
        let db = tmp.path().join("flynt-index.db");

        // Manually construct a pre-v9 schema by running everything except the
        // v9 DROP statements. We bypass SqliteStore::open and reach for the
        // raw connection so we can splice the migrations.
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(SCHEMA).unwrap();
            // Re-create the table v9 dropped, so we can prove v9 cleans it up.
            conn.execute_batch(
                r#"CREATE TABLE IF NOT EXISTS project_deletions (
                    entity_id   TEXT PRIMARY KEY,
                    entity_kind TEXT NOT NULL,
                    project_id  TEXT NOT NULL,
                    deleted_at  TEXT NOT NULL,
                    committed   INTEGER NOT NULL DEFAULT 0
                );"#,
            ).unwrap();
            // Apply v1..v8 only (everything before the v9 DROP block).
            for m in MIGRATIONS.iter().take_while(|m| !m.contains("DROP")) {
                let _ = conn.execute_batch(m);
            }
            // Seed a board + task with the (now-vestigial) project_id populated,
            // and a project_deletions row.
            let board_id = uuid::Uuid::new_v4().to_string();
            let task_id = uuid::Uuid::new_v4().to_string();
            let project_id = uuid::Uuid::new_v4().to_string();
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO boards (id, name, columns, project_id, created_at) VALUES (?1, 'Sprint', '[]', ?2, ?3)",
                rusqlite::params![board_id, project_id, now],
            ).unwrap();
            conn.execute(
                "INSERT INTO tasks (id, board_id, column_name, title, project_id, last_committed_at, created_at, updated_at)
                 VALUES (?1, ?2, 'Backlog', 'Pre-v9 task', ?3, ?4, ?4, ?4)",
                rusqlite::params![task_id, board_id, project_id, now],
            ).unwrap();
            conn.execute(
                "INSERT INTO project_deletions (entity_id, entity_kind, project_id, deleted_at)
                 VALUES (?1, 'task', ?2, ?3)",
                rusqlite::params![uuid::Uuid::new_v4().to_string(), project_id, now],
            ).unwrap();
        }

        // Now open via SqliteStore (which runs the full migration chain
        // including v9). The open must succeed cleanly.
        let store = SqliteStore::open(&db).unwrap();

        // The board + task survived.
        let boards = store.list_boards().unwrap();
        assert_eq!(boards.len(), 1, "board survives v9");
        assert_eq!(boards[0].name, "Sprint");

        let tasks = store.list_tasks(&TaskFilter {
            board_id: Some(boards[0].id.clone()),
            ..Default::default()
        }).unwrap();
        assert_eq!(tasks.len(), 1, "task survives v9");
        assert_eq!(tasks[0].title, "Pre-v9 task");

        // The legacy schema bits are gone.
        let conn = rusqlite::Connection::open(&db).unwrap();
        let table_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'project_deletions'",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(table_count, 0, "project_deletions dropped even when populated");
    }

    #[test]
    fn migration_v9_is_idempotent_on_reopen() {
        // Reopening a post-v9 DB must not error or re-create the dropped
        // surfaces. This tests the runner's let-_-= swallowing for the DROP
        // statements specifically (DROP COLUMN on a missing column raises).
        let tmp = tempfile::TempDir::new().unwrap();
        let db = tmp.path().join("flynt-index.db");
        for _ in 0..3 {
            let _ = SqliteStore::open(&db).unwrap();
        }
        let conn = rusqlite::Connection::open(&db).unwrap();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(boards);")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(!cols.contains(&"project_id".to_string()));
    }

    #[test]
    fn migration_v4_v5_is_idempotent_on_reopen() {
        // The migration runner re-applies all ALTER TABLE statements on every
        // open. Re-running on a DB that already has the columns must not
        // error or destroy data. This proves the runner's `let _ = ...; //
        // ignore duplicate column errors` strategy actually works.
        let tmp = tempfile::TempDir::new().unwrap();
        let db = tmp.path().join("flynt-index.db");

        let task_id = TaskId(uuid::Uuid::new_v4());
        {
            let store = SqliteStore::open(&db).unwrap();
            let board_id = seed_board(&store);
            let mut t = Task::new(board_id, "Backlog", "T");
            t.id = task_id.clone();
            t.openspec_change = Some("change-x".into());
            store.save_task(&t).unwrap();
        }
        // Reopen multiple times — exercises migration idempotency.
        for _ in 0..3 {
            let store = SqliteStore::open(&db).unwrap();
            let loaded = store.get_task(&task_id).unwrap().expect("task survives reopens");
            assert_eq!(loaded.openspec_change.as_deref(), Some("change-x"));
        }
    }

    #[test]
    fn update_task_with_only_status_does_not_clobber_other_fields() {
        // The TaskPatch contract: untouched fields must be preserved. If
        // update_task accidentally read the patch's None-as-clear instead of
        // None-as-leave-unchanged for any field, the side effect would land
        // here.
        let (_tmp, store) = fresh_store();
        let board_id = seed_board(&store);
        let task_id = TaskId(uuid::Uuid::new_v4());
        let mut t = Task::new(board_id.clone(), "Backlog", "Original");
        t.id = task_id.clone();
        t.tags = vec!["a".into(), "b".into()];
        t.priority = Priority::High;
        t.openspec_change = Some("change-x".into());
        t.execution = Some(ExecutionSpec { model: Some("m".into()), ..Default::default() });
        store.save_task(&t).unwrap();

        let patch = TaskPatch {
            status: Some(TaskStatus::InProgress),
            ..Default::default()
        };
        let updated = store.update_task(&task_id, &patch).unwrap();
        assert!(updated);

        let after = store.get_task(&task_id).unwrap().unwrap();
        assert_eq!(after.status, TaskStatus::InProgress);
        // Everything else preserved.
        assert_eq!(after.title, "Original");
        assert_eq!(after.tags, vec!["a", "b"]);
        assert_eq!(after.priority, Priority::High);
        assert_eq!(after.openspec_change.as_deref(), Some("change-x"));
        assert!(after.execution.is_some());
        assert_eq!(after.execution.unwrap().model.as_deref(), Some("m"));
    }

    #[test]
    fn list_tasks_tag_filter_uses_intersection_against_real_data() {
        // Sentry's discovery query: column + status + multi-tag against a
        // populated DB. Ensures the json_each-based SQL is actually correct
        // when there are tasks with overlapping but not identical tag sets.
        let (_tmp, store) = fresh_store();
        let board_id = seed_board(&store);

        let bid = board_id.clone();
        let mk = |col: &str, tags: &[&str], st: TaskStatus| {
            let mut t = Task::new(bid.clone(), col, format!("T-{col}-{:?}", st));
            t.tags = tags.iter().map(|s| (*s).to_string()).collect();
            t.status = st;
            store.save_task(&t).unwrap();
            t.id
        };

        let _a = mk("Scheduled", &["sentry", "recurring"], TaskStatus::Todo);
        let _b = mk("Scheduled", &["sentry"],              TaskStatus::Todo);
        let _c = mk("Scheduled", &["recurring"],           TaskStatus::Todo);
        let _d = mk("Backlog",   &["sentry", "recurring"], TaskStatus::InProgress);

        let intersection = store.list_tasks(&TaskFilter {
            board_id: Some(board_id),
            column: Some("Scheduled".into()),
            tags: vec!["sentry".into(), "recurring".into()],
            status: Some(TaskStatus::Todo),
            ..Default::default()
        }).unwrap();
        assert_eq!(intersection.len(), 1, "only A matches all four predicates");
        assert_eq!(intersection[0].tags, vec!["sentry", "recurring"]);
    }

    #[test]
    fn update_task_clear_sentinels_actually_clear() {
        // Some(None) on Option<Option<T>> patch fields means CLEAR. Verify
        // that's what happens, not "preserve" or "set to default".
        let (_tmp, store) = fresh_store();
        let board_id = seed_board(&store);
        let task_id = TaskId(uuid::Uuid::new_v4());
        let mut t = Task::new(board_id.clone(), "Backlog", "T");
        t.id = task_id.clone();
        t.design_node_id = Some(uuid::Uuid::new_v4());
        t.openspec_change = Some("change-x".into());
        t.execution = Some(ExecutionSpec { model: Some("m".into()), ..Default::default() });
        store.save_task(&t).unwrap();

        let patch = TaskPatch {
            design_node_id: Some(None),
            openspec_change: Some(None),
            execution: Some(None),
            ..Default::default()
        };
        store.update_task(&task_id, &patch).unwrap();

        let after = store.get_task(&task_id).unwrap().unwrap();
        assert!(after.design_node_id.is_none());
        assert!(after.openspec_change.is_none());
        assert!(after.execution.is_none());
    }

    #[test]
    fn update_task_returns_false_for_missing_id_without_creating() {
        // Soft-failure path: missing id → false return, no insert.
        let (_tmp, store) = fresh_store();
        let phantom = TaskId(uuid::Uuid::new_v4());
        let patch = TaskPatch {
            title: Some("Should not be inserted".into()),
            ..Default::default()
        };
        let result = store.update_task(&phantom, &patch).unwrap();
        assert!(!result);
        assert!(store.get_task(&phantom).unwrap().is_none());
    }

    #[test]
    fn empty_patch_returns_true_when_task_exists_no_write() {
        // The is_empty() short-circuit. Verify no spurious updated_at bump.
        let (_tmp, store) = fresh_store();
        let board_id = seed_board(&store);
        let task_id = TaskId(uuid::Uuid::new_v4());
        let mut t = Task::new(board_id.clone(), "Backlog", "T");
        t.id = task_id.clone();
        store.save_task(&t).unwrap();
        let original_ts = store.get_task(&task_id).unwrap().unwrap().updated_at;

        let patch = TaskPatch::default();
        let result = store.update_task(&task_id, &patch).unwrap();
        assert!(result);
        let after = store.get_task(&task_id).unwrap().unwrap();
        assert_eq!(after.updated_at, original_ts, "empty patch must not bump updated_at");
    }

    #[test]
    fn engagement_id_round_trips_through_sqlite() {
        use flynt_models::engagement::EngagementId;
        let (_tmp, store) = fresh_store();
        let board_id = seed_board(&store);
        let mut t = Task::new(board_id.clone(), "Backlog", "T");
        let eid = EngagementId::new();
        t.engagement_id = Some(eid.clone());
        store.save_task(&t).unwrap();

        let loaded = store.get_task(&t.id).unwrap().unwrap();
        assert_eq!(loaded.engagement_id, Some(eid));
    }

    #[test]
    fn list_tasks_filters_by_engagement() {
        use flynt_models::engagement::EngagementId;
        let (_tmp, store) = fresh_store();
        let board_id = seed_board(&store);
        let eid_a = EngagementId::new();
        let eid_b = EngagementId::new();

        let mut a1 = Task::new(board_id.clone(), "Backlog", "A1");
        a1.engagement_id = Some(eid_a.clone());
        let mut a2 = Task::new(board_id.clone(), "Backlog", "A2");
        a2.engagement_id = Some(eid_a.clone());
        let mut b1 = Task::new(board_id.clone(), "Backlog", "B1");
        b1.engagement_id = Some(eid_b.clone());
        let unscoped = Task::new(board_id.clone(), "Backlog", "Unscoped");

        for t in [&a1, &a2, &b1, &unscoped] { store.save_task(t).unwrap(); }

        let only_a = store.list_tasks(&TaskFilter {
            engagement_id: Some(eid_a),
            ..Default::default()
        }).unwrap();
        assert_eq!(only_a.len(), 2, "expected only the two A-engagement tasks");
        assert!(only_a.iter().all(|t| t.title == "A1" || t.title == "A2"));
    }

    #[test]
    fn update_task_engagement_id_set_and_clear() {
        use flynt_models::engagement::EngagementId;
        let (_tmp, store) = fresh_store();
        let board_id = seed_board(&store);
        let task_id = TaskId(uuid::Uuid::new_v4());
        let mut t = Task::new(board_id.clone(), "Backlog", "T");
        t.id = task_id.clone();
        store.save_task(&t).unwrap();

        // Set
        let eid = EngagementId::new();
        let patch = TaskPatch {
            engagement_id: Some(Some(eid.clone())),
            ..Default::default()
        };
        store.update_task(&task_id, &patch).unwrap();
        assert_eq!(
            store.get_task(&task_id).unwrap().unwrap().engagement_id,
            Some(eid),
        );

        // Clear via Some(None)
        let patch = TaskPatch {
            engagement_id: Some(None),
            ..Default::default()
        };
        store.update_task(&task_id, &patch).unwrap();
        assert!(store.get_task(&task_id).unwrap().unwrap().engagement_id.is_none());
    }

    // ── Engagement table persistence (migration v7) ────────────────────────

    fn sample_engagement() -> flynt_models::engagement::Engagement {
        use flynt_models::engagement::{Engagement, RepoBinding};
        use styrene_forge::{ForgeEndpoint, ForgeKind};
        let mut e = Engagement::new(
            "Q2 Migration",
            ForgeEndpoint {
                id: "github".into(),
                kind: ForgeKind::GitHub,
                base_url: "https://api.github.com".into(),
                token_secret: Some("GITHUB_TOKEN".into()),
            },
        );
        e.description = Some("Audit + migration of legacy auth path".into());
        e.repos.push(RepoBinding::new("anthropics", "claude-code"));
        e.repos.push(RepoBinding::new("anthropics", "claude-sdk"));
        e
    }

    #[test]
    fn engagement_round_trips_through_sqlite() {
        let (_tmp, store) = fresh_store();
        let e = sample_engagement();
        store.save_engagement(&e).unwrap();
        let loaded = store.get_engagement(&e.id).unwrap().expect("engagement should load");
        assert_eq!(loaded.id, e.id);
        assert_eq!(loaded.name, "Q2 Migration");
        assert_eq!(loaded.description.as_deref(), Some("Audit + migration of legacy auth path"));
        assert_eq!(loaded.repos.len(), 2);
        assert_eq!(loaded.repos[0].full_name(), "anthropics/claude-code");
        assert_eq!(loaded.forge.kind, styrene_forge::ForgeKind::GitHub);
        assert!(matches!(loaded.status, flynt_models::engagement::EngagementStatus::Active));
    }

    #[test]
    fn list_engagements_returns_sorted_by_name() {
        let (_tmp, store) = fresh_store();
        let mut a = sample_engagement(); a.name = "Beta".into();
        let mut b = sample_engagement(); b.name = "Alpha".into();
        store.save_engagement(&a).unwrap();
        store.save_engagement(&b).unwrap();
        let list = store.list_engagements().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Alpha");
        assert_eq!(list[1].name, "Beta");
    }

    #[test]
    fn delete_engagement_returns_true_for_existing_false_for_missing() {
        use flynt_models::engagement::EngagementId;
        let (_tmp, store) = fresh_store();
        let e = sample_engagement();
        store.save_engagement(&e).unwrap();
        assert!(store.delete_engagement(&e.id).unwrap());
        assert!(store.get_engagement(&e.id).unwrap().is_none());
        // Second delete: nothing to remove → false.
        assert!(!store.delete_engagement(&EngagementId::new()).unwrap());
    }

    #[test]
    fn delete_engagement_does_not_cascade_to_tasks() {
        // Soft-coupling: tasks keep their engagement_id pointing at a
        // now-missing record. Callers treat it as "no engagement," not
        // an error — matches the trait doc and avoids data loss.
        let (_tmp, store) = fresh_store();
        let board_id = seed_board(&store);
        let e = sample_engagement();
        let eid = e.id.clone();
        store.save_engagement(&e).unwrap();

        let mut t = Task::new(board_id.clone(), "Backlog", "T");
        t.engagement_id = Some(eid.clone());
        store.save_task(&t).unwrap();

        assert!(store.delete_engagement(&eid).unwrap());
        let after = store.get_task(&t.id).unwrap().unwrap();
        assert_eq!(after.engagement_id, Some(eid), "task should still carry the dangling id");
        assert!(store.get_engagement(&after.engagement_id.unwrap()).unwrap().is_none());
    }

    #[test]
    fn save_engagement_is_upsert() {
        // Same id, mutated name → row is updated, not duplicated.
        let (_tmp, store) = fresh_store();
        let mut e = sample_engagement();
        store.save_engagement(&e).unwrap();
        e.name = "Renamed".into();
        store.save_engagement(&e).unwrap();
        let list = store.list_engagements().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Renamed");
    }
}
