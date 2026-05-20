//! `IssueMap` persistence — the bridge between flynt task UUIDs and
//! forge issue numbers.
//!
//! Owns its own SQLite file (default `<project>/.flynt/forge-sync.db`)
//! rather than colocating with the main project DB. Rationale: forge sync
//! is an opt-in side channel; keeping it separate means a project that's
//! never been wired to a forge has no schema overhead, and re-sync /
//! reset is just `rm forge-sync.db`.
//!
//! Re-exports the [`IssueMap`] struct from `sync` for caller convenience.

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

pub use crate::sync::IssueMap;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS issue_maps (
    local_id            TEXT PRIMARY KEY,
    board_id            TEXT NOT NULL,
    forge_org           TEXT NOT NULL,
    forge_repo          TEXT NOT NULL,
    forge_issue_number  INTEGER NOT NULL,
    last_synced         TEXT NOT NULL,
    last_hash           TEXT,
    forge_url           TEXT,
    UNIQUE(forge_org, forge_repo, forge_issue_number)
);
CREATE INDEX IF NOT EXISTS idx_issue_maps_repo
    ON issue_maps (forge_org, forge_repo);
CREATE INDEX IF NOT EXISTS idx_issue_maps_board
    ON issue_maps (board_id);
"#;

pub struct SyncStore {
    conn: Mutex<Connection>,
}

impl SyncStore {
    /// Open (or create) the sync DB at the given path. Parent directory
    /// must exist; we don't create it here because the call site usually
    /// wants to apply different policies (e.g. flynt's `.flynt/` is
    /// already managed by flynt-store).
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("open forge sync db at {}", path.display()))?;
        conn.execute_batch(SCHEMA)
            .context("apply forge sync schema")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// In-memory store, primarily for tests. Schema is applied.
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("open :memory: forge sync db")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Look up by forge identifier. Returns `Ok(None)` if no mapping exists.
    pub fn get_by_issue(&self, org: &str, repo: &str, number: u64) -> Result<Option<IssueMap>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT local_id, board_id, forge_org, forge_repo, forge_issue_number, last_synced, last_hash, forge_url
             FROM issue_maps WHERE forge_org = ?1 AND forge_repo = ?2 AND forge_issue_number = ?3",
        )?;
        let mut rows = stmt.query(params![org, repo, number as i64])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(row_to_map(row)?))
    }

    /// All mappings for one repo. Used by the sync engine to diff
    /// against the freshly-pulled issue list.
    pub fn list_by_repo(&self, org: &str, repo: &str) -> Result<Vec<IssueMap>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT local_id, board_id, forge_org, forge_repo, forge_issue_number, last_synced, last_hash, forge_url
             FROM issue_maps WHERE forge_org = ?1 AND forge_repo = ?2",
        )?;
        let rows = stmt.query_map(params![org, repo], row_to_map)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// All mappings for one local task. Usually one — multi-issue
    /// mirroring of a single task is unusual but allowed by schema.
    pub fn list_by_local(&self, local_id: &Uuid) -> Result<Vec<IssueMap>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT local_id, board_id, forge_org, forge_repo, forge_issue_number, last_synced, last_hash, forge_url
             FROM issue_maps WHERE local_id = ?1",
        )?;
        let rows = stmt.query_map(params![local_id.to_string()], row_to_map)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Upsert. Caller passes the canonical [`IssueMap`] and we replace
    /// any existing row keyed on `local_id`.
    ///
    /// **Cross-uniqueness**: the schema also enforces
    /// `UNIQUE(forge_org, forge_repo, forge_issue_number)`. Attempting
    /// to upsert a *different* `local_id` that points at the same forge
    /// issue returns an error rather than silently rebinding — a forge
    /// issue may only have one local representation. To rebind, the
    /// caller must `delete_by_local(old)` first.
    pub fn upsert(&self, m: &IssueMap) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO issue_maps (local_id, board_id, forge_org, forge_repo, forge_issue_number, last_synced, last_hash, forge_url)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(local_id) DO UPDATE SET
                board_id            = excluded.board_id,
                forge_org           = excluded.forge_org,
                forge_repo          = excluded.forge_repo,
                forge_issue_number  = excluded.forge_issue_number,
                last_synced         = excluded.last_synced,
                last_hash           = excluded.last_hash,
                forge_url           = excluded.forge_url",
            params![
                m.local_id.to_string(),
                m.board_id.to_string(),
                m.forge_org,
                m.forge_repo,
                m.forge_issue_number as i64,
                m.last_synced.to_rfc3339(),
                m.last_hash,
                m.forge_url,
            ],
        )?;
        Ok(())
    }

    /// Drop a mapping. Used when a task is deleted locally and we want
    /// to stop syncing it (without affecting the forge issue).
    pub fn delete_by_local(&self, local_id: &Uuid) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "DELETE FROM issue_maps WHERE local_id = ?1",
            params![local_id.to_string()],
        )?;
        Ok(())
    }
}

