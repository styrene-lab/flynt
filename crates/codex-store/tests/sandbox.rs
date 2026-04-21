//! Sandbox integration tests for Codex CRUD operations.
//!
//! Tests all major operations: documents, boards, tasks, graph, sync, images.
//! Uses a temporary vault with known fixtures.
//!
//! Run: cargo test --test sandbox -- --nocapture

use chrono::Utc;
use codex_core::{
    graph::{build_graph_payload, force_layout, render_graph_svg, LayoutConfig, GraphNodeKind, GraphEdgeKind},
    models::*,
    store::{TaskFilter, VaultStore},
};
use codex_store::vault::Vault;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn setup_vault() -> (TempDir, Arc<Vault>) {
    let tmp = tempfile::Builder::new().prefix("codex-test-").tempdir().unwrap();
    let root = tmp.path().to_path_buf();

    // Config
    std::fs::create_dir_all(root.join(".codex")).unwrap();
    std::fs::write(
        root.join(".codex/config.toml"),
        r#"vault_name = "test-sandbox"
[sync]
backend = "none"
[appearance]
theme = "alpharius"
font_size = "medium"
[local_runtime]
[publication]
default_visibility = "private"
"#,
    )
    .unwrap();

    // Fixture documents
    write_doc(&root, "Welcome.md", "Welcome", &["meta"], "# Welcome\n\nSee [[Projects]] and [[Architecture]].");
    write_doc(&root, "Projects.md", "Projects", &["index"], "# Projects\n\n- [[Codex]]\n- [[Omegon]]");
    write_doc(&root, "Architecture.md", "Architecture", &["engineering"], "# Architecture\n\n| Layer | Crate |\n|---|---|\n| Core | codex-core |\n\nSee [[Projects]].");
    write_doc(&root, "Orphan.md", "Orphan Note", &["stale"], "# Orphan\n\nNo links to or from anywhere.");

    std::fs::create_dir_all(root.join("Research")).unwrap();
    write_doc(&root, "Research/Graphs.md", "Graph Research", &["research", "graphs"], "# Graphs\n\n[[Architecture]] uses property graphs.");
    write_doc(&root, "Research/Engines.md", "Game Engines", &["research", "gamedev"], "# Game Engines\n\nBevy, Notan, FireOx.");

    // Image
    std::fs::create_dir_all(root.join("assets")).unwrap();
    std::fs::write(root.join("assets/photo.png"), &[0x89, 0x50, 0x4E, 0x47]).unwrap();

    let vault = Arc::new(Vault::open(&root).unwrap());
    let (n, errs) = vault.reindex().unwrap();
    assert!(n >= 6, "Expected at least 6 docs, got {n}");

    (tmp, vault)
}

fn write_doc(root: &Path, rel: &str, title: &str, tags: &[&str], body: &str) {
    let tags_str: Vec<String> = tags.iter().map(|t| format!("\"{}\"", t)).collect();
    let content = format!("+++\ntitle = \"{title}\"\ntags = [{}]\n+++\n\n{body}", tags_str.join(", "));
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

// ── Document CRUD ────────────────────────────────────────────────────────────

#[test]
fn test_list_documents() {
    let (_tmp, vault) = setup_vault();
    let docs = vault.store.list_documents().unwrap();
    assert!(docs.len() >= 6);

    let titles: Vec<&str> = docs.iter().map(|d| d.title.as_str()).collect();
    assert!(titles.contains(&"Welcome"), "Missing Welcome doc");
    assert!(titles.contains(&"Projects"), "Missing Projects doc");
    assert!(titles.contains(&"Architecture"), "Missing Architecture doc");
    assert!(titles.contains(&"Orphan"), "Missing Orphan doc");
}

#[test]
fn test_search_documents() {
    let (_tmp, vault) = setup_vault();
    let results = vault.store.search_documents("architecture").unwrap();
    assert!(!results.is_empty(), "Search for 'architecture' returned nothing");
    assert!(results.iter().any(|d| d.title == "Architecture"));
}

#[test]
fn test_get_document_by_path() {
    let (_tmp, vault) = setup_vault();
    let doc = vault.store.get_document_by_path(Path::new("Welcome.md")).unwrap();
    assert!(doc.is_some());
    let doc = doc.unwrap();
    assert_eq!(doc.title, "Welcome");
    assert!(doc.content.contains("[[Projects]]"));
}

#[test]
fn test_find_document_by_slug() {
    let (_tmp, vault) = setup_vault();
    let doc = vault.store.find_document_by_slug("welcome").unwrap();
    assert!(doc.is_some());
    assert_eq!(doc.unwrap().title, "Welcome");
}

#[test]
fn test_create_and_update_document() {
    let (tmp, vault) = setup_vault();

    // Create
    let path = Path::new("New Note.md");
    let content = "+++\ntitle = \"New Note\"\ntags = [\"test\"]\n+++\n\n# New Note\n\nCreated by test.";
    vault.save_document_content(path, content).unwrap();

    // Reindex to pick it up
    vault.reindex().unwrap();
    let doc = vault.store.get_document_by_path(path).unwrap().unwrap();
    assert_eq!(doc.title, "New Note");
    assert!(doc.frontmatter.tags.contains(&"test".to_string()));

    // Update
    let updated = "+++\ntitle = \"New Note Updated\"\ntags = [\"test\", \"updated\"]\n+++\n\n# New Note Updated\n\nModified.";
    vault.save_document_content(path, updated).unwrap();
    vault.reindex().unwrap();
    let doc2 = vault.store.get_document_by_path(path).unwrap().unwrap();
    assert_eq!(doc2.title, "New Note Updated");
    assert!(doc2.frontmatter.tags.contains(&"updated".to_string()));
}

#[test]
fn test_backlinks() {
    let (_tmp, vault) = setup_vault();
    // Architecture is linked from Welcome and Research/Graphs
    let arch = vault.store.find_document_by_slug("architecture").unwrap().unwrap();
    let backlinks = vault.store.get_backlinks(&arch.id).unwrap();
    assert!(backlinks.len() >= 2, "Expected ≥2 backlinks to Architecture, got {}", backlinks.len());
}

// ── Board CRUD ───────────────────────────────────────────────────────────────

#[test]
fn test_create_and_list_boards() {
    let (_tmp, vault) = setup_vault();

    // No boards initially
    let boards = vault.store.list_boards().unwrap();
    assert!(boards.is_empty());

    // Create
    let board = Board::default_sprint("Test Sprint");
    vault.store.save_board(&board).unwrap();

    let boards = vault.store.list_boards().unwrap();
    assert_eq!(boards.len(), 1);
    assert_eq!(boards[0].name, "Test Sprint");
    assert_eq!(boards[0].columns.len(), 4); // Backlog, In Progress, Review, Done
}

#[test]
fn test_get_board() {
    let (_tmp, vault) = setup_vault();
    let board = Board::default_sprint("My Board");
    vault.store.save_board(&board).unwrap();

    let fetched = vault.store.get_board(&board.id).unwrap();
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().name, "My Board");
}

