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
                "tags" => doc.tags.iter().map(|t| format!("<code>{t}</code>")).collect::<Vec<_>>().join(" "),
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
    let start = s.find('"')?;
    let end = s[start + 1..].find('"')?;
    Some(s[start + 1..start + 1 + end].to_string())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