fn row_to_map(row: &rusqlite::Row<'_>) -> rusqlite::Result<IssueMap> {
    let local_id: String = row.get(0)?;
    let board_id: String = row.get(1)?;
    let last_synced: String = row.get(5)?;
    Ok(IssueMap {
        local_id: Uuid::parse_str(&local_id).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        board_id: Uuid::parse_str(&board_id).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
        })?,
        forge_org: row.get(2)?,
        forge_repo: row.get(3)?,
        forge_issue_number: row.get::<_, i64>(4)? as u64,
        last_synced: last_synced.parse().unwrap_or_else(|_| chrono::Utc::now()),
        last_hash: row.get(6)?,
        forge_url: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_map(local: Uuid, number: u64) -> IssueMap {
        IssueMap {
            local_id: local,
            board_id: Uuid::new_v4(),
            forge_org: "anthropics".into(),
            forge_repo: "test".into(),
            forge_issue_number: number,
            last_synced: Utc::now(),
            last_hash: Some("abc123".into()),
            forge_url: Some(format!(
                "https://github.com/anthropics/test/issues/{number}"
            )),
        }
    }

    #[test]
    fn upsert_then_get_by_issue() {
        let store = SyncStore::in_memory().unwrap();
        let local = Uuid::new_v4();
        store.upsert(&sample_map(local, 42)).unwrap();
        let got = store.get_by_issue("anthropics", "test", 42).unwrap();
        assert!(got.is_some());
        assert_eq!(got.unwrap().local_id, local);
    }

    #[test]
    fn upsert_replaces_existing_local_id_row() {
        // Same local_id, different forge issue number — UPDATE path.
        let store = SyncStore::in_memory().unwrap();
        let local = Uuid::new_v4();
        store.upsert(&sample_map(local, 10)).unwrap();
        store.upsert(&sample_map(local, 11)).unwrap();
        let by_local = store.list_by_local(&local).unwrap();
        assert_eq!(
            by_local.len(),
            1,
            "second upsert should replace, not duplicate"
        );
        assert_eq!(by_local[0].forge_issue_number, 11);
    }

    #[test]
    fn list_by_repo_returns_only_matching() {
        let store = SyncStore::in_memory().unwrap();
        store.upsert(&sample_map(Uuid::new_v4(), 1)).unwrap();
        store.upsert(&sample_map(Uuid::new_v4(), 2)).unwrap();
        let mut other = sample_map(Uuid::new_v4(), 99);
        other.forge_repo = "other".into();
        store.upsert(&other).unwrap();
        let test_repo = store.list_by_repo("anthropics", "test").unwrap();
        assert_eq!(test_repo.len(), 2);
    }

    #[test]
    fn delete_by_local_removes_row() {
        let store = SyncStore::in_memory().unwrap();
        let local = Uuid::new_v4();
        store.upsert(&sample_map(local, 7)).unwrap();
        store.delete_by_local(&local).unwrap();
        assert!(store.list_by_local(&local).unwrap().is_empty());
    }

    #[test]
    fn upsert_rejects_cross_uniqueness_conflict() {
        // Two different local_ids cannot both point at (org, repo, #).
        // The UNIQUE(forge_org, forge_repo, forge_issue_number) check
        // surfaces as an Err — caller must delete_by_local first to
        // rebind. Test exercises the contract documented on upsert().
        let store = SyncStore::in_memory().unwrap();
        let local_a = Uuid::new_v4();
        let local_b = Uuid::new_v4();
        store.upsert(&sample_map(local_a, 42)).unwrap();

        let conflict = sample_map(local_b, 42);
        let err = store.upsert(&conflict);
        assert!(err.is_err(), "expected UNIQUE violation, got: {err:?}");

        // Original mapping still intact.
        let got = store
            .get_by_issue("anthropics", "test", 42)
            .unwrap()
            .unwrap();
        assert_eq!(got.local_id, local_a);

        // Rebind path: delete first, then upsert succeeds.
        store.delete_by_local(&local_a).unwrap();
        store.upsert(&conflict).unwrap();
        let got = store
            .get_by_issue("anthropics", "test", 42)
            .unwrap()
            .unwrap();
        assert_eq!(got.local_id, local_b);
    }

    #[test]
    fn cold_restart_durability() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("forge-sync.db");
        let local = Uuid::new_v4();
        {
            let store = SyncStore::open(&path).unwrap();
            store.upsert(&sample_map(local, 5)).unwrap();
        }
        let reopened = SyncStore::open(&path).unwrap();
        let got = reopened
            .get_by_issue("anthropics", "test", 5)
            .unwrap()
            .unwrap();
        assert_eq!(got.local_id, local);
    }
}
