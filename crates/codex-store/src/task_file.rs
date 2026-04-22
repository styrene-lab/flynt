//! Serialize and parse kanban tasks as markdown entity files.
//!
//! Tasks belonging to git-backed projects are stored on disk as markdown files
//! with TOML frontmatter (`kind = "task"`). This module handles the conversion
//! between the runtime `Task` struct and its canonical markdown representation.

use anyhow::{Context, Result};
use chrono::Utc;
use codex_core::{
    models::{BoardId, DocumentId, Priority, Task, TaskId, TaskStatus},
    parser::parse_document_source,
};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Serialize a `Task` to its canonical markdown file content.
pub fn serialize_task_to_markdown(task: &Task, project_id: &Uuid) -> String {
    let mut fm = String::new();
    fm.push_str("+++\n");
    fm.push_str(&format!("id = \"{}\"\n", task.id.0));
    fm.push_str("kind = \"task\"\n\n");
    fm.push_str("[data]\n");
    fm.push_str(&format!("title = {}\n", toml_quote(&task.title)));
    fm.push_str(&format!("project = \"{project_id}\"\n"));
    fm.push_str(&format!("board = \"{}\"\n", task.board_id.0));
    fm.push_str(&format!("column = {}\n", toml_quote(&task.column)));
    fm.push_str(&format!("priority = {}\n", priority_to_int(&task.priority)));
    fm.push_str(&format!("status = {}\n", toml_quote(task_status_str(&task.status))));
    fm.push_str(&format!("position = {}\n", task.position));

    if !task.tags.is_empty() {
        let tags: Vec<String> = task.tags.iter().map(|t| toml_quote(t)).collect();
        fm.push_str(&format!("tags = [{}]\n", tags.join(", ")));
    }

    if !task.document_refs.is_empty() {
        let refs: Vec<String> = task.document_refs.iter().map(|r| format!("\"{}\"", r.0)).collect();
        fm.push_str(&format!("document_refs = [{}]\n", refs.join(", ")));
    }

    if let Some(due) = &task.due_date {
        fm.push_str(&format!("due_date = \"{due}\"\n"));
    }

    fm.push_str("+++\n");

    let body = task.description.trim();
    if body.is_empty() {
        fm
    } else {
        format!("{fm}\n{body}\n")
    }
}

