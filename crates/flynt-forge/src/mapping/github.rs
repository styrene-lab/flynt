//! GitHub-shaped mapper.
//!
//! Forgejo will reuse most of this — its issue API is GitHub-compatible
//! at the field level (state is open/closed, labels are strings,
//! milestones have a title). GitLab gets its own impl because its
//! state/milestone/label semantics diverge enough that sharing code
//! would be net-noise.
//!
//! ## Label-preserve contract (enforced here, defined in `MappingConfig`)
//!
//! On push, `task_to_issue_update` builds a labels vec by:
//! 1. Taking `current.labels` (whatever upstream has now),
//! 2. Filtering out any labels matching `cfg.owns_label(_)` (those are
//!    flynt-managed and we'll re-emit them),
//! 3. Appending the freshly-computed flynt labels (priority + status +
//!    optional due-date label).
//!
//! Net effect: operator-managed labels (`good-first-issue`, `infra`,
//! `bug`, etc.) round-trip untouched; flynt-managed labels reflect the
//! current task state. Without this contract every push would clobber.

use flynt_models::task::{Priority, Task, TaskPatch, TaskStatus};
use styrene_forge::{CreateIssue, ForgeIssue, IssueState, UpdateIssue};

use super::{DueDateStrategy, MappingConfig, TaskFieldMapper};

#[derive(Debug, Default, Clone, Copy)]
pub struct GitHubMapper;

impl TaskFieldMapper for GitHubMapper {
    fn task_to_issue_create(&self, task: &Task, cfg: &MappingConfig) -> CreateIssue {
        let title = if cfg.sync_fields.title { task.title.clone() } else { String::new() };
        let body = if cfg.sync_fields.description { task.description.clone() } else { String::new() };

        let mut labels = collect_flynt_labels(task, cfg);
        if cfg.sync_fields.tags {
            // Tags merged in. Dedup defensively — operator might have
            // a tag that happens to overlap a flynt-managed value.
            for tag in &task.tags {
                if !labels.contains(tag) {
                    labels.push(tag.clone());
                }
            }
        }

        let milestone = milestone_for_due_date(task, cfg);
        if matches!(cfg.due_date.strategy, DueDateStrategy::Label) {
            if let Some(d) = task.due_date {
                let label = format!("due:{}", d);
                if !labels.contains(&label) {
                    labels.push(label);
                }
            }
        }

        CreateIssue {
            title,
            body,
            labels,
            milestone,
            // Assignees: not pushed in v1. flynt doesn't have an
            // "assignee" field; the agent that runs the task is an
            // execution detail, not collaborative ownership.
            assignees: Vec::new(),
        }
    }

    fn task_to_issue_update(
        &self,
        task: &Task,
        current: &ForgeIssue,
        cfg: &MappingConfig,
    ) -> UpdateIssue {
        let mut update = UpdateIssue::default();

        if cfg.sync_fields.title && task.title != current.title {
            update.title = Some(task.title.clone());
        }
        if cfg.sync_fields.description && task.description != current.body {
            update.body = Some(task.description.clone());
        }

        if cfg.sync_fields.status {
            let desired_state: IssueState = cfg
                .status_to_state
                .for_status(status_to_str(task.status))
                .into();
            if desired_state != current.state {
                update.state = Some(desired_state);
            }
        }

        // Labels: preserve operator-managed, rewrite flynt-managed.
        // Always emit a labels diff if anything changed — GitHub's
        // PATCH semantics replace the labels array entirely, so we
        // build the full target list every time.
        let target_labels = build_target_labels(task, current, cfg);
        let mut current_sorted = current.labels.clone();
        current_sorted.sort();
        let mut target_sorted = target_labels.clone();
        target_sorted.sort();
        if current_sorted != target_sorted {
            update.labels = Some(target_labels);
        }

        // Milestone — only push if we have one configured and it differs.
        // We don't clear an existing milestone (would require Some(None)
        // semantics in the styrene-forge UpdateIssue; today the field
        // is `Option<String>` where None means "don't change").
        if cfg.sync_fields.due_date && matches!(cfg.due_date.strategy, DueDateStrategy::Milestone) {
            let desired = milestone_for_due_date(task, cfg);
            if desired != current.milestone {
                update.milestone = desired;
            }
        }

        update
    }

