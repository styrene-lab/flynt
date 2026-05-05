use anyhow::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncStatus {
    Idle,
    Syncing,
    Conflict(Vec<String>),
    Error(String),
}

pub struct SyncResult {
    pub files_pulled: usize,
    pub files_pushed: usize,
    pub conflicts: Vec<String>,
}

/// Pluggable sync backend. Implementations live in `flynt-store`.
pub trait SyncBackend: Send + Sync {
    fn name(&self) -> &str;
    fn status(&self) -> Result<SyncStatus>;
    fn pull(&self) -> Result<SyncResult>;
    fn push(&self) -> Result<SyncResult>;
    fn sync(&self) -> Result<SyncResult>;
}
