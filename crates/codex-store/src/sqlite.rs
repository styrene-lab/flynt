use anyhow::Result;
use codex_core::{
    models::*,
    store::{DocumentMetadataFilter, TaskFilter, VaultStore},
};
use rusqlite::{Connection, params};
use std::{path::Path, sync::Mutex};

/// SQLite-backed `VaultStore`.
/// The database file lives at `<vault_root>/.codex/state.db`.
/// Documents are stored as markdown files; the DB is an index + task store.
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    pub fn open(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
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

// ── VaultStore implementation ─────────────────────────────────────────────────

impl VaultStore for SqliteStore {
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
                document_id: DocumentId(row.get::<_, String>(0)?.parse().unwrap()),
                path: row.get::<_, String>(1)?.into(),
                title: row.get(2)?,
                excerpt: row.get(3)?,
                score: 1.0,
            })
        })?;
        Ok(results.collect::<rusqlite::Result<_>>()?)
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
                id: DocumentId(row.get::<_, String>(0)?.parse().unwrap()),
                path: row.get::<_, String>(1)?.into(),
                title: row.get(2)?,
                tags: serde_json::from_str::<Frontmatter>(&fm_json).unwrap_or_default().tags,
                metadata: document_metadata_fields_from_frontmatter_json(&fm_json),
                entity_kind: entity_kind_from_frontmatter_json(&fm_json),
                updated_at: updated_at.parse().unwrap(),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    fn get_task(&self, id: &TaskId) -> Result<Option<Task>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, board_id, column_name, title, description, priority, status, tags, document_refs, due_date, position, created_at, updated_at FROM tasks WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id.0.to_string()])?;
        let Some(row) = rows.next()? else { return Ok(None) };
        Ok(Some(row_to_task(row)?))
    }

    fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>> {
        let conn = self.conn.lock().unwrap();
        // Build query dynamically
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
        let sql = format!(
            "SELECT id, board_id, column_name, title, description, priority, status, tags, document_refs, due_date, position, created_at, updated_at FROM tasks WHERE {} ORDER BY position ASC",
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
        conn.execute(
            r#"INSERT INTO tasks (id, board_id, column_name, title, description, priority, status, tags, document_refs, due_date, position, created_at, updated_at)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
               ON CONFLICT(id) DO UPDATE SET
                 board_id=excluded.board_id, column_name=excluded.column_name,
                 title=excluded.title, description=excluded.description,
                 priority=excluded.priority, status=excluded.status,
                 tags=excluded.tags, document_refs=excluded.document_refs,
                 due_date=excluded.due_date, position=excluded.position,
                 updated_at=excluded.updated_at"#,
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
            ],
        )?;
        Ok(())
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
        id: DocumentId(source_id.parse().unwrap()),
        path: path_str.into(),
        title: row.get(2)?,
        content: row.get(3)?,
        outgoing_links,
        frontmatter,
        created_at: created_at.parse().unwrap(),
        updated_at: updated_at.parse().unwrap(),
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
    Ok(Task {
        id: TaskId(row.get::<_, String>(0)?.parse().unwrap()),
        board_id: BoardId(row.get::<_, String>(1)?.parse().unwrap()),
        column: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        priority: serde_json::from_str(&priority).unwrap_or_default(),
        status: serde_json::from_str(&status).unwrap_or_default(),
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        document_refs: serde_json::from_str(&refs_json).unwrap_or_default(),
        due_date: due.and_then(|s| s.parse().ok()),
        position: row.get(10)?,
        created_at: created_at.parse().unwrap(),
        updated_at: updated_at.parse().unwrap(),
    })
}

fn row_to_board(row: &rusqlite::Row<'_>) -> rusqlite::Result<Board> {
    let cols_json: String = row.get(2)?;
    let created_at: String = row.get(3)?;
    Ok(Board {
        id: BoardId(row.get::<_, String>(0)?.parse().unwrap()),
        name: row.get(1)?,
        columns: serde_json::from_str(&cols_json).unwrap_or_default(),
        created_at: created_at.parse().unwrap(),
    })
}

fn entity_kind_from_frontmatter_json(frontmatter_json: &str) -> Option<codex_core::datum::EntityKind> {
    serde_json::from_str::<Frontmatter>(frontmatter_json)
        .ok()?
        .kind
        .map(|k| codex_core::datum::EntityKind::from_str(&k))
}

fn entity_from_frontmatter(fm: &Frontmatter) -> Option<codex_core::datum::Entity> {
    let kind_str = fm.kind.as_deref()?;
    let kind = codex_core::datum::EntityKind::from_str(kind_str);
    let id = fm.id.unwrap_or_else(uuid::Uuid::new_v4);
    let mut fields = std::collections::BTreeMap::new();
    if let Some(toml::Value::Table(data)) = &fm.data {
        for (k, v) in data {
            fields.insert(k.clone(), codex_core::datum::Datum::from(v.clone()));
        }
    }
    Some(codex_core::datum::Entity {
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