    fn issue_to_task_patch(&self, issue: &ForgeIssue, cfg: &MappingConfig) -> TaskPatch {
        let mut patch = TaskPatch::default();

        if cfg.sync_fields.title {
            patch.title = Some(issue.title.clone());
        }
        if cfg.sync_fields.description {
            patch.description = Some(issue.body.clone());
        }

        // Labels split: flynt-managed → typed fields; rest → tags.
        let mut tags_from_labels: Vec<String> = Vec::new();
        let mut priority_label: Option<&str> = None;
        let mut status_label: Option<&str> = None;
        for label in &issue.labels {
            // Status label takes precedence over priority — they have
            // different prefixes, so this loop catches both.
            if cfg.status_labels.status_for_label(label).is_some() {
                status_label = Some(label.as_str());
                continue;
            }
            if !cfg.priority.label_prefix.is_empty()
                && label.starts_with(&cfg.priority.label_prefix)
            {
                priority_label = Some(label.as_str());
                continue;
            }
            if cfg.owns_label(label) {
                // Owned but not recognized (operator deleted the value
                // half? schema drift?) — drop rather than land it in
                // tags. Tags should never contain "priority:..." junk.
                continue;
            }
            tags_from_labels.push(label.clone());
        }

        if cfg.sync_fields.tags {
            patch.tags = Some(tags_from_labels);
        }

        if cfg.sync_fields.priority && !cfg.priority.label_prefix.is_empty() {
            if let Some(label) = priority_label {
                if let Some(p) = priority_from_label(label, &cfg.priority.label_prefix) {
                    patch.priority = Some(p);
                }
            }
        }

        if cfg.sync_fields.status {
            // First check for a status:* label (richer than open/closed).
            // Fall back to deriving from the binary state.
            let status_from_label = status_label
                .and_then(|l| cfg.status_labels.status_for_label(l))
                .and_then(status_from_str);
            let status = status_from_label.unwrap_or_else(|| match issue.state {
                IssueState::Open => TaskStatus::Todo,
                IssueState::Closed => TaskStatus::Done,
            });
            patch.status = Some(status);
        }

        if cfg.sync_fields.due_date {
            if let Some(m) = &issue.milestone {
                if let Ok(d) = m.parse::<chrono::NaiveDate>() {
                    patch.due_date = Some(Some(d));
                }
            }
        }

        patch
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn priority_to_str(p: Priority) -> &'static str {
    match p {
        Priority::Low => "low",
        Priority::Medium => "medium",
        Priority::High => "high",
        Priority::Critical => "critical",
    }
}

fn priority_from_label(label: &str, prefix: &str) -> Option<Priority> {
    let suffix = label.strip_prefix(prefix)?;
    match suffix {
        "low" => Some(Priority::Low),
        "medium" => Some(Priority::Medium),
        "high" => Some(Priority::High),
        "critical" => Some(Priority::Critical),
        _ => None,
    }
}

fn status_to_str(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Todo => "todo",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Done => "done",
        TaskStatus::Archived => "archived",
    }
}

fn status_from_str(s: &str) -> Option<TaskStatus> {
    match s {
        "todo" => Some(TaskStatus::Todo),
        "in_progress" => Some(TaskStatus::InProgress),
        // "review" is in the mapping config but not the typed enum yet
        // — falls back to Todo in the pull direction so we don't lose
        // the issue. v2 can add Review to TaskStatus and update here.
        "review" => Some(TaskStatus::Todo),
        "done" => Some(TaskStatus::Done),
        "archived" => Some(TaskStatus::Archived),
        _ => None,
    }
}

