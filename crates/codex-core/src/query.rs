//! Inline query engine for vault documents.
//!
//! Supports a simple query language in markdown code blocks:
//!
//! ```query
//! TABLE title, tags, updated_at
//! FROM documents
//! WHERE tags CONTAINS "engineering"
//! SORT updated_at DESC
//! LIMIT 10
//! ```
//!
//! Queries run against the VaultStore and return rendered HTML tables/lists.

use crate::models::*;
use crate::store::VaultStore;
use anyhow::Result;

/// Parse and execute a query block, returning rendered HTML.
pub fn execute_query(source: &str, store: &dyn VaultStore) -> Result<String> {
    let lines: Vec<&str> = source.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return Ok("<em>Empty query</em>".into());
    }

    let first = lines[0].to_uppercase();

    if first.starts_with("TABLE") || first.starts_with("LIST") {
        execute_document_query(&lines, store)
    } else if first.starts_with("TASK") {
        execute_task_query(&lines, store)
    } else {
        Ok(format!("<em>Unknown query type: {}</em>", lines[0]))
    }
}

fn execute_document_query(lines: &[&str], store: &dyn VaultStore) -> Result<String> {
    let first = lines[0].to_uppercase();
    let is_table = first.starts_with("TABLE");

    // Parse fields from "TABLE field1, field2, field3" or "LIST field"
    let fields: Vec<String> = if is_table {
        let after = lines[0].get(5..).unwrap_or("").trim();
        if after.is_empty() {
            vec!["title".into(), "tags".into(), "updated_at".into()]
        } else {
            after.split(',').map(|f| f.trim().to_lowercase()).collect()
        }
    } else {
        let after = lines[0].get(4..).unwrap_or("").trim();
        if after.is_empty() { vec!["title".into()] }
        else { vec![after.trim().to_lowercase()] }
    };

    // Parse WHERE, SORT, LIMIT
    let mut tag_filter: Option<String> = None;
    let mut title_filter: Option<String> = None;
    let mut sort_field = "title".to_string();
    let mut sort_desc = false;
    let mut limit: usize = 50;

    for line in &lines[1..] {
        let upper = line.to_uppercase();
        if upper.starts_with("WHERE") {
            let clause = line.get(5..).unwrap_or("").trim();
            let clause_upper = clause.to_uppercase();
            if clause_upper.contains("TAGS CONTAINS") || clause_upper.contains("TAG CONTAINS") {
                if let Some(val) = extract_quoted(clause) {
                    tag_filter = Some(val);
                }
            } else if clause_upper.contains("TITLE CONTAINS") {
                if let Some(val) = extract_quoted(clause) {
                    title_filter = Some(val.to_lowercase());
                }
            }
        } else if upper.starts_with("SORT") {
            let parts: Vec<&str> = line.get(4..).unwrap_or("").trim().split_whitespace().collect();
            if let Some(f) = parts.first() {
                sort_field = f.to_lowercase();
            }
            if parts.get(1).map(|s| s.to_uppercase()) == Some("DESC".into()) {
                sort_desc = true;
            }
        } else if upper.starts_with("LIMIT") {
            if let Some(n) = line.get(5..).unwrap_or("").trim().parse::<usize>().ok() {
                limit = n;
            }
        }
    }

    // Fetch documents
    let mut docs = store.list_documents()?;

    // Filter
    if let Some(ref tag) = tag_filter {
        docs.retain(|d| d.tags.iter().any(|t| t.to_lowercase() == tag.to_lowercase()));
    }
    if let Some(ref search) = title_filter {
        docs.retain(|d| d.title.to_lowercase().contains(search));
    }

    // Sort
    match sort_field.as_str() {
        "updated_at" | "updated" | "date" => {
            docs.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
            if sort_desc { docs.reverse(); }
        }
        "title" | "name" => {
            docs.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
            if sort_desc { docs.reverse(); }
        }
        _ => {}
    }

    // Limit
    docs.truncate(limit);

    // Render
    if is_table {
        render_table(&docs, &fields)
    } else {
        render_list(&docs, &fields)
    }
}

