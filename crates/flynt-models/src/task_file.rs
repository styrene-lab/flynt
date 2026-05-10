//! Serialize and parse kanban tasks as markdown entity files.
//!
//! Single source of truth for the task file format. Both flynt-store and
//! scribe consume these functions. No heavy dependencies (no comrak).

use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

use crate::task::{BoardId, DecayRate, DocumentId, Priority, Task, TaskId, TaskStatus};

/// Serialize a `Task` to its canonical markdown file content.
pub fn serialize_task_to_markdown(task: &Task) -> String {
    let mut fm = String::new();
    fm.push_str("+++\n");
    fm.push_str(&format!("id = \"{}\"\n", task.id.0));
    fm.push_str("kind = \"task\"\n\n");
    fm.push_str("[data]\n");
    fm.push_str(&format!("title = {}\n", toml_quote(&task.title)));
    fm.push_str(&format!("board = \"{}\"\n", task.board_id.0));
    fm.push_str(&format!("column = {}\n", toml_quote(&task.column)));
    fm.push_str(&format!("priority = {}\n", priority_to_int(&task.priority)));
    fm.push_str(&format!(
        "status = {}\n",
        toml_quote(task_status_str(&task.status))
    ));
    fm.push_str(&format!("position = {}\n", task.position));

    if !task.tags.is_empty() {
        let tags: Vec<String> = task.tags.iter().map(|t| toml_quote(t)).collect();
        fm.push_str(&format!("tags = [{}]\n", tags.join(", ")));
    }

    if !task.document_refs.is_empty() {
        let refs: Vec<String> = task
            .document_refs
            .iter()
            .map(|r| format!("\"{}\"", r.0))
            .collect();
        fm.push_str(&format!("document_refs = [{}]\n", refs.join(", ")));
    }

    if !task.external_refs.is_empty() {
        let refs: Vec<String> = task.external_refs.iter().map(|r| toml_quote(r)).collect();
        fm.push_str(&format!("external_refs = [{}]\n", refs.join(", ")));
    }

    if let Some(due) = &task.due_date {
        fm.push_str(&format!("due_date = \"{due}\"\n"));
    }

    if let Some(node) = task.design_node_id {
        fm.push_str(&format!("design_node = \"{node}\"\n"));
    }

    if let Some(change) = &task.openspec_change {
        fm.push_str(&format!("openspec_change = {}\n", toml_quote(change)));
    }

    if let Some(eng) = &task.engagement_id {
        fm.push_str(&format!("engagement = \"{}\"\n", eng.0));
    }

    // Execution block — nested table under [data.execution]. Hand-format
    // each field. We can't use toml::to_string here: it emits BTreeMap<String,
    // String> as a separate sub-table header, which after our [data.execution]
    // header becomes a SIBLING table at top-level scope, not a child of
    // execution. Net effect: env vars round-trip-deserialize to empty.
    // Inline tables (`env = { K = "V" }`) keep env scoped correctly.
    if let Some(exec) = task.execution.as_ref() {
        if !exec.is_empty() {
            fm.push_str("\n[data.execution]\n");
            if let Some(v) = &exec.model {
                fm.push_str(&format!("model = {}\n", toml_quote(v)));
            }
            if let Some(v) = &exec.skill {
                fm.push_str(&format!("skill = {}\n", toml_quote(v)));
            }
            if let Some(v) = exec.max_turns {
                fm.push_str(&format!("max_turns = {v}\n"));
            }
            if let Some(v) = exec.timeout_secs {
                fm.push_str(&format!("timeout_secs = {v}\n"));
            }
            if let Some(v) = exec.token_budget {
                fm.push_str(&format!("token_budget = {v}\n"));
            }
            if let Some(v) = &exec.cwd {
                fm.push_str(&format!("cwd = {}\n", toml_quote(&v.to_string_lossy())));
            }
            if !exec.env.is_empty() {
                let pairs: Vec<String> = exec.env.iter()
                    .map(|(k, v)| format!("{k} = {}", toml_quote(v)))
                    .collect();
                fm.push_str(&format!("env = {{ {} }}\n", pairs.join(", ")));
            }
        }
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
pub fn parse_task_from_markdown(raw: &str) -> Result<Task> {
    let body = extract_body(raw);

    let fm_toml =
        extract_raw_frontmatter(raw).context("task file missing TOML frontmatter")?;
    let val: toml::Value =
        toml::from_str(&fm_toml).context("invalid TOML frontmatter in task file")?;
    let table = val.as_table().context("frontmatter is not a TOML table")?;

    let kind = table.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    if kind != "task" {
        anyhow::bail!("expected kind = \"task\", got kind = \"{kind}\"");
    }

    let id = table
        .get("id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .map(TaskId)
        .unwrap_or_else(TaskId::new);

    let data = table.get("data").and_then(|v| v.as_table());

    let get_str = |key: &str| -> Option<String> {
        data.and_then(|d| d.get(key))
            .and_then(|v| v.as_str())
            .map(String::from)
    };
    let get_int = |key: &str| -> Option<i64> {
        data.and_then(|d| d.get(key))
            .and_then(|v| v.as_integer())
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
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
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
        external_refs: data
            .and_then(|d| d.get("external_refs"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        due_date: get_str("due_date").and_then(|s| s.parse().ok()),
        position: get_int("position").unwrap_or(0) as u32,
        created_at: now,
        updated_at: now,
        decay: get_str("decay")
            .map(|s| str_to_decay_rate(&s))
            .unwrap_or_default(),
        last_touched_at: None,
        design_node_id: get_str("design_node").and_then(|s| Uuid::parse_str(&s).ok()),
        // openspec_change: bare string from `[data]` (not nested). Sentry's
        // lifecycle integration matches changes by name, so we don't try to
        // canonicalize — just round-trip the string.
        openspec_change: get_str("openspec_change"),
        engagement_id: get_str("engagement")
            .and_then(|s| Uuid::parse_str(&s).ok())
            .map(crate::engagement::EngagementId),
        // execution: nested `[data.execution]` table. Parsed via toml::Value
        // → typed ExecutionSpec. Absent table = None; empty table also = None
        // (no point persisting an empty execution block).
        execution: data
            .and_then(|d| d.get("execution"))
            .and_then(|v| v.as_table())
            .and_then(|t| {
                let value = toml::Value::Table(t.clone());
                let spec: crate::task::ExecutionSpec = value.try_into().ok()?;
                if spec.is_empty() { None } else { Some(spec) }
            }),
    })
}

// ── Public helpers ──────────────────────────────────────────────────────────

pub fn toml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn extract_raw_frontmatter(raw: &str) -> Option<String> {
    let trimmed = raw.trim_start();
    if let Some(rest) = trimmed.strip_prefix("+++") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        if let Some(end) = rest.find("\n+++") {
            return Some(rest[..end].to_string());
        }
    }
    None
}

pub fn extract_body(raw: &str) -> String {
    let trimmed = raw.trim_start();
    if let Some(rest) = trimmed.strip_prefix("+++") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        if let Some(end) = rest.find("\n+++") {
            let after = &rest[end + 4..];
            let after = after.strip_prefix('\n').unwrap_or(after);
            return after.trim().to_string();
        }
    }
    String::new()
}

pub fn priority_to_int(p: &Priority) -> i64 {
    match p { Priority::Low => 1, Priority::Medium => 2, Priority::High => 3, Priority::Critical => 4 }
}

pub fn int_to_priority(n: i64) -> Priority {
    match n { 1 => Priority::Low, 3 => Priority::High, 4 => Priority::Critical, _ => Priority::Medium }
}

pub fn task_status_str(s: &TaskStatus) -> &'static str {
    match s { TaskStatus::Todo => "todo", TaskStatus::InProgress => "in_progress", TaskStatus::Done => "done", TaskStatus::Archived => "archived" }
}

pub fn str_to_task_status(s: &str) -> TaskStatus {
    match s { "todo" => TaskStatus::Todo, "in_progress" => TaskStatus::InProgress, "done" => TaskStatus::Done, "archived" => TaskStatus::Archived, _ => TaskStatus::Todo }
}

pub fn str_to_decay_rate(s: &str) -> DecayRate {
    match s {
        "none" => DecayRate::None,
        "slow" => DecayRate::Slow,
        "natural" => DecayRate::Natural,
        "fast" => DecayRate::Fast,
        other => other.parse::<f64>().map(DecayRate::Custom).unwrap_or_default(),
    }
}

pub fn column_to_status(column: &str) -> &'static str {
    match column.to_lowercase().as_str() {
        "done" | "closed" | "archived" => "done",
        "in progress" | "doing" | "active" => "in_progress",
        _ => "todo",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn sample_task() -> Task {
        let board_id = BoardId(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap());
        let mut task = Task::new(board_id, "In Progress", "Fix the parser");
        task.description = "Detailed description with [[wikilinks]].".into();
        task.priority = Priority::High;
        task.status = TaskStatus::InProgress;
        task.tags = vec!["bug".into(), "parser".into()];
        task.due_date = Some(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap());
        task.position = 3;
        task
    }

    #[test]
    fn roundtrip() {
        let task = sample_task();
        let original_id = task.id.clone();

        let md = serialize_task_to_markdown(&task);
        let parsed = parse_task_from_markdown(&md).unwrap();

        assert_eq!(parsed.id, original_id);
        assert_eq!(parsed.title, "Fix the parser");
        assert_eq!(parsed.column, "In Progress");
        assert_eq!(parsed.priority, Priority::High);
        assert_eq!(parsed.status, TaskStatus::InProgress);
        assert_eq!(parsed.tags, vec!["bug", "parser"]);
    }

    #[test]
    fn roundtrip_external_refs() {
        let mut task = sample_task();
        task.external_refs = vec!["https://github.com/org/repo/issues/42".into()];

        let md = serialize_task_to_markdown(&task);
        let parsed = parse_task_from_markdown(&md).unwrap();
        assert_eq!(parsed.external_refs, vec!["https://github.com/org/repo/issues/42"]);
    }

    #[test]
    fn parse_rejects_non_task() {
        let md = "+++\nkind = \"project\"\n+++\n";
        assert!(parse_task_from_markdown(md).is_err());
    }

    // ── execution + openspec_change round-trips ────────────────────────────
    //
    // Adversarial concern: toml::to_string of an ExecutionSpec with a
    // non-empty BTreeMap<String, String> (env vars) emits a sub-table header
    // like `[env]` which, when concatenated under our `[data.execution]`
    // section, would silently break the TOML parse. Verify both paths.

    #[test]
    fn roundtrip_execution_minimal() {
        let mut task = sample_task();
        task.execution = Some(crate::task::ExecutionSpec {
            model: Some("anthropic:claude-sonnet-4-6".into()),
            max_turns: Some(20),
            ..Default::default()
        });

        let md = serialize_task_to_markdown(&task);
        let parsed = parse_task_from_markdown(&md).unwrap();
        let exec = parsed.execution.expect("execution should round-trip");
        assert_eq!(exec.model.as_deref(), Some("anthropic:claude-sonnet-4-6"));
        assert_eq!(exec.max_turns, Some(20));
    }

    #[test]
    fn roundtrip_execution_with_env_vars() {
        // env: BTreeMap<String, String> serializes to a TOML sub-table.
        // If our [data.execution] section is followed by an [env] block at
        // the wrong scope, this round-trip silently drops env entries.
        let mut task = sample_task();
        let mut env = std::collections::BTreeMap::new();
        env.insert("SCAN_DEPTH".to_string(), "deep".to_string());
        env.insert("API_TOKEN".to_string(), "xyz".to_string());
        task.execution = Some(crate::task::ExecutionSpec {
            model: Some("x".into()),
            env,
            ..Default::default()
        });

        let md = serialize_task_to_markdown(&task);
        let parsed = parse_task_from_markdown(&md).unwrap_or_else(|e| {
            panic!("parse failed:\n=== md ===\n{md}\n=== err ===\n{e}");
        });
        let exec = parsed.execution.unwrap_or_else(|| {
            panic!("execution missing after round-trip:\n=== md ===\n{md}\n=== end ===");
        });
        assert_eq!(exec.env.get("SCAN_DEPTH").map(String::as_str), Some("deep"));
        assert_eq!(exec.env.get("API_TOKEN").map(String::as_str), Some("xyz"));
        assert_eq!(exec.model.as_deref(), Some("x"));
    }

    #[test]
    fn roundtrip_openspec_change() {
        let mut task = sample_task();
        task.openspec_change = Some("auth-rewrite".into());

        let md = serialize_task_to_markdown(&task);
        let parsed = parse_task_from_markdown(&md).unwrap();
        assert_eq!(parsed.openspec_change.as_deref(), Some("auth-rewrite"));
    }

    #[test]
    fn roundtrip_engagement_id() {
        let mut task = sample_task();
        let eid = crate::engagement::EngagementId::new();
        task.engagement_id = Some(eid.clone());

        let md = serialize_task_to_markdown(&task);
        assert!(md.contains("engagement = "), "expected engagement field in:\n{md}");
        let parsed = parse_task_from_markdown(&md).unwrap();
        assert_eq!(parsed.engagement_id, Some(eid));
    }

    #[test]
    fn missing_engagement_round_trips_as_none() {
        let task = sample_task();
        let md = serialize_task_to_markdown(&task);
        assert!(!md.contains("engagement"), "expected no engagement field in:\n{md}");
        let parsed = parse_task_from_markdown(&md).unwrap();
        assert!(parsed.engagement_id.is_none());
    }

    #[test]
    fn empty_execution_block_does_not_emit_section() {
        // is_empty() check should prevent us from writing
        // `[data.execution]\n` for a task with no meaningful exec params.
        let mut task = sample_task();
        task.execution = Some(crate::task::ExecutionSpec::default());

        let md = serialize_task_to_markdown(&task);
        assert!(!md.contains("[data.execution]"), "got: {md}");
    }
}
