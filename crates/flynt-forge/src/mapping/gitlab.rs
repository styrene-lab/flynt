//! GitLab mapper.
//!
//! GitLab's REST API uses the same `styrene-forge` abstraction
//! (CreateIssue, UpdateIssue, ForgeIssue with title/body/state/labels/
//! milestone) so the v1 mapping logic mirrors GitHub's. Notable
//! GitLab quirks that *might* matter when divergence becomes
//! necessary:
//!
//! - GitLab uses `opened` / `closed` state strings on the wire, but
//!   `styrene-forge::IssueState` normalizes them to `Open` / `Closed`.
//!   Mapper doesn't see the wire form — no change here.
//! - GitLab milestones can carry a `due_date` field of their own.
//!   The current milestone strategy treats milestone-as-title-with-
//!   date, which works but loses the precision GitLab natively
//!   supports. A v2 GitLabMapper could prefer the native field.
//! - GitLab supports issue weights (priority-ish numeric field).
//!   v1 ignores; future versions could mirror flynt's priority int
//!   into GitLab weight.
//! - GitLab "scoped labels" (e.g. `status::in-progress`) use `::` as
//!   the separator. Operators using that convention may want to
//!   override `status_labels` in `forge-mapping.toml` accordingly —
//!   it's all string-driven on flynt's side.
//!
//! For v1, all methods delegate to GitHubMapper. The divergence
//! points are documented above so future mapper work knows where
//! to start.

use flynt_models::task::{Task, TaskPatch};
use styrene_forge::{CreateIssue, ForgeIssue, UpdateIssue};

use super::{GitHubMapper, MappingConfig, TaskFieldMapper};

#[derive(Debug, Default, Clone, Copy)]
pub struct GitlabMapper;

impl TaskFieldMapper for GitlabMapper {
    fn task_to_issue_create(&self, task: &Task, cfg: &MappingConfig) -> CreateIssue {
        GitHubMapper.task_to_issue_create(task, cfg)
    }

    fn task_to_issue_update(
        &self,
        task: &Task,
        current: &ForgeIssue,
        cfg: &MappingConfig,
    ) -> UpdateIssue {
        GitHubMapper.task_to_issue_update(task, current, cfg)
    }

    fn issue_to_task_patch(&self, issue: &ForgeIssue, cfg: &MappingConfig) -> TaskPatch {
        GitHubMapper.issue_to_task_patch(issue, cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use flynt_models::task::{BoardId, Priority, TaskId, TaskStatus};
    use styrene_forge::IssueState;
    use uuid::Uuid;

    fn make_task() -> Task {
        Task {
            id: TaskId(Uuid::new_v4()),
            board_id: BoardId(Uuid::new_v4()),
            column: "Active".into(),
            title: "T".into(),
            description: "B".into(),
            priority: Priority::Medium,
            status: TaskStatus::Todo,
            tags: vec![],
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

    #[test]
    fn gitlab_create_matches_github_until_divergence() {
        // Same property guard as forgejo's matching test — when this
        // fails, divergence has landed and both mappers' tests should
        // be revisited.
        let task = make_task();
        let cfg = MappingConfig::default();
        let gl = GitlabMapper.task_to_issue_create(&task, &cfg);
        let gh = GitHubMapper.task_to_issue_create(&task, &cfg);
        assert_eq!(gl.title, gh.title);
        assert_eq!(gl.body, gh.body);
        assert_eq!(gl.labels, gh.labels);
    }

    #[test]
    fn gitlab_pull_translates_status_labels() {
        // Spot-check via the operator's likely override:
        // `status_labels.in_progress = "status::in-progress"` (double
        // colon — GitLab scoped label style). Mapper doesn't care; it
        // reads whatever `status_labels` says.
        let mut cfg = MappingConfig::default();
        cfg.status_labels.in_progress = Some("status::in-progress".into());

        let issue = ForgeIssue {
            number: 7,
            title: "T".into(),
            body: "B".into(),
            state: IssueState::Open,
            labels: vec!["status::in-progress".into()],
            milestone: None,
            assignees: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
            url: String::new(),
        };
        let patch = GitlabMapper.issue_to_task_patch(&issue, &cfg);
        assert_eq!(patch.status, Some(TaskStatus::InProgress));
    }
}
