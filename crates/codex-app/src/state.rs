#[derive(Clone, PartialEq, Debug, Default)]
pub enum Route {
    #[default]
    Notes,
    Kanban,
    Graph,
    Settings,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub enum SyncStatus {
    #[default]
    Idle,
    Syncing,
    Conflict(usize),
}
