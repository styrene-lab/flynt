use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct ConflictRecord {
    pub id: String,
    pub path: String,
    pub ours: String,
    pub theirs: String,
    pub detected_at: DateTime<Utc>,
}

/// Conflict management methods mixed into SqliteStore via this extension trait.
/// Call these from SqliteStore — they share the same Mutex<Connection> pattern.
pub struct ConflictStore<'a>(pub &'a Mutex<rusqlite::Connection>);

impl<'a> ConflictStore<'a> {
    pub fn insert(&self, path: &str, ours: &str, theirs: &str) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.0.lock().unwrap().execute(
            "INSERT INTO conflicts (id, path, ours, theirs, detected_at) VALUES (?1,?2,?3,?4,?5)",
            params![id, path, ours, theirs, now],
        )?;
        Ok(id)
    }

    pub fn list(&self) -> Result<Vec<ConflictRecord>> {
        let conn = self.0.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, path, ours, theirs, detected_at FROM conflicts ORDER BY detected_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let detected_at: String = row.get(4)?;
            Ok(ConflictRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                ours: row.get(2)?,
                theirs: row.get(3)?,
                detected_at: detected_at.parse().unwrap(),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn resolve(&self, id: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .execute("DELETE FROM conflicts WHERE id = ?1", params![id])?;
        Ok(())
    }
}