#[test]
fn test_board_with_project() {
    let (_tmp, vault) = setup_vault();
    let project_id = uuid::Uuid::new_v4();
    let board = Board::for_project("Project Board", project_id);
    vault.store.save_board(&board).unwrap();

    let fetched = vault.store.get_board(&board.id).unwrap().unwrap();
    assert_eq!(fetched.project_id, Some(project_id));
}

// ── Task CRUD ────────────────────────────────────────────────────────────────

#[test]
fn test_create_and_list_tasks() {
    let (_tmp, vault) = setup_vault();
    let board = Board::default_sprint("Sprint 1");
    vault.store.save_board(&board).unwrap();

    // Create tasks
    let t1 = Task::new(board.id.clone(), "Backlog", "Fix login bug");
    let t2 = Task::new(board.id.clone(), "Backlog", "Add dark mode");
    let t3 = Task::new(board.id.clone(), "In Progress", "Write tests");
    vault.store.save_task(&t1).unwrap();
    vault.store.save_task(&t2).unwrap();
    vault.store.save_task(&t3).unwrap();

    let all = vault.store.list_tasks(&TaskFilter {
        board_id: Some(board.id.clone()),
        ..Default::default()
    }).unwrap();
    assert_eq!(all.len(), 3);

    // Filter by column
    let backlog = vault.store.list_tasks(&TaskFilter {
        board_id: Some(board.id.clone()),
        column: Some("Backlog".into()),
        ..Default::default()
    }).unwrap();
    assert_eq!(backlog.len(), 2);
}

#[test]
fn test_update_task() {
    let (_tmp, vault) = setup_vault();
    let board = Board::default_sprint("Sprint");
    vault.store.save_board(&board).unwrap();

    let mut task = Task::new(board.id.clone(), "Backlog", "Original title");
    vault.store.save_task(&task).unwrap();

    // Update
    task.title = "Updated title".into();
    task.description = "Added description".into();
    task.priority = Priority::High;
    task.column = "In Progress".into();
    task.updated_at = Utc::now();
    vault.store.save_task(&task).unwrap();

    let fetched = vault.store.get_task(&task.id).unwrap().unwrap();
    assert_eq!(fetched.title, "Updated title");
    assert_eq!(fetched.description, "Added description");
    assert_eq!(fetched.priority, Priority::High);
    assert_eq!(fetched.column, "In Progress");
}

#[test]
fn test_archive_task() {
    let (_tmp, vault) = setup_vault();
    let board = Board::default_sprint("Sprint");
    vault.store.save_board(&board).unwrap();

    let mut task = Task::new(board.id.clone(), "Done", "Completed task");
    vault.store.save_task(&task).unwrap();

    task.status = TaskStatus::Archived;
    vault.store.save_task(&task).unwrap();

    let fetched = vault.store.get_task(&task.id).unwrap().unwrap();
    assert_eq!(fetched.status, TaskStatus::Archived);

    // Archived tasks still in list but filtered by UI
    let all = vault.store.list_tasks(&TaskFilter {
        board_id: Some(board.id.clone()),
        ..Default::default()
    }).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].status, TaskStatus::Archived);
}

#[test]
fn test_task_with_due_date_and_tags() {
    let (_tmp, vault) = setup_vault();
    let board = Board::default_sprint("Sprint");
    vault.store.save_board(&board).unwrap();

    let mut task = Task::new(board.id.clone(), "Backlog", "Tagged task");
    task.tags = vec!["urgent".into(), "frontend".into()];
    task.due_date = Some(chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap());
    vault.store.save_task(&task).unwrap();

    let fetched = vault.store.get_task(&task.id).unwrap().unwrap();
    assert_eq!(fetched.tags, vec!["urgent", "frontend"]);
    assert_eq!(fetched.due_date, Some(chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()));
}

// ── Knowledge Graph ──────────────────────────────────────────────────────────

#[test]
fn test_graph_payload() {
    let (_tmp, vault) = setup_vault();
    let graph = build_graph_payload(&*vault.store).unwrap();

    // Should have nodes for all documents
    assert!(graph.nodes.len() >= 6);
    assert!(graph.nodes.iter().any(|n| n.kind == GraphNodeKind::Document));

    // Should have wikilink edges
    assert!(!graph.edges.is_empty());
    assert!(graph.edges.iter().any(|e| e.kind == GraphEdgeKind::Wikilink));

    // Groups from subdirectories
    assert!(graph.groups.contains(&"Research".to_string()));

    // Tags
    assert!(graph.all_tags.contains(&"engineering".to_string()));
    assert!(graph.all_tags.contains(&"research".to_string()));
}

#[test]
fn test_graph_with_boards_and_tasks() {
    let (_tmp, vault) = setup_vault();
    let board = Board::default_sprint("Sprint 1");
    vault.store.save_board(&board).unwrap();
    let task = Task::new(board.id.clone(), "Backlog", "Test task");
    vault.store.save_task(&task).unwrap();

    let graph = build_graph_payload(&*vault.store).unwrap();
    assert!(graph.nodes.iter().any(|n| n.kind == GraphNodeKind::Board));
    assert!(graph.nodes.iter().any(|n| n.kind == GraphNodeKind::Task));
    assert!(graph.edges.iter().any(|e| e.kind == GraphEdgeKind::TaskMembership));
}

#[test]
fn test_force_layout() {
    let (_tmp, vault) = setup_vault();
    let graph = build_graph_payload(&*vault.store).unwrap();
    let config = LayoutConfig::default();
    let positions = force_layout(&graph, &config);

    assert_eq!(positions.len(), graph.nodes.len());

    // All positions should be within bounds
    for (x, y) in &positions {
        assert!(*x >= 0.0 && *x <= config.width, "x={x} out of bounds");
        assert!(*y >= 0.0 && *y <= config.height, "y={y} out of bounds");
    }
}