fn execute_task_query(lines: &[&str], store: &dyn VaultStore) -> Result<String> {
    let mut tasks = store.list_tasks(&crate::store::TaskFilter::default())?;

    // Parse filters
    for line in &lines[1..] {
        let upper = line.to_uppercase();
        if upper.contains("STATUS") {
            if upper.contains("TODO") {
                tasks.retain(|t| t.status == TaskStatus::Todo);
            } else if upper.contains("INPROGRESS") || upper.contains("IN_PROGRESS") {
                tasks.retain(|t| t.status == TaskStatus::InProgress);
            } else if upper.contains("DONE") {
                tasks.retain(|t| t.status == TaskStatus::Done);
            }
        }
        if upper.contains("NOT ARCHIVED") {
            tasks.retain(|t| t.status != TaskStatus::Archived);
        }
        if upper.starts_with("WHERE") && upper.contains("PRIORITY") {
            if let Some(val) = extract_quoted(line) {
                match val.to_lowercase().as_str() {
                    "high" => tasks.retain(|t| t.priority == Priority::High),
                    "critical" => tasks.retain(|t| t.priority == Priority::Critical),
                    "low" => tasks.retain(|t| t.priority == Priority::Low),
                    _ => {}
                }
            }
        }
    }

    // Render as checklist
    let mut html = String::from("<ul class=\"query-tasks\">\n");
    for task in &tasks {
        let checked = if task.status == TaskStatus::Done { " checked" } else { "" };
        let priority_class = match task.priority {
            Priority::Critical => " critical",
            Priority::High => " high",
            _ => "",
        };
        html.push_str(&format!(
            "<li class=\"query-task{priority_class}\"><input type=\"checkbox\" disabled{checked}/> <strong>{}</strong> <span class=\"query-task-col\">{}</span></li>\n",
            html_escape(&task.title),
            html_escape(&task.column),
        ));
    }
    html.push_str("</ul>");
    Ok(html)
}

fn render_table(docs: &[DocumentMeta], fields: &[String]) -> Result<String> {
    let mut html = String::from("<table class=\"query-table\"><thead><tr>");
    for f in fields {
        html.push_str(&format!("<th>{}</th>", html_escape(f)));
    }
    html.push_str("</tr></thead><tbody>");

    for doc in docs {
        html.push_str("<tr>");
        for f in fields {
            let val = match f.as_str() {
                "title" | "name" => format!("<a href=\"codex-note://{}\">[[{}]]</a>", doc.id.0, html_escape(&doc.title)),
                "tags" => doc.tags.iter().map(|t| format!("<code>{}</code>", html_escape(t))).collect::<Vec<_>>().join(" "),
                "path" => html_escape(&doc.path.display().to_string()),
                "updated_at" | "updated" | "date" => doc.updated_at.format("%Y-%m-%d").to_string(),
                _ => String::new(),
            };
            html.push_str(&format!("<td>{val}</td>"));
        }
        html.push_str("</tr>");
    }

    html.push_str("</tbody></table>");
    Ok(html)
}

fn render_list(docs: &[DocumentMeta], fields: &[String]) -> Result<String> {
    let mut html = String::from("<ul class=\"query-list\">");
    for doc in docs {
        let field = fields.first().map(|s| s.as_str()).unwrap_or("title");
        let val = match field {
            "title" | "name" => format!("[[{}]]", html_escape(&doc.title)),
            "path" => html_escape(&doc.path.display().to_string()),
            _ => html_escape(&doc.title),
        };
        html.push_str(&format!("<li>{val}</li>"));
    }
    html.push_str("</ul>");
    Ok(html)
}

