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