#[test]
fn test_render_graph_svg() {
    let (_tmp, vault) = setup_vault();
    let graph = build_graph_payload(&*vault.store).unwrap();
    let config = LayoutConfig { width: 400.0, height: 300.0, ..Default::default() };
    let svg = render_graph_svg(&graph, &config);

    assert!(svg.starts_with("<svg"));
    assert!(svg.ends_with("</svg>"));
    assert!(svg.contains("<circle")); // nodes
    assert!(svg.contains("<line")); // edges
    assert!(svg.contains("<text")); // labels
}

// ── Orphan detection ─────────────────────────────────────────────────────────

#[test]
fn test_orphan_nodes() {
    let (_tmp, vault) = setup_vault();
    let graph = build_graph_payload(&*vault.store).unwrap();

    // Orphan.md has no links — should have degree 0
    let orphan = graph.nodes.iter().find(|n| n.title == "Orphan").unwrap();
    let degree = graph.edges.iter().filter(|e| e.source == orphan.id || e.target == orphan.id).count();
    assert_eq!(degree, 0, "Orphan should have no edges");

    // Game Engines also has no wikilinks (no [[...]] in content)
    let engines = graph.nodes.iter().find(|n| n.title == "Game Engines").unwrap();
    let eng_degree = graph.edges.iter().filter(|e| e.source == engines.id || e.target == engines.id).count();
    assert_eq!(eng_degree, 0, "Game Engines should have no edges");
}

// ── Document deletion ────────────────────────────────────────────────────────

#[test]
fn test_delete_document() {
    let (tmp, vault) = setup_vault();

    // Verify orphan exists
    let orphan = vault.store.find_document_by_slug("orphan").unwrap();
    assert!(orphan.is_some(), "Orphan should exist before deletion");

    // Delete the file
    std::fs::remove_file(tmp.path().join("Orphan.md")).unwrap();

    // File is gone from disk
    assert!(!tmp.path().join("Orphan.md").exists());

    // Note: reindex only adds/updates — doesn't prune deleted docs from DB.
    // The document still exists in SQLite until a full prune is implemented.
    // This test validates the file was removed from disk.
}

// ── Memory / Communications ─────────────────────────────────────────────────

#[test]
fn test_store_memory_fact() {
    let (tmp, vault) = setup_vault();
    let path = vault.store_memory_fact("testing", "Test Fact", "This is a test memory fact.").unwrap();
    assert!(tmp.path().join(&path).exists());

    vault.reindex().unwrap();
    let docs = vault.store.list_documents().unwrap();
    assert!(docs.iter().any(|d| d.title == "Test Fact"));
}

#[test]
fn test_store_communication() {
    let (tmp, vault) = setup_vault();
    let path = vault.store_agent_communication("testing", "Test Comm", "Agent communication content.").unwrap();
    assert!(tmp.path().join(&path).exists());

    vault.reindex().unwrap();
    let docs = vault.store.list_documents().unwrap();
    assert!(docs.iter().any(|d| d.title == "Test Comm"));
}

// ── Config ───────────────────────────────────────────────────────────────────

#[test]
fn test_vault_config() {
    let (_tmp, vault) = setup_vault();
    assert_eq!(vault.config.vault_name, "test-sandbox");
    assert_eq!(vault.config.sync, SyncConfig::None);
    assert_eq!(vault.config.appearance.theme, "alpharius");
}

#[test]
fn test_save_config() {
    let (_tmp, vault) = setup_vault();
    let mut config = vault.config.clone();
    config.vault_name = "updated-sandbox".into();
    vault.save_config(&config).unwrap();

    let vault2 = Vault::open(&vault.root).unwrap();
    assert_eq!(vault2.config.vault_name, "updated-sandbox");
}

// ── Multiple boards ──────────────────────────────────────────────────────────

#[test]
fn test_multiple_boards_isolation() {
    let (_tmp, vault) = setup_vault();

    let b1 = Board::default_sprint("Board A");
    let b2 = Board::default_sprint("Board B");
    vault.store.save_board(&b1).unwrap();
    vault.store.save_board(&b2).unwrap();

    let t1 = Task::new(b1.id.clone(), "Backlog", "Task on A");
    let t2 = Task::new(b2.id.clone(), "Backlog", "Task on B");
    vault.store.save_task(&t1).unwrap();
    vault.store.save_task(&t2).unwrap();

    let a_tasks = vault.store.list_tasks(&TaskFilter {
        board_id: Some(b1.id.clone()), ..Default::default()
    }).unwrap();
    let b_tasks = vault.store.list_tasks(&TaskFilter {
        board_id: Some(b2.id.clone()), ..Default::default()
    }).unwrap();

    assert_eq!(a_tasks.len(), 1);
    assert_eq!(b_tasks.len(), 1);
    assert_eq!(a_tasks[0].title, "Task on A");
    assert_eq!(b_tasks[0].title, "Task on B");
}

// ── Document Rename + Link Update ────────────────────────────────────────

#[test]
fn test_rename_document_updates_links() {
    let (tmp, vault) = setup_vault();

    // Welcome.md links to [[Projects]] and [[Architecture]]
    // Rename "Projects" to "Active Projects"
    let files_updated = vault.rename_document(
        Path::new("Projects.md"),
        "Active Projects",
    ).unwrap();

    // The file should be renamed on disk
    assert!(!tmp.path().join("Projects.md").exists());
    assert!(tmp.path().join("Active Projects.md").exists());

    // The new file should have updated title in frontmatter
    let content = std::fs::read_to_string(tmp.path().join("Active Projects.md")).unwrap();
    assert!(content.contains("title = \"Active Projects\""));

    // Welcome.md should have [[Active Projects]] instead of [[Projects]]
    let welcome = std::fs::read_to_string(tmp.path().join("Welcome.md")).unwrap();
    assert!(welcome.contains("[[Active Projects]]"), "Welcome should link to Active Projects, got: {welcome}");
    assert!(!welcome.contains("[[Projects]]"), "Welcome should not have old link");

    // At least Welcome.md was updated
    assert!(files_updated >= 1, "Expected at least 1 file updated, got {files_updated}");
}