fn extract_quoted(s: &str) -> Option<String> {
    // Find the first properly quoted value — no embedded quotes allowed
    let start = s.find('"')? + 1;
    let rest = &s[start..];
    let end = rest.find('"')?;
    let val = &rest[..end];
    // Reject values containing control characters or additional quotes
    if val.contains('"') || val.contains('\\') { return None; }
    Some(val.to_string())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datum::EntityKind;
    use crate::store::{DocumentMetadataFilter, TaskFilter};
    use std::path::PathBuf;
    use std::collections::BTreeMap;
    use chrono::{DateTime, Utc};

    struct MockStore {
        docs: Vec<DocumentMeta>,
        tasks: Vec<Task>,
    }

    impl VaultStore for MockStore {
        fn get_document(&self, _id: &DocumentId) -> Result<Option<Document>> { Ok(None) }
        fn get_document_by_path(&self, _path: &std::path::Path) -> Result<Option<Document>> { Ok(None) }
        fn find_document_by_slug(&self, _slug: &str) -> Result<Option<DocumentMeta>> { Ok(None) }
        fn list_documents(&self) -> Result<Vec<DocumentMeta>> { Ok(self.docs.clone()) }
        fn list_documents_by_metadata(&self, _filter: &DocumentMetadataFilter) -> Result<Vec<DocumentMeta>> { Ok(vec![]) }
        fn save_document(&self, _doc: &Document) -> Result<()> { Ok(()) }
        fn delete_document(&self, _id: &DocumentId) -> Result<()> { Ok(()) }
        fn search_documents(&self, _query: &str) -> Result<Vec<crate::models::SearchResult>> { Ok(vec![]) }
        fn get_backlinks(&self, _id: &DocumentId) -> Result<Vec<DocumentMeta>> { Ok(vec![]) }
        fn list_entities_by_kind(&self, _kind: &EntityKind) -> Result<Vec<DocumentMeta>> { Ok(vec![]) }
        fn get_task(&self, _id: &TaskId) -> Result<Option<Task>> { Ok(None) }
        fn list_tasks(&self, _filter: &TaskFilter) -> Result<Vec<Task>> { Ok(self.tasks.clone()) }
        fn save_task(&self, _task: &Task) -> Result<()> { Ok(()) }
        fn delete_task(&self, _id: &TaskId) -> Result<()> { Ok(()) }
        fn get_board(&self, _id: &BoardId) -> Result<Option<Board>> { Ok(None) }
        fn list_boards(&self) -> Result<Vec<Board>> { Ok(vec![]) }
        fn save_board(&self, _board: &Board) -> Result<()> { Ok(()) }
        fn list_dirty_tasks(&self, _pid: &uuid::Uuid) -> Result<Vec<Task>> { Ok(vec![]) }
        fn list_dirty_documents(&self, _pid: &uuid::Uuid) -> Result<Vec<Document>> { Ok(vec![]) }
        fn mark_committed(&self, _t: &[TaskId], _d: &[DocumentId], _at: DateTime<Utc>) -> Result<()> { Ok(()) }
        fn record_project_deletion(&self, _eid: &uuid::Uuid, _kind: &str, _pid: &uuid::Uuid) -> Result<()> { Ok(()) }
        fn list_pending_deletions(&self, _pid: &uuid::Uuid) -> Result<Vec<(uuid::Uuid, String)>> { Ok(vec![]) }
        fn mark_deletions_committed(&self, _eids: &[uuid::Uuid]) -> Result<()> { Ok(()) }
    }

    fn doc(title: &str, tags: &[&str]) -> DocumentMeta {
        DocumentMeta {
            id: DocumentId::new(),
            path: PathBuf::from(format!("{title}.md")),
            title: title.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            metadata: BTreeMap::new(),
            entity_kind: None,
            updated_at: Utc::now(),
        }
    }

    fn task(title: &str, column: &str, status: TaskStatus, priority: Priority) -> Task {
        let bid = BoardId(uuid::Uuid::new_v4());
        let mut t = Task::new(bid, column, title);
        t.status = status;
        t.priority = priority;
        t
    }

    #[test]
    fn empty_query_returns_message() {
        let store = MockStore { docs: vec![], tasks: vec![] };
        let result = execute_query("", &store).unwrap();
        assert!(result.contains("Empty query"));
    }

    #[test]
    fn unknown_query_type() {
        let store = MockStore { docs: vec![], tasks: vec![] };
        let result = execute_query("FOOBAR stuff", &store).unwrap();
        assert!(result.contains("Unknown query type"));
    }

    #[test]
    fn table_default_fields() {
        let store = MockStore { docs: vec![doc("Alpha", &["tag1"]), doc("Beta", &[])], tasks: vec![] };
        let html = execute_query("TABLE", &store).unwrap();
        assert!(html.contains("<table"));
        assert!(html.contains("Alpha"));
        assert!(html.contains("Beta"));
    }

    #[test]
    fn table_with_custom_fields() {
        let store = MockStore { docs: vec![doc("Alpha", &["x"])], tasks: vec![] };
        let html = execute_query("TABLE title, path", &store).unwrap();
        assert!(html.contains("Alpha"));
        assert!(html.contains("Alpha.md"));
    }

    #[test]
    fn table_where_tags_filter() {
        let store = MockStore {
            docs: vec![doc("Match", &["engineering"]), doc("Skip", &["other"])],
            tasks: vec![],
        };
        let html = execute_query("TABLE title\nWHERE tags CONTAINS \"engineering\"", &store).unwrap();
        assert!(html.contains("Match"));
        assert!(!html.contains("Skip"));
    }

    #[test]
    fn table_where_title_filter() {
        let store = MockStore { docs: vec![doc("Hello World", &[]), doc("Goodbye", &[])], tasks: vec![] };
        let html = execute_query("TABLE title\nWHERE title CONTAINS \"hello\"", &store).unwrap();
        assert!(html.contains("Hello World"));
        assert!(!html.contains("Goodbye"));
    }

    #[test]
    fn table_sort_title_desc() {
        let store = MockStore { docs: vec![doc("Alpha", &[]), doc("Zeta", &[])], tasks: vec![] };
        let html = execute_query("TABLE title\nSORT title DESC", &store).unwrap();
        let zeta_pos = html.find("Zeta").unwrap();
        let alpha_pos = html.find("Alpha").unwrap();
        assert!(zeta_pos < alpha_pos, "Zeta should come before Alpha in DESC");
    }

    #[test]
    fn table_limit() {
        let store = MockStore { docs: vec![doc("A", &[]), doc("B", &[]), doc("C", &[])], tasks: vec![] };
        let html = execute_query("TABLE title\nLIMIT 2", &store).unwrap();
        let row_count = html.matches("<tr>").count() - 1;
        assert_eq!(row_count, 2);
    }

    #[test]
    fn list_query() {
        let store = MockStore { docs: vec![doc("Note One", &[])], tasks: vec![] };
        let html = execute_query("LIST", &store).unwrap();
        assert!(html.contains("<ul"));
        assert!(html.contains("Note One"));
    }

    #[test]
    fn task_query_renders_checklist() {
        let store = MockStore {
            docs: vec![],
            tasks: vec![task("Buy milk", "Backlog", TaskStatus::Todo, Priority::Medium)],
        };
        let html = execute_query("TASK", &store).unwrap();
        assert!(html.contains("<input type=\"checkbox\""));
        assert!(html.contains("Buy milk"));
    }

    #[test]
    fn task_query_done_is_checked() {
        let store = MockStore {
            docs: vec![],
            tasks: vec![task("Done task", "Done", TaskStatus::Done, Priority::Medium)],
        };
        let html = execute_query("TASK", &store).unwrap();
        assert!(html.contains("checked"));
    }

    #[test]
    fn task_query_filter_status() {
        let store = MockStore {
            docs: vec![],
            tasks: vec![
                task("Todo", "Backlog", TaskStatus::Todo, Priority::Medium),
                task("Done", "Done", TaskStatus::Done, Priority::Medium),
            ],
        };
        let html = execute_query("TASK\nWHERE STATUS = TODO", &store).unwrap();
        assert!(html.contains("Todo"));
        assert!(!html.contains(">Done<"));
    }

    #[test]
    fn task_query_filter_priority() {
        let store = MockStore {
            docs: vec![],
            tasks: vec![
                task("High", "Backlog", TaskStatus::Todo, Priority::High),
                task("Low", "Backlog", TaskStatus::Todo, Priority::Low),
            ],
        };
        let html = execute_query("TASK\nWHERE priority = \"high\"", &store).unwrap();
        assert!(html.contains("High"));
        assert!(!html.contains("Low"));
    }

    #[test]
    fn html_escape_special_chars() {
        assert_eq!(html_escape("<script>&"), "&lt;script&gt;&amp;");
    }

    #[test]
    fn extract_quoted_basic() {
        assert_eq!(extract_quoted("tags CONTAINS \"hello\""), Some("hello".into()));
    }

    #[test]
    fn extract_quoted_no_quotes() {
        assert_eq!(extract_quoted("no quotes here"), None);
    }

    #[test]
    fn extract_quoted_empty_value() {
        assert_eq!(extract_quoted("value = \"\""), Some("".into()));
    }

    #[test]
    fn table_empty_store() {
        let store = MockStore { docs: vec![], tasks: vec![] };
        let html = execute_query("TABLE title", &store).unwrap();
        assert!(html.contains("<table"));
        assert!(html.contains("</table>"));
        // No data rows
        assert_eq!(html.matches("<tr>").count(), 1); // header only
    }

    #[test]
    fn task_query_empty_store() {
        let store = MockStore { docs: vec![], tasks: vec![] };
        let html = execute_query("TASK", &store).unwrap();
        assert!(html.contains("<ul"));
        assert!(!html.contains("<li"));
    }
}
