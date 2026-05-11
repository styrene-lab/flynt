//! Forgejo mapper. Forgejo's REST API is GitHub-compatible at the
//! issue-field level (title/body/state/labels/milestone all have
//! the same shape), so the mapping logic is identical to GitHubMapper.
//!
//! This module exists as a separate type so:
//! 1. Push code can `match` on `ForgeKind::Forgejo` and construct the
//!    right mapper without conditional logic in the consumer.
//! 2. If Forgejo's behavior ever diverges (custom labels handling,
//!    different state strings on the wire, Forgejo's "Reactions" field),
//!    the divergence lands here without touching GitHubMapper.
//!
//! For now, every method delegates to `GitHubMapper`. No surprises.

use flynt_models::task::{Task, TaskPatch};
use styrene_forge::{CreateIssue, ForgeIssue, UpdateIssue};

use super::{GitHubMapper, MappingConfig, TaskFieldMapper};

#[derive(Debug, Default, Clone, Copy)]
pub struct ForgejoMapper;

impl TaskFieldMapper for ForgejoMapper {
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
            priority: Priority::High,
            status: TaskStatus::InProgress,
            tags: vec!["x".into()],
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
    fn forgejo_create_matches_github_byte_for_byte() {
        // Until Forgejo diverges, the two mappers produce identical
        // payloads. This test will fail (loudly) when divergence
        // lands — that's the right signal to look at both mappers.
        let task = make_task();
        let cfg = MappingConfig::default();
        let fj = ForgejoMapper.task_to_issue_create(&task, &cfg);
        let gh = GitHubMapper.task_to_issue_create(&task, &cfg);
        assert_eq!(fj.title, gh.title);
        assert_eq!(fj.body, gh.body);
        assert_eq!(fj.labels, gh.labels);
        assert_eq!(fj.milestone, gh.milestone);
    }

    #[test]
    fn forgejo_pull_matches_github() {
        let cfg = MappingConfig::default();
        let issue = ForgeIssue {
            number: 1,
            title: "T".into(),
            body: "B".into(),
            state: IssueState::Open,
            labels: vec!["priority:high".into(), "tag".into()],
            milestone: None,
            assignees: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
            url: String::new(),
        };
        let fj = ForgejoMapper.issue_to_task_patch(&issue, &cfg);
        let gh = GitHubMapper.issue_to_task_patch(&issue, &cfg);
        assert_eq!(fj.priority, gh.priority);
        assert_eq!(fj.status, gh.status);
        assert_eq!(fj.tags, gh.tags);
    }
}