/// Build the flynt-managed label list for a task. Doesn't include tags
/// — caller merges those separately so the dedup against operator
/// labels happens once.
fn collect_flynt_labels(task: &Task, cfg: &MappingConfig) -> Vec<String> {
    let mut labels = Vec::new();

    if cfg.sync_fields.priority && !cfg.priority.label_prefix.is_empty() {
        labels.push(format!("{}{}", cfg.priority.label_prefix, priority_to_str(task.priority)));
    }

    if cfg.sync_fields.status {
        if let Some(label) = cfg.status_labels.for_status(status_to_str(task.status)) {
            labels.push(label.clone());
        }
    }

    labels
}

/// Compute the full target labels for an update, applying the
/// preserve-operator-labels contract: keep `current.labels` minus
/// anything flynt owns, then append the freshly-computed flynt set.
fn build_target_labels(task: &Task, current: &ForgeIssue, cfg: &MappingConfig) -> Vec<String> {
    let mut out: Vec<String> = current
        .labels
        .iter()
        .filter(|l| !cfg.owns_label(l))
        .cloned()
        .collect();

    // Flynt-managed labels (priority + status).
    for label in collect_flynt_labels(task, cfg) {
        if !out.contains(&label) {
            out.push(label);
        }
    }

    // Tags merged in (with dedup against everything already present).
    if cfg.sync_fields.tags {
        for tag in &task.tags {
            if !out.contains(tag) {
                out.push(tag.clone());
            }
        }
    }

    // Due-date label if that strategy is selected.
    if cfg.sync_fields.due_date && matches!(cfg.due_date.strategy, DueDateStrategy::Label) {
        if let Some(d) = task.due_date {
            let label = format!("due:{}", d);
            if !out.contains(&label) {
                out.push(label);
            }
        }
    }

    out
}

