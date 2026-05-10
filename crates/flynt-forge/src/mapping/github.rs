//! GitHub-shaped mapper. Forgejo will reuse most of this — its issue
//! API is GitHub-compatible at the field level. GitLab gets its own
//! impl because its label/milestone/state semantics diverge enough
//! that sharing code would be net-noise.
//!
//! Full push/pull logic lands in task #49; this stub establishes the
//! type so the trait surface compiles. Push module (task #50) calls
//! through this; the test surface in task #49 exercises the field
//! translations in isolation.

use flynt_models::task::{Task, TaskPatch};
use styrene_forge::{CreateIssue, ForgeIssue, UpdateIssue};

use super::{MappingConfig, TaskFieldMapper};

#[derive(Debug, Default, Clone, Copy)]
pub struct GitHubMapper;

impl TaskFieldMapper for GitHubMapper {
    fn task_to_issue_create(&self, _task: &Task, _cfg: &MappingConfig) -> CreateIssue {
        // Placeholder — task #49 implements. Returning a minimal payload
        // here so the trait wires through and the push module compiles
        // against it without unimplemented!() in the hot path.
        CreateIssue {
            title: String::new(),
            body: String::new(),
            labels: Vec::new(),
            milestone: None,
            assignees: Vec::new(),
        }
    }

    fn task_to_issue_update(
        &self,
        _task: &Task,
        _current: &ForgeIssue,
        _cfg: &MappingConfig,
    ) -> UpdateIssue {
        UpdateIssue::default()
    }

    fn issue_to_task_patch(&self, _issue: &ForgeIssue, _cfg: &MappingConfig) -> TaskPatch {
        TaskPatch::default()
    }
}