#[test]
fn test_rename_preserves_display_links() {
    let (tmp, vault) = setup_vault();

    // Create a doc with a display link
    write_doc(
        tmp.path(),
        "Linker.md",
        "Linker",
        &[],
        "See [[Projects|our projects]] for more.",
    );
    vault.reindex().unwrap();

    vault.rename_document(Path::new("Projects.md"), "Active Projects").unwrap();

    let linker = std::fs::read_to_string(tmp.path().join("Linker.md")).unwrap();
    assert!(linker.contains("[[Active Projects|our projects]]"), "Display link should be preserved: {linker}");
}

// ── Task Decay ───────────────────────────────────────────────────────────────

#[test]
fn test_decay_relevance_fresh_task() {
    let task = Task::new(BoardId::new(), "Backlog", "Fresh task");
    let r = task.relevance();
    assert!(r > 0.99, "Fresh task should be ~1.0, got {r}");
    assert!(!task.is_fading());
    assert!(!task.should_auto_archive());
}

#[test]
fn test_decay_relevance_no_decay() {
    let mut task = Task::new_tracked(BoardId::new(), "Backlog", "Tracked task");
    // Simulate old updated_at
    task.updated_at = Utc::now() - chrono::Duration::days(30);
    let r = task.relevance();
    assert_eq!(r, 1.0, "Non-decaying task should always be 1.0");
}

#[test]
fn test_decay_relevance_natural_7day() {
    let mut task = Task::new(BoardId::new(), "Backlog", "Old task");
    task.decay = DecayRate::Natural; // 7 day half-life
    task.updated_at = Utc::now() - chrono::Duration::days(7);
    let r = task.relevance();
    // After 1 half-life, relevance should be ~0.5
    assert!(r > 0.45 && r < 0.55, "After 7 days natural decay should be ~0.5, got {r}");
}

#[test]
fn test_decay_relevance_fast() {
    let mut task = Task::new(BoardId::new(), "Backlog", "Quick errand");
    task.decay = DecayRate::Fast; // 3 day half-life
    task.updated_at = Utc::now() - chrono::Duration::days(9);
    // After 3 half-lives: 0.5^3 = 0.125
    let r = task.relevance();
    assert!(r < 0.2, "After 9 days fast decay should be <0.2, got {r}");
    assert!(task.is_fading());
}

#[test]
fn test_decay_auto_archive_threshold() {
    let mut task = Task::new(BoardId::new(), "Backlog", "Forgotten task");
    task.decay = DecayRate::Natural;
    task.updated_at = Utc::now() - chrono::Duration::days(25);
    // After ~3.5 half-lives: 0.5^3.5 ≈ 0.088
    let r = task.relevance();
    assert!(r < 0.1, "After 25 days should auto-archive, got {r}");
    assert!(task.should_auto_archive());
}

#[test]
fn test_decay_touch_resets_clock() {
    let mut task = Task::new(BoardId::new(), "Backlog", "Touched task");
    task.decay = DecayRate::Fast;
    task.updated_at = Utc::now() - chrono::Duration::days(10); // very decayed
    assert!(task.is_fading());

    task.touch(); // resets the clock
    let r = task.relevance();
    assert!(r > 0.99, "After touch, should be fresh, got {r}");
    assert!(!task.is_fading());
}

#[test]
fn test_decay_done_tasks_zero_relevance() {
    let mut task = Task::new(BoardId::new(), "Done", "Completed");
    task.status = TaskStatus::Done;
    assert_eq!(task.relevance(), 0.0);
}

#[test]
fn test_decay_persistence() {
    let (_tmp, vault) = setup_vault();
    let board = Board::default_sprint("Sprint");
    vault.store.save_board(&board).unwrap();

    let mut task = Task::new(board.id.clone(), "Backlog", "Decaying task");
    task.decay = DecayRate::Fast;
    task.touch();
    vault.store.save_task(&task).unwrap();

    let fetched = vault.store.get_task(&task.id).unwrap().unwrap();
    assert_eq!(fetched.decay, DecayRate::Fast);
    assert!(fetched.last_touched_at.is_some());
}

#[test]
fn test_decay_custom_rate() {
    let mut task = Task::new(BoardId::new(), "Backlog", "Custom decay");
    task.decay = DecayRate::Custom(1.0); // 1-day half-life
    task.updated_at = Utc::now() - chrono::Duration::days(1);
    let r = task.relevance();
    assert!(r > 0.45 && r < 0.55, "After 1 day with 1-day half-life should be ~0.5, got {r}");
}

// ── Notifications ────────────────────────────────────────────────────────────

#[test]
fn test_push_and_read_notification() {
    let (_tmp, vault) = setup_vault();
    use codex_core::models::*;

    let notif = Notification::new(
        NotificationKind::DueDate,
        "Mow the lawn",
        "Task is due today",
        "test-sandbox",
    );
    vault.push_notification(&notif).unwrap();

    let pending = vault.pending_notifications().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].title, "Mow the lawn");
    assert!(pending[0].delivered_at.is_none());
}

#[test]
fn test_mark_notification_delivered() {
    let (tmp, vault) = setup_vault();
    use codex_core::models::*;

    let notif = Notification::new(NotificationKind::Decay, "Old task", "Fading", "test-sandbox");
    let id = notif.id;
    vault.push_notification(&notif).unwrap();

    vault.mark_notification_delivered(&id).unwrap();

    // Pending should be empty
    let pending = vault.pending_notifications().unwrap();
    assert!(pending.is_empty());

    // Delivered file should exist
    let delivered_path = tmp.path().join(format!(".codex/notifications/delivered/{id}.json"));
    assert!(delivered_path.exists());
}

#[test]
fn test_check_task_notifications_due_date() {
    let (_tmp, vault) = setup_vault();
    let board = Board::default_sprint("Sprint");
    vault.store.save_board(&board).unwrap();

    let today = chrono::Local::now().date_naive();
    let mut task = Task::new(board.id.clone(), "Backlog", "Due today");
    task.due_date = Some(today);
    task.decay = DecayRate::None;
    vault.store.save_task(&task).unwrap();

    let notifications = vault.check_task_notifications().unwrap();
    assert!(notifications.iter().any(|n| n.title == "Due today" && n.kind == NotificationKind::DueDate));
}

// ── Vault open / reindex ────────────────────────────────────────────────────