/// Choose the milestone string for a task. Milestone-strategy uses the
/// ISO date as the title; label-strategy returns None (label is
/// handled separately).
fn milestone_for_due_date(task: &Task, cfg: &MappingConfig) -> Option<String> {
    if !cfg.sync_fields.due_date {
        return None;
    }
    if !matches!(cfg.due_date.strategy, DueDateStrategy::Milestone) {
        return None;
    }
    task.due_date.map(|d| d.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Utc};
    use flynt_models::task::{BoardId, TaskId};
    use uuid::Uuid;

    fn make_task() -> Task {
        Task {
            id: TaskId(Uuid::new_v4()),
            board_id: BoardId(Uuid::new_v4()),
            column: "Active".into(),
            title: "Fix the indexer".into(),
            description: "It eats the task title".into(),
            priority: Priority::High,
            status: TaskStatus::InProgress,
            tags: vec!["infra".into(), "bug".into()],
            document_refs: vec![],
            external_refs: vec![],
            due_date: None,
            position: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            decay: flynt_models::task::DecayRate::Natural,
            last_touched_at: None,
            design_node_id: None,
            execution: None,
            openspec_change: None,
            engagement_id: None,
        }
    }

    fn make_issue(labels: Vec<String>, state: IssueState) -> ForgeIssue {
        ForgeIssue {
            number: 42,
            title: "Original".into(),
            body: "Original body".into(),
            state,
            labels,
            milestone: None,
            assignees: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
            url: "https://example.com/issues/42".into(),
        }
    }

    // ── task_to_issue_create ──────────────────────────────────────────

    #[test]
    fn create_emits_title_body_and_labels() {
        let mapper = GitHubMapper;
        let task = make_task();
        let cfg = MappingConfig::default();
        let create = mapper.task_to_issue_create(&task, &cfg);

        assert_eq!(create.title, "Fix the indexer");
        assert_eq!(create.body, "It eats the task title");
        assert!(create.labels.contains(&"priority:high".to_string()),
                "priority label present: {:?}", create.labels);
        assert!(create.labels.contains(&"status:in-progress".to_string()),
                "status label present: {:?}", create.labels);
        assert!(create.labels.contains(&"infra".to_string()));
        assert!(create.labels.contains(&"bug".to_string()));
    }

    #[test]
    fn create_skips_priority_when_field_disabled() {
        let mapper = GitHubMapper;
        let task = make_task();
        let mut cfg = MappingConfig::default();
        cfg.sync_fields.priority = false;
        let create = mapper.task_to_issue_create(&task, &cfg);
        assert!(!create.labels.iter().any(|l| l.starts_with("priority:")));
    }

    #[test]
    fn create_with_due_date_milestone_strategy_sets_milestone() {
        let mapper = GitHubMapper;
        let mut task = make_task();
        task.due_date = NaiveDate::from_ymd_opt(2026, 6, 1);
        let cfg = MappingConfig::default();
        let create = mapper.task_to_issue_create(&task, &cfg);
        assert_eq!(create.milestone.as_deref(), Some("2026-06-01"));
    }

    #[test]
    fn create_with_due_date_label_strategy_emits_due_label() {
        let mapper = GitHubMapper;
        let mut task = make_task();
        task.due_date = NaiveDate::from_ymd_opt(2026, 6, 1);
        let mut cfg = MappingConfig::default();
        cfg.due_date.strategy = DueDateStrategy::Label;
        let create = mapper.task_to_issue_create(&task, &cfg);
        assert_eq!(create.milestone, None);
        assert!(create.labels.contains(&"due:2026-06-01".to_string()));
    }

    // ── task_to_issue_update — the label-preserve contract ───────────────

    #[test]
    fn update_preserves_operator_labels_when_status_changes() {
        let mapper = GitHubMapper;
        let task = make_task(); // status = InProgress
        let cfg = MappingConfig::default();

        // Upstream issue has both operator labels and stale flynt labels.
        let issue = make_issue(
            vec![
                "good-first-issue".into(),     // operator
                "infra".into(),                 // operator (will round-trip)
                "priority:low".into(),         // flynt (stale)
                "status:review".into(),         // flynt (stale)
            ],
            IssueState::Open,
        );

        let update = mapper.task_to_issue_update(&task, &issue, &cfg);
        let new_labels = update.labels.expect("labels updated");

        // Operator labels survive.
        assert!(new_labels.contains(&"good-first-issue".into()), "{new_labels:?}");
        assert!(new_labels.contains(&"infra".into()), "{new_labels:?}");
        // Stale flynt labels gone.
        assert!(!new_labels.contains(&"priority:low".into()), "{new_labels:?}");
        assert!(!new_labels.contains(&"status:review".into()), "{new_labels:?}");
        // Fresh flynt labels present.
        assert!(new_labels.contains(&"priority:high".into()), "{new_labels:?}");
        assert!(new_labels.contains(&"status:in-progress".into()), "{new_labels:?}");
    }

    #[test]
    fn update_skips_unchanged_fields() {
        let mapper = GitHubMapper;
        let task = make_task();
        let cfg = MappingConfig::default();

        // Upstream matches the task exactly.
        let issue = ForgeIssue {
            number: 42,
            title: task.title.clone(),
            body: task.description.clone(),
            state: IssueState::Open,
            labels: vec!["priority:high".into(), "status:in-progress".into(), "infra".into(), "bug".into()],
            milestone: None,
            assignees: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
            url: String::new(),
        };

        let update = mapper.task_to_issue_update(&task, &issue, &cfg);
        // No diffs — every field is None.
        assert!(update.title.is_none());
        assert!(update.body.is_none());
        assert!(update.state.is_none());
        assert!(update.labels.is_none(), "labels match — no diff emitted: {:?}", update.labels);
    }

    #[test]
    fn update_status_done_closes_the_issue() {
        let mapper = GitHubMapper;
        let mut task = make_task();
        task.status = TaskStatus::Done;
        let cfg = MappingConfig::default();
        let issue = make_issue(vec![], IssueState::Open);
        let update = mapper.task_to_issue_update(&task, &issue, &cfg);
        assert_eq!(update.state, Some(IssueState::Closed));
    }

    // ── issue_to_task_patch ──────────────────────────────────────────────

    #[test]
    fn pull_translates_labels_back_into_typed_fields() {
        let mapper = GitHubMapper;
        let cfg = MappingConfig::default();
        let issue = make_issue(
            vec![
                "priority:critical".into(),
                "status:in-progress".into(),
                "good-first-issue".into(),
                "infra".into(),
            ],
            IssueState::Open,
        );
        let patch = mapper.issue_to_task_patch(&issue, &cfg);

        assert_eq!(patch.priority, Some(Priority::Critical));
        assert_eq!(patch.status, Some(TaskStatus::InProgress));
        // tags = unprefixed labels only
        let tags = patch.tags.expect("tags set");
        assert_eq!(tags, vec!["good-first-issue".to_string(), "infra".to_string()]);
    }

    #[test]
    fn pull_falls_back_to_state_when_no_status_label() {
        let mapper = GitHubMapper;
        let cfg = MappingConfig::default();
        let issue = make_issue(vec!["priority:high".into()], IssueState::Closed);
        let patch = mapper.issue_to_task_patch(&issue, &cfg);
        assert_eq!(patch.status, Some(TaskStatus::Done),
                   "closed state without status: label → Done");
    }

    #[test]
    fn pull_drops_owned_but_unrecognized_labels() {
        // E.g., a `priority:weird` label that doesn't match any known
        // priority. Drop rather than land in tags (tags should never
        // contain "priority:..." junk).
        let mapper = GitHubMapper;
        let cfg = MappingConfig::default();
        let issue = make_issue(vec!["priority:weird".into(), "infra".into()], IssueState::Open);
        let patch = mapper.issue_to_task_patch(&issue, &cfg);
        assert!(!patch.tags.unwrap().contains(&"priority:weird".to_string()));
        // priority stays default (None — caller decides whether to apply).
        assert!(patch.priority.is_none());
    }

    #[test]
    fn pull_reads_milestone_as_due_date() {
        let mapper = GitHubMapper;
        let cfg = MappingConfig::default();
        let mut issue = make_issue(vec![], IssueState::Open);
        issue.milestone = Some("2026-06-01".into());
        let patch = mapper.issue_to_task_patch(&issue, &cfg);
        assert_eq!(patch.due_date, Some(Some(NaiveDate::from_ymd_opt(2026, 6, 1).unwrap())));
    }

    #[test]
    fn pull_ignores_unparseable_milestone() {
        // Operator-named milestones like "v1.0" — we don't crash, we
        // just leave due_date alone.
        let mapper = GitHubMapper;
        let cfg = MappingConfig::default();
        let mut issue = make_issue(vec![], IssueState::Open);
        issue.milestone = Some("v1.0".into());
        let patch = mapper.issue_to_task_patch(&issue, &cfg);
        assert_eq!(patch.due_date, None);
    }

    // ── Round-trip ───────────────────────────────────────────────────────

    #[test]
    fn task_create_then_pull_round_trips() {
        // Push the task, simulate the issue that GitHub would store,
        // pull it back, verify the typed fields survive.
        let mapper = GitHubMapper;
        let task = make_task();
        let cfg = MappingConfig::default();

        let create = mapper.task_to_issue_create(&task, &cfg);
        let issue = ForgeIssue {
            number: 1,
            title: create.title,
            body: create.body,
            state: IssueState::Open,
            labels: create.labels,
            milestone: create.milestone,
            assignees: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
            url: String::new(),
        };
        let patch = mapper.issue_to_task_patch(&issue, &cfg);

        assert_eq!(patch.title.as_deref(), Some("Fix the indexer"));
        assert_eq!(patch.description.as_deref(), Some("It eats the task title"));
        assert_eq!(patch.priority, Some(Priority::High));
        assert_eq!(patch.status, Some(TaskStatus::InProgress));
        let tags = patch.tags.expect("tags");
        assert!(tags.contains(&"infra".to_string()));
        assert!(tags.contains(&"bug".to_string()));
    }
}