/// Parse a markdown file into a `Task`.
///
/// The file must have TOML frontmatter with `kind = "task"` and fields under `[data]`.
pub fn parse_task_from_markdown(raw: &str) -> Result<Task> {
    let (body, _frontmatter, _links) = parse_document_source(raw);

    // Parse the raw frontmatter TOML directly for entity fields
    let fm_toml = extract_raw_frontmatter(raw)
        .context("task file missing TOML frontmatter")?;
    let val: toml::Value = toml::from_str(&fm_toml)
        .context("invalid TOML frontmatter in task file")?;
    let table = val.as_table()
        .context("frontmatter is not a TOML table")?;

    let kind = table.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    if kind != "task" {
        anyhow::bail!("expected kind = \"task\", got kind = \"{kind}\"");
    }

    let id = table.get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .map(TaskId)
        .unwrap_or_else(TaskId::new);

    let data = table.get("data").and_then(|v| v.as_table());

    let get_str = |key: &str| -> Option<String> {
        data.and_then(|d| d.get(key)).and_then(|v| v.as_str()).map(String::from)
    };
    let get_int = |key: &str| -> Option<i64> {
        data.and_then(|d| d.get(key)).and_then(|v| v.as_integer())
    };

    let board_id = get_str("board")
        .and_then(|s| Uuid::parse_str(&s).ok())
        .map(BoardId)
        .unwrap_or_else(BoardId::new);

    let now = Utc::now();
    Ok(Task {
        id,
        board_id,
        column: get_str("column").unwrap_or_else(|| "Backlog".into()),
        title: get_str("title").unwrap_or_else(|| "Untitled".into()),
        description: body,
        priority: get_int("priority")
            .map(int_to_priority)
            .unwrap_or_default(),
        status: get_str("status")
            .as_deref()
            .map(str_to_task_status)
            .unwrap_or_default(),
        tags: data
            .and_then(|d| d.get("tags"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        document_refs: data
            .and_then(|d| d.get("document_refs"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| Uuid::parse_str(s).ok())
                    .map(DocumentId)
                    .collect()
            })
            .unwrap_or_default(),
        due_date: get_str("due_date")
            .and_then(|s| s.parse().ok()),
        position: get_int("position").unwrap_or(0) as u32,
        created_at: now,
        updated_at: now,
        decay: get_str("decay")
            .and_then(|s| serde_json::from_str(&format!("\"{s}\"")).ok())
            .unwrap_or_default(),
        last_touched_at: None,
        external_refs: data
            .and_then(|d| d.get("external_refs"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        design_node_id: get_str("design_node")
            .and_then(|s| Uuid::parse_str(&s).ok()),
    })
}

/// Return the relative file path for a task within a project sub-path.
pub fn task_file_path(sub_path: &Path, task_id: &TaskId) -> PathBuf {
    sub_path.join("tasks").join(format!("{}.md", task_id.0))
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn extract_raw_frontmatter(raw: &str) -> Option<String> {
    let trimmed = raw.trim_start();
    if let Some(rest) = trimmed.strip_prefix("+++") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        if let Some(end) = rest.find("\n+++") {
            return Some(rest[..end].to_string());
        }
    }
    None
}

fn toml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn priority_to_int(p: &Priority) -> i64 {
    match p {
        Priority::Low => 1,
        Priority::Medium => 2,
        Priority::High => 3,
        Priority::Critical => 4,
    }
}

fn int_to_priority(n: i64) -> Priority {
    match n {
        1 => Priority::Low,
        3 => Priority::High,
        4 => Priority::Critical,
        _ => Priority::Medium,
    }
}

fn task_status_str(s: &TaskStatus) -> &'static str {
    match s {
        TaskStatus::Todo => "todo",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Done => "done",
        TaskStatus::Archived => "archived",
    }
}

fn str_to_task_status(s: &str) -> TaskStatus {
    match s {
        "todo" => TaskStatus::Todo,
        "in_progress" => TaskStatus::InProgress,
        "done" => TaskStatus::Done,
        "archived" => TaskStatus::Archived,
        _ => TaskStatus::Todo,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn sample_task(project_id: &Uuid) -> Task {
        let board_id = BoardId(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap());
        let mut task = Task::new(board_id, "In Progress", "Fix the parser");
        task.description = "Detailed description with [[wikilinks]].".into();
        task.priority = Priority::High;
        task.status = TaskStatus::InProgress;
        task.tags = vec!["bug".into(), "parser".into()];
        task.due_date = Some(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap());
        task.position = 3;
        let _ = project_id; // used by caller
        task
    }

    #[test]
    fn roundtrip_task_to_markdown_and_back() {
        let project_id = Uuid::new_v4();
        let task = sample_task(&project_id);
        let original_id = task.id.clone();

        let md = serialize_task_to_markdown(&task, &project_id);
        assert!(md.contains("kind = \"task\""));
        assert!(md.contains("title = \"Fix the parser\""));
        assert!(md.contains("column = \"In Progress\""));
        assert!(md.contains("priority = 3"));
        assert!(md.contains("status = \"in_progress\""));
        assert!(md.contains("Detailed description with [[wikilinks]]."));

        let parsed = parse_task_from_markdown(&md).unwrap();
        assert_eq!(parsed.id, original_id);
        assert_eq!(parsed.title, "Fix the parser");
        assert_eq!(parsed.column, "In Progress");
        assert_eq!(parsed.priority, Priority::High);
        assert_eq!(parsed.status, TaskStatus::InProgress);
        assert_eq!(parsed.tags, vec!["bug", "parser"]);
        assert_eq!(parsed.due_date, Some(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()));
        assert_eq!(parsed.position, 3);
        assert!(parsed.description.contains("Detailed description"));
    }

    #[test]
    fn serialize_empty_description() {
        let project_id = Uuid::new_v4();
        let mut task = sample_task(&project_id);
        task.description = String::new();

        let md = serialize_task_to_markdown(&task, &project_id);
        // Should end with the closing +++ without extra body
        assert!(md.trim_end().ends_with("+++"));
    }

    #[test]
    fn parse_rejects_non_task_kind() {
        let md = "+++\nid = \"550e8400-e29b-41d4-a716-446655440000\"\nkind = \"project\"\n+++\n\nBody\n";
        let err = parse_task_from_markdown(md).unwrap_err();
        assert!(err.to_string().contains("expected kind = \"task\""));
    }

    #[test]
    fn task_file_path_construction() {
        let task_id = TaskId(Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap());
        let path = task_file_path(Path::new(".codex/projects/myproj"), &task_id);
        assert_eq!(
            path,
            PathBuf::from(".codex/projects/myproj/tasks/550e8400-e29b-41d4-a716-446655440000.md")
        );
    }

    #[test]
    fn roundtrip_with_document_refs() {
        let project_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let mut task = sample_task(&project_id);
        task.document_refs = vec![DocumentId(doc_id)];

        let md = serialize_task_to_markdown(&task, &project_id);
        let parsed = parse_task_from_markdown(&md).unwrap();
        assert_eq!(parsed.document_refs.len(), 1);
        assert_eq!(parsed.document_refs[0].0, doc_id);
    }
}