#[test]
fn test_vault_open_creates_codex_dir() {
    let tmp = tempfile::Builder::new().prefix("codex-test-").tempdir().unwrap();
    let root = tmp.path().join("fresh-vault");
    let vault = Vault::open(&root).unwrap();
    assert!(root.join(".codex").exists());
    assert!(root.join(".codex/config.toml").exists());
    assert_eq!(vault.config.vault_name, "fresh-vault");
}

#[test]
fn test_vault_open_preserves_existing_config() {
    let tmp = tempfile::Builder::new().prefix("codex-test-").tempdir().unwrap();
    let root = tmp.path().join("vault");
    std::fs::create_dir_all(root.join(".codex")).unwrap();
    std::fs::write(root.join(".codex/config.toml"), "vault_name = \"Custom Name\"\n[sync]\nbackend = \"none\"\n").unwrap();
    let vault = Vault::open(&root).unwrap();
    assert_eq!(vault.config.vault_name, "Custom Name");
}

#[test]
fn test_reindex_counts_files() {
    let (_tmp, vault) = setup_vault();
    let (count, errors) = vault.reindex().unwrap();
    // setup_vault creates alpha.md and beta.md
    assert!(count >= 2);
    assert!(errors.is_empty());
}

#[test]
fn test_reindex_skips_codex_dir() {
    let (_tmp, vault) = setup_vault();
    // Create a file in .codex that should be ignored
    std::fs::write(vault.root.join(".codex/internal.md"), "# Should be ignored").unwrap();
    vault.reindex().unwrap();
    // This file should NOT appear in the document list
    let docs = vault.store.list_documents().unwrap();
    assert!(!docs.iter().any(|d| d.title == "Should be ignored"));
}

// ── Save document ───────────────────────────────────────────────────────────

#[test]
fn test_save_document_content() {
    let (_tmp, vault) = setup_vault();
    let path = std::path::PathBuf::from("new-note.md");
    vault.save_document_content(&path, "+++\ntitle = \"New\"\ntags = []\n+++\n\nContent here.").unwrap();
    let doc = vault.store.get_document_by_path(&path).unwrap().unwrap();
    assert_eq!(doc.title, "New");
    assert!(doc.content.contains("Content here."));
}

#[test]
fn test_save_document_creates_parent_dirs() {
    let (_tmp, vault) = setup_vault();
    let path = std::path::PathBuf::from("nested/deep/note.md");
    vault.save_document_content(&path, "# Deep Note").unwrap();
    assert!(vault.root.join("nested/deep/note.md").exists());
}

// ── Tag operations ──────────────────────────────────────────────────────────

#[test]
fn test_list_tags() {
    let (_tmp, vault) = setup_vault();
    vault.reindex().unwrap();
    let tags = vault.list_tags().unwrap();
    // setup_vault creates docs with tags
    assert!(!tags.is_empty());
}

#[test]
fn test_rename_tag() {
    let (_tmp, vault) = setup_vault();
    // Create a note with a specific tag
    let path = std::path::PathBuf::from("tagged.md");
    vault.save_document_content(&path, "+++\ntitle = \"Tagged\"\ntags = [\"old-tag\"]\n+++\n\nContent.").unwrap();

    let count = vault.rename_tag("old-tag", "new-tag").unwrap();
    assert!(count >= 1);

    // Verify the tag was renamed in the file
    let content = std::fs::read_to_string(vault.root.join("tagged.md")).unwrap();
    assert!(content.contains("new-tag"));
    assert!(!content.contains("old-tag"));
}

#[test]
fn test_rename_tag_nonexistent() {
    let (_tmp, vault) = setup_vault();
    vault.reindex().unwrap();
    let count = vault.rename_tag("nonexistent-tag-xyz", "new-tag").unwrap();
    assert_eq!(count, 0);
}

#[test]
fn test_delete_tag() {
    let (_tmp, vault) = setup_vault();
    let path = std::path::PathBuf::from("to-delete-tag.md");
    vault.save_document_content(&path, "+++\ntitle = \"Del\"\ntags = [\"remove-me\", \"keep\"]\n+++\n\nBody.").unwrap();

    let count = vault.delete_tag("remove-me").unwrap();
    assert!(count >= 1);

    let content = std::fs::read_to_string(vault.root.join("to-delete-tag.md")).unwrap();
    assert!(!content.contains("remove-me"));
    assert!(content.contains("keep"));
}

#[test]
fn test_merge_tags() {
    let (_tmp, vault) = setup_vault();
    let p1 = std::path::PathBuf::from("merge1.md");
    let p2 = std::path::PathBuf::from("merge2.md");
    vault.save_document_content(&p1, "+++\ntitle = \"M1\"\ntags = [\"src1\"]\n+++\n\nBody.").unwrap();
    vault.save_document_content(&p2, "+++\ntitle = \"M2\"\ntags = [\"src2\"]\n+++\n\nBody.").unwrap();

    let count = vault.merge_tags(&["src1", "src2"], "target").unwrap();
    assert!(count >= 2);

    let c1 = std::fs::read_to_string(vault.root.join("merge1.md")).unwrap();
    let c2 = std::fs::read_to_string(vault.root.join("merge2.md")).unwrap();
    assert!(c1.contains("target"));
    assert!(c2.contains("target"));
}

// ── Notifications ───────────────────────────────────────────────────────────

#[test]
fn test_push_and_list_notifications() {
    let (_tmp, vault) = setup_vault();
    let n = Notification::new(NotificationKind::DueDate, "Test", "Body", "test-vault");
    vault.push_notification(&n).unwrap();

    let pending = vault.pending_notifications().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].title, "Test");
}

#[test]
fn test_mark_notification_delivered_clears_pending() {
    let (_tmp, vault) = setup_vault();
    let n = Notification::new(NotificationKind::Decay, "Fading", "Task fading", "vault");
    vault.push_notification(&n).unwrap();
    assert_eq!(vault.pending_notifications().unwrap().len(), 1);

    vault.mark_notification_delivered(&n.id).unwrap();
    assert_eq!(vault.pending_notifications().unwrap().len(), 0);
}

#[test]
fn test_check_task_notifications_decay() {
    let (_tmp, vault) = setup_vault();
    let board = Board::default_sprint("Sprint");
    vault.store.save_board(&board).unwrap();

    // Create a task in the fading range: relevance between 0.1 and 0.3
    // Natural decay (7-day half-life), 14 days old → relevance ≈ 0.25 (fading but not auto-archive)
    let mut task = Task::new(board.id.clone(), "Backlog", "Fading task");
    task.decay = DecayRate::Natural; // 7-day half-life
    task.last_touched_at = Some(Utc::now() - chrono::Duration::days(14));
    task.updated_at = Utc::now() - chrono::Duration::days(14);
    vault.store.save_task(&task).unwrap();

    let notifications = vault.check_task_notifications().unwrap();
    assert!(notifications.iter().any(|n| n.kind == NotificationKind::Decay),
        "expected decay notification for fading task (relevance ~0.25), got: {:?}", notifications);
}

#[test]
fn test_check_task_notifications_skips_done() {
    let (_tmp, vault) = setup_vault();
    let board = Board::default_sprint("Sprint");
    vault.store.save_board(&board).unwrap();

    let mut task = Task::new(board.id.clone(), "Done", "Completed");
    task.status = TaskStatus::Done;
    task.due_date = Some(chrono::Local::now().date_naive());
    vault.store.save_task(&task).unwrap();

    let notifications = vault.check_task_notifications().unwrap();
    assert!(!notifications.iter().any(|n| n.title == "Completed"),
        "should not notify for done tasks");
}

// ── SQLite store edge cases ─────────────────────────────────────────────────

#[test]
fn test_search_documents_empty_query() {
    let (_tmp, vault) = setup_vault();
    vault.reindex().unwrap();
    let results = vault.store.search_documents("").unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_search_documents_finds_match() {
    let (_tmp, vault) = setup_vault();
    // Fixtures have "Welcome", "Projects", "Architecture", etc.
    let results = vault.store.search_documents("Welcome").unwrap();
    assert!(!results.is_empty(), "search for 'Welcome' should find the welcome doc");
}

#[test]
fn test_delete_document_removes_from_store() {
    let (_tmp, vault) = setup_vault();
    vault.reindex().unwrap();
    let docs = vault.store.list_documents().unwrap();
    let first = docs[0].id.clone();
    vault.store.delete_document(&first).unwrap();
    assert!(vault.store.get_document(&first).unwrap().is_none());
}

#[test]
fn test_get_backlinks() {
    let (_tmp, vault) = setup_vault();
    // Welcome links to Projects, so Projects should have Welcome as a backlink
    let docs = vault.store.list_documents().unwrap();
    let projects = docs.iter().find(|d| d.title == "Projects").unwrap();
    let backlinks = vault.store.get_backlinks(&projects.id).unwrap();
    assert!(backlinks.iter().any(|bl| bl.title == "Welcome"),
        "expected Welcome in backlinks of Projects, got: {:?}", backlinks.iter().map(|b| &b.title).collect::<Vec<_>>());
}

// ── create_drawing ──────────────────────────────────────────────────────────

/// Mirror of create_drawing from codex-app (can't import it directly — UI crate)
fn create_drawing(vault_root: &std::path::Path, name: &str) -> anyhow::Result<std::path::PathBuf> {
    let drawings_dir = vault_root.join("drawings");
    std::fs::create_dir_all(&drawings_dir)?;
    let filename = format!("{name}.excalidraw");
    let rel_path = std::path::PathBuf::from("drawings").join(&filename);
    let abs_path = vault_root.join(&rel_path);
    let scene = r#"{"type":"excalidraw","version":2,"elements":[],"appState":{"viewBackgroundColor":"transparent","theme":"dark"}}"#;
    std::fs::write(&abs_path, scene)?;
    Ok(rel_path)
}

#[test]
fn test_create_drawing() {
    let tmp = tempfile::Builder::new().prefix("codex-test-").tempdir().unwrap();
    let root = tmp.path().to_path_buf();

    let path = create_drawing(&root, "Test Drawing").unwrap();
    assert!(path.to_string_lossy().ends_with(".excalidraw"));
    assert!(root.join("drawings/Test Drawing.excalidraw").exists());

    // Content should be valid JSON scene
    let content = std::fs::read_to_string(root.join(&path)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["type"], "excalidraw");
    assert_eq!(parsed["version"], 2);
}

#[test]
fn test_create_drawing_idempotent_dir() {
    let tmp = tempfile::Builder::new().prefix("codex-test-").tempdir().unwrap();
    let root = tmp.path().to_path_buf();

    create_drawing(&root, "First").unwrap();
    create_drawing(&root, "Second").unwrap();

    assert!(root.join("drawings/First.excalidraw").exists());
    assert!(root.join("drawings/Second.excalidraw").exists());
}

// ── Delete document (Move to Trash) ─────────────────────────────────────────

#[test]
fn test_delete_document_removes_file_and_index() {
    let (_tmp, vault) = setup_vault();

    // Get a document that exists
    let docs = vault.store.list_documents().unwrap();
    assert!(!docs.is_empty());
    let target = &docs[0];
    let abs_path = vault.root.join(&target.path);
    assert!(abs_path.exists(), "file should exist before delete");

    // Delete it
    std::fs::remove_file(&abs_path).unwrap();
    vault.store.delete_document(&target.id).unwrap();

    // Verify it's gone from both disk and index
    assert!(!abs_path.exists(), "file should be gone after delete");
    assert!(vault.store.get_document(&target.id).unwrap().is_none(),
        "document should be gone from index after delete");
}

#[test]
fn test_delete_document_does_not_affect_others() {
    let (_tmp, vault) = setup_vault();

    let docs = vault.store.list_documents().unwrap();
    let initial_count = docs.len();
    assert!(initial_count >= 2, "need at least 2 docs for this test");

    let target = &docs[0];
    let other = &docs[1];

    std::fs::remove_file(vault.root.join(&target.path)).unwrap();
    vault.store.delete_document(&target.id).unwrap();

    // Other document should still exist
    assert!(vault.store.get_document(&other.id).unwrap().is_some(),
        "other document should survive the delete");

    let remaining = vault.store.list_documents().unwrap();
    assert_eq!(remaining.len(), initial_count - 1);
}

// ── Vault switching ─────────────────────────────────────────────────────────

#[test]
fn test_switch_vault_opens_different_content() {
    // Create two separate vaults with different content
    let tmp1 = tempfile::Builder::new().prefix("codex-vault1-").tempdir().unwrap();
    let tmp2 = tempfile::Builder::new().prefix("codex-vault2-").tempdir().unwrap();

    let vault1 = Vault::open(tmp1.path()).unwrap();
    vault1.save_document_content(
        &std::path::PathBuf::from("vault1-note.md"),
        "+++\ntitle = \"Vault One Note\"\ntags = []\n+++\n\nContent from vault 1.",
    ).unwrap();
    vault1.reindex().unwrap();

    let vault2 = Vault::open(tmp2.path()).unwrap();
    vault2.save_document_content(
        &std::path::PathBuf::from("vault2-note.md"),
        "+++\ntitle = \"Vault Two Note\"\ntags = []\n+++\n\nContent from vault 2.",
    ).unwrap();
    vault2.reindex().unwrap();

    // Vault 1 should have its doc, not vault 2's
    let docs1 = vault1.store.list_documents().unwrap();
    assert!(docs1.iter().any(|d| d.title == "Vault One Note"));
    assert!(!docs1.iter().any(|d| d.title == "Vault Two Note"));

    // Vault 2 should have its doc, not vault 1's
    let docs2 = vault2.store.list_documents().unwrap();
    assert!(docs2.iter().any(|d| d.title == "Vault Two Note"));
    assert!(!docs2.iter().any(|d| d.title == "Vault One Note"));
}

#[test]
fn test_switch_vault_reindexes_correctly() {
    let tmp = tempfile::Builder::new().prefix("codex-switch-").tempdir().unwrap();
    let vault = Vault::open(tmp.path()).unwrap();

    // Start empty
    let (count, _) = vault.reindex().unwrap();
    assert_eq!(count, 0);

    // Add a file and reindex (simulates what happens on vault switch)
    vault.save_document_content(
        &std::path::PathBuf::from("new.md"),
        "+++\ntitle = \"New\"\ntags = []\n+++\n\nHello.",
    ).unwrap();
    let (count, _) = vault.reindex().unwrap();
    assert_eq!(count, 1);

    let docs = vault.store.list_documents().unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].title, "New");
}

// ── Excalidraw file detection ───────────────────────────────────────────────

#[test]
fn test_excalidraw_files_not_indexed_as_documents() {
    let tmp = tempfile::Builder::new().prefix("codex-test-").tempdir().unwrap();
    let vault = Vault::open(tmp.path()).unwrap();

    // Create a .excalidraw file
    create_drawing(tmp.path(), "Diagram").unwrap();

    // Also create a regular .md file
    vault.save_document_content(
        &std::path::PathBuf::from("note.md"),
        "+++\ntitle = \"Note\"\ntags = []\n+++\n\nA note.",
    ).unwrap();

    vault.reindex().unwrap();

    let docs = vault.store.list_documents().unwrap();
    // Only the .md should be indexed, not the .excalidraw
    assert_eq!(docs.len(), 1, "only .md files should be indexed, got: {:?}",
        docs.iter().map(|d| &d.title).collect::<Vec<_>>());
    assert_eq!(docs[0].title, "Note");
}

// ── Publication system ──────────────────────────────────────────────────────

#[test]
fn test_publication_unlisted_exported_but_marked_correctly() {
    let (_tmp, vault) = setup_vault();
    let output = _tmp.path().join("pub-output");

    // Create an unlisted document
    vault.save_document_content(
        &std::path::PathBuf::from("unlisted-note.md"),
        "+++\ntitle = \"Secret Page\"\ntags = []\n[publication]\nenabled = true\nvisibility = \"unlisted\"\n+++\n\nUnlisted content.",
    ).unwrap();
    vault.reindex().unwrap();

    let report = vault.export_publication_tree(&output).unwrap();
    assert!(report.exported >= 1, "unlisted note should be exported");

    // Check manifest — visibility should be Unlisted, not hardcoded Public
    let manifest_raw = std::fs::read_to_string(output.join("manifest.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest_raw).unwrap();
    let docs = manifest["documents"].as_array().unwrap();
    let secret = docs.iter().find(|d| d["title"] == "Secret Page");
    assert!(secret.is_some(), "Secret Page should be in manifest");
    assert_eq!(secret.unwrap()["visibility"], "unlisted",
        "manifest should reflect actual visibility, not hardcode Public");
}

#[test]
fn test_publication_private_not_exported() {
    let (_tmp, vault) = setup_vault();
    let output = _tmp.path().join("pub-output");

    vault.save_document_content(
        &std::path::PathBuf::from("private-note.md"),
        "+++\ntitle = \"Private\"\ntags = []\n[publication]\nenabled = true\nvisibility = \"private\"\n+++\n\nSecret.",
    ).unwrap();
    vault.reindex().unwrap();

    let report = vault.export_publication_tree(&output).unwrap();
    // Private doc should NOT be exported
    if output.join("manifest.json").exists() {
        let manifest_raw = std::fs::read_to_string(output.join("manifest.json")).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifest_raw).unwrap();
        let docs = manifest["documents"].as_array().unwrap();
        assert!(!docs.iter().any(|d| d["title"] == "Private"),
            "private note should not appear in manifest");
    }
}

#[test]
fn test_publication_policy_rules_tag_match() {
    let (_tmp, vault) = setup_vault();
    let output = _tmp.path().join("pub-output");

    // Set policy: default private, public if tagged "published"
    let mut config = vault.config.clone();
    config.publication.default_visibility = PublicationVisibility::Private;
    config.publication.rules = vec![
        codex_core::models::PublicationRule {
            match_tag: Some("published".into()),
            match_path_prefix: None,
            visibility: PublicationVisibility::Public,
        },
    ];
    vault.save_config(&config).unwrap();

    vault.save_document_content(
        &std::path::PathBuf::from("tagged.md"),
        "+++\ntitle = \"Tagged\"\ntags = [\"published\"]\n[publication]\nenabled = true\n+++\n\nShould be public.",
    ).unwrap();
    vault.save_document_content(
        &std::path::PathBuf::from("untagged.md"),
        "+++\ntitle = \"Untagged\"\ntags = [\"other\"]\n[publication]\nenabled = true\n+++\n\nShould be private.",
    ).unwrap();
    vault.reindex().unwrap();

    // Re-open vault to pick up new config
    let vault = std::sync::Arc::new(codex_store::vault::Vault::open(&vault.root).unwrap());
    vault.reindex().unwrap();
    let report = vault.export_publication_tree(&output).unwrap();

    let manifest_raw = std::fs::read_to_string(output.join("manifest.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest_raw).unwrap();
    let docs = manifest["documents"].as_array().unwrap();
    let titles: Vec<&str> = docs.iter().filter_map(|d| d["title"].as_str()).collect();

    assert!(titles.contains(&"Tagged"), "tagged doc should be exported");
    assert!(!titles.contains(&"Untagged"), "untagged doc should NOT be exported (default private)");
}

#[test]
fn test_publication_policy_rules_path_prefix() {
    let (_tmp, vault) = setup_vault();
    let output = _tmp.path().join("pub-output");

    let mut config = vault.config.clone();
    config.publication.default_visibility = PublicationVisibility::Private;
    config.publication.rules = vec![
        codex_core::models::PublicationRule {
            match_tag: None,
            match_path_prefix: Some("public/".into()),
            visibility: PublicationVisibility::Public,
        },
    ];
    vault.save_config(&config).unwrap();

    std::fs::create_dir_all(vault.root.join("public")).unwrap();
    vault.save_document_content(
        &std::path::PathBuf::from("public/visible.md"),
        "+++\ntitle = \"Visible\"\ntags = []\n[publication]\nenabled = true\n+++\n\nPublic path.",
    ).unwrap();
    vault.save_document_content(
        &std::path::PathBuf::from("hidden.md"),
        "+++\ntitle = \"Hidden\"\ntags = []\n[publication]\nenabled = true\n+++\n\nNot in public path.",
    ).unwrap();
    vault.reindex().unwrap();

    let vault = std::sync::Arc::new(codex_store::vault::Vault::open(&vault.root).unwrap());
    vault.reindex().unwrap();
    let report = vault.export_publication_tree(&output).unwrap();

    let manifest_raw = std::fs::read_to_string(output.join("manifest.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest_raw).unwrap();
    let docs = manifest["documents"].as_array().unwrap();
    let titles: Vec<&str> = docs.iter().filter_map(|d| d["title"].as_str()).collect();

    assert!(titles.contains(&"Visible"), "path-matched doc should be exported");
    assert!(!titles.contains(&"Hidden"), "non-matched doc should NOT be exported");
}

#[test]
fn test_publication_wikilink_to_private_becomes_plain_text() {
    let (_tmp, vault) = setup_vault();
    let output = _tmp.path().join("pub-output");

    vault.save_document_content(
        &std::path::PathBuf::from("public-note.md"),
        "+++\ntitle = \"Public Note\"\ntags = []\n[publication]\nenabled = true\nvisibility = \"public\"\n+++\n\nSee [[Private Note]] for details.",
    ).unwrap();
    vault.save_document_content(
        &std::path::PathBuf::from("private-note.md"),
        "+++\ntitle = \"Private Note\"\ntags = []\n+++\n\nThis is private.",
    ).unwrap();
    vault.reindex().unwrap();

    let report = vault.export_publication_tree(&output).unwrap();
    assert!(report.exported >= 1);

    // The exported public note should NOT have a clickable link to the private note
    let exported_md = std::fs::read_to_string(output.join("public-note.md")).unwrap();
    assert!(!exported_md.contains("[[Private Note]]"), "wikilink to private doc should be rewritten");
    assert!(exported_md.contains("Private Note"), "text should remain, just not as a link");
}

#[test]
fn test_publication_empty_board_exports() {
    let (_tmp, vault) = setup_vault();
    let output = _tmp.path().join("pub-output");

    // Create a project with publication enabled
    std::fs::create_dir_all(vault.root.join("projects")).unwrap();
    vault.save_document_content(
        &std::path::PathBuf::from("projects/test-project.md"),
        "+++\ntitle = \"Test Project\"\nkind = \"project\"\n[publication]\nenabled = true\nvisibility = \"public\"\n[data]\n+++\n\n# Test Project",
    ).unwrap();
    vault.reindex().unwrap();

    // Create a board with no tasks
    let doc = vault.store.list_documents().unwrap();
    let project_doc = doc.iter().find(|d| d.title == "Test Project");
    if let Some(proj) = project_doc {
        if let Ok(Some(full_doc)) = vault.store.get_document(&proj.id) {
            if let Some(id) = full_doc.frontmatter.id {
                let board = Board::for_project("Empty Board", id);
                vault.store.save_board(&board).unwrap();
            }
        }
    }

    let report = vault.export_publication_tree(&output).unwrap();
    // Should not crash even with empty board
    assert!(report.errors.is_empty(), "empty board export should not error: {:?}", report.errors);
}

// ── Sync config validation ──────────────────────────────────────────────────

#[test]
fn test_sync_config_serialization_roundtrip() {
    let config = SyncConfig::Git {
        remote: "origin".into(),
        branch: "main".into(),
        auto_commit_seconds: 60,
    };
    let serialized = toml::to_string(&config).unwrap();
    let deserialized: SyncConfig = toml::from_str(&serialized).unwrap();
    match deserialized {
        SyncConfig::Git { remote, branch, auto_commit_seconds } => {
            assert_eq!(remote, "origin");
            assert_eq!(branch, "main");
            assert_eq!(auto_commit_seconds, 60);
        }
        _ => panic!("expected Git variant"),
    }
}

#[test]
fn test_sync_config_none_roundtrip() {
    let config = SyncConfig::None;
    let serialized = toml::to_string(&config).unwrap();
    let deserialized: SyncConfig = toml::from_str(&serialized).unwrap();
    assert!(matches!(deserialized, SyncConfig::None));
}

#[test]
fn test_sync_config_icloud_roundtrip() {
    let config = SyncConfig::ICloud;
    let serialized = toml::to_string(&config).unwrap();
    let deserialized: SyncConfig = toml::from_str(&serialized).unwrap();
    assert!(matches!(deserialized, SyncConfig::ICloud));
}

#[test]
fn test_sync_config_s3_roundtrip() {
    let config = SyncConfig::S3 {
        bucket: "my-bucket".into(),
        prefix: "vault/".into(),
        region: "us-east-1".into(),
        endpoint: Some("https://s3.example.com".into()),
    };
    let serialized = toml::to_string(&config).unwrap();
    let deserialized: SyncConfig = toml::from_str(&serialized).unwrap();
    match deserialized {
        SyncConfig::S3 { bucket, prefix, region, endpoint } => {
            assert_eq!(bucket, "my-bucket");
            assert_eq!(prefix, "vault/");
            assert_eq!(region, "us-east-1");
            assert_eq!(endpoint, Some("https://s3.example.com".into()));
        }
        _ => panic!("expected S3 variant"),
    }
}
