# Sentry Integration — Flynt as a TaskBoard for Omegon

## Context

Omegon is building `omegon sentry` — a long-running autonomous task executor that pulls work from a **TaskBoard** trait. The trait is board-agnostic: the built-in `FileTaskBoard` reads tasks from a TOML config file. But the natural upgrade path is `FlyntTaskBoard` — tasks managed in Flynt's kanban UI, executed automatically by sentry.

This document specifies what Flynt needs to implement to be a viable TaskBoard backend, and where the current task system falls short.

See: `omegon/design/autonomous-tasking.md` for the sentry-side architecture.

## The TaskBoard Contract

Sentry interacts with any board through 6 operations:

| Operation | What sentry needs | What it does |
|---|---|---|
| `list_actionable()` | Tasks ready to execute | Query tasks by column/status/tag that are eligible for agent execution |
| `claim(task_id)` | Atomic exclusive lock | Prevent double-execution when multiple sentry instances run |
| `release(task_id)` | Undo a claim | Return task to actionable state (sentry shutdown, preemption) |
| `complete(task_id, result)` | Mark done + attach result | Move task, record summary/tokens/duration, touch decay clock |
| `fail(task_id, error)` | Mark failed + decide retry | Move task to failed state or increment retry counter |
| `task_spec(task_id)` | Execution parameters | Prompt text, model, max_turns, timeout, skill, token_budget, cwd |

## Current State Assessment

### What works today

| Capability | Status | Notes |
|---|---|---|
| Task CRUD (Create/Read/Delete) | Production | `save_task`, `get_task`, `list_tasks`, `delete_task` all work |
| Board CRUD | Production | Full lifecycle |
| Task file serialization | Production | Round-trip tested, TOML frontmatter + markdown body |
| Git sync / flush | Production | Dirty tracking, atomic commit, conflict detection |
| Decay logic | Production | Exponential half-life, `relevance()`, `is_fading()`, `should_auto_archive()` — well-tested |
| flynt-agent extension | Production | `list_tasks`, `get_task`, `create_task`, `list_boards`, `get_board`, `create_board` all wired |

### What's missing

| Gap | Impact on sentry | Severity |
|---|---|---|
| **No task Update RPC** | `claim()`, `complete()`, `fail()` all need to mutate task status/column. Agent currently can't change a task's status, priority, column, or any field without delete+recreate. | **Blocking** |
| **`external_refs` not persisted to SQLite** | Trigger definitions (cron expressions, webhook names) stored in `external_refs` are lost on process restart. sqlite.rs:697 has `external_refs: Vec::new()` with a TODO. | **Blocking** |
| **`design_node_id` not persisted to SQLite** | Minor for sentry, but indicates the schema migration gap is broader. sqlite.rs:698. | Low |
| **No claim/release semantics** | Two sentry instances could execute the same task simultaneously. No advisory lock or CAS on task status. | **Blocking for multi-instance** |
| **No execution metadata field** | Tasks have title+description+priority+status but nowhere to store model, max_turns, timeout, skill. The `[data.sentry]` frontmatter section doesn't exist yet. | **Blocking** |
| **Decay system unused** | `relevance()` is called once (notification check). No auto-archival, no UI visualization, no query filtering by decay state. Sentry would benefit from decay-based re-queuing of recurring tasks. | Nice-to-have |
| **Task query limited** | Can filter by status and priority only. No filter by tag, column, due_date, or decay state. Sentry needs column-based or tag-based filtering for `list_actionable()`. | Medium |
| **WIP limits not enforced** | Sentry respects max_concurrent on its side, but Flynt's WIP limits are decorative. Not blocking — sentry handles its own concurrency. | Low |

## Required Changes

### Tier 1 — Blocking (must ship before FlyntTaskBoard adapter)

#### 1. Task Update RPC

Add `update_task` to the flynt-agent extension. Must support partial updates — sentry needs to change status and column without clobbering other fields.

**flynt-agent/src/extension.rs:**
- Add `update_task` tool: accepts `task_id` + partial field map (status, column, priority, tags, description, due_date)
- Route to a new `Project::update_task()` or `ProjectStore::update_task()` method

**flynt-store/src/sqlite.rs:**
- Add `update_task(&self, id: &TaskId, patch: &TaskPatch) -> Result<()>`
- `TaskPatch` struct with all-optional fields — only provided fields are SET in the UPDATE query
- Bump `updated_at` on every mutation

**flynt-models/src/task.rs:**
```rust
#[derive(Default, Serialize, Deserialize)]
pub struct TaskPatch {
    pub column: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub status: Option<TaskStatus>,
    pub tags: Option<Vec<String>>,
    pub due_date: Option<Option<NaiveDate>>,   // Some(None) = clear
    pub external_refs: Option<Vec<String>>,
    pub position: Option<u32>,
    pub decay: Option<DecayRate>,
}
```

#### 2. Persist external_refs and design_node_id to SQLite

**flynt-store/src/sqlite.rs:**
- Add `external_refs TEXT NOT NULL DEFAULT '[]'` column (JSON array)
- Add `design_node_id TEXT` column
- Schema migration: ALTER TABLE or version bump
- Fix row deserializer at line ~697-698 to read from DB instead of hardcoding empty

#### 3. Execution Metadata

Tasks need a place to store sentry execution parameters. Two options:

**Option A — Dedicated `execution` field on Task:**
```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub execution: Option<ExecutionSpec>,

#[derive(Serialize, Deserialize)]
pub struct ExecutionSpec {
    pub model: Option<String>,
    pub skill: Option<String>,
    pub max_turns: Option<u32>,
    pub timeout_secs: Option<u64>,
    pub token_budget: Option<u64>,
    pub cwd: Option<String>,
}
```

**Option B — Convention over structure:**
Use `external_refs` with prefixed URIs:
```
sentry:model=anthropic:claude-sonnet-4-6
sentry:max_turns=20
sentry:skill=security
cron:0 */4 * * *
webhook:github-pr
```

**Recommendation:** Option A. It's type-safe, queryable, and doesn't overload a string array with structured data. Option B is clever but fragile — parsing `key=value` from a `Vec<String>` is a code smell.

For the task file format, this becomes a natural frontmatter section:

```markdown
+++
id = "..."
kind = "task"

[data]
title = "Review open PRs"
board = "..."
column = "Scheduled"
priority = 3
status = "todo"
tags = ["sentry", "recurring"]
external_refs = ["cron:0 */4 * * *", "webhook:github-pr"]
decay = "none"

[data.execution]
model = "anthropic:claude-sonnet-4-6"
max_turns = 20
timeout_secs = 300
skill = "security"
+++

Review all open PRs on the main repository...
```

#### 4. Task Filtering by Column and Tags

**flynt-store/src/sqlite.rs:**
- Extend `TaskFilter` to support tag filtering:
```rust
pub struct TaskFilter {
    pub board_id: Option<BoardId>,
    pub column: Option<String>,
    pub tags: Vec<String>,         // already exists but verify it's wired
    pub status: Option<TaskStatus>, // add this
}
```
- `list_tasks()` SQL builder must handle all filter fields

**flynt-agent extension:**
- `list_tasks` tool should accept `column`, `tags`, and `status` parameters

### Tier 2 — Important (should ship for production sentry use)

#### 5. Claim/Release Semantics

For single-instance sentry, claim is just "move to Running column." For multi-instance, it needs atomicity.

**Simple approach (single-instance):**
- `claim()` = `update_task(id, status=InProgress, column="Running")`
- `release()` = `update_task(id, status=Todo, column="Scheduled")`
- Race condition possible but acceptable for single-pod deployment

**Robust approach (multi-instance, future):**
- Add `claimed_by: Option<String>` field to Task (sentry instance ID)
- `claim()` uses `UPDATE tasks SET claimed_by = ? WHERE id = ? AND claimed_by IS NULL` (CAS)
- `release()` uses `UPDATE tasks SET claimed_by = NULL WHERE id = ? AND claimed_by = ?`

**Recommendation:** Start with simple approach. Add `claimed_by` when distributed sentry (Phase 6 in omegon roadmap) is in scope.

#### 6. Decay-Based Auto-Requeue

For recurring tasks: when a task completes, its decay clock starts. When relevance drops below a threshold, the task should re-enter the "Scheduled" column automatically.

This requires:
- A periodic sweep in Flynt (or in sentry via the adapter) that checks `should_auto_archive()` and reverses it for recurring tasks
- A `recurring: bool` or `recurrence: Option<RecurrencePolicy>` field on Task
- The decay system to actually be wired into the task lifecycle, not just notifications

**This is the bridge between Flynt's existing decay math and sentry's scheduling.**

### Tier 3 — Nice-to-Have

#### 7. Run History on Tasks

Tasks should accumulate a run log — when sentry executed them, what the result was, tokens consumed, duration. Options:

- Append to task description (simple but noisy)
- Linked documents (one doc per run, linked via `document_refs`)
- Dedicated `runs` table in SQLite

**Recommendation:** Dedicated `task_runs` table. It's clean, queryable, and doesn't pollute the task model.

#### 8. Decay Visualization in UI

Show relevance score on kanban cards. Fading tasks get muted colors. Tasks that `should_auto_archive()` get a visual indicator. This is independent of sentry but makes the decay system visible to users.

#### 9. WIP Limit Enforcement

When sentry tries to move a task into a column that's at WIP capacity, the board should reject it. This creates backpressure from the UI into the executor.

## Integration Flow

When fully implemented, the workflow looks like:

1. User creates a task in Flynt UI (or via agent), places it in "Scheduled" column on a sentry-watched board
2. User fills in execution metadata (model, turns, timeout) — either via UI fields or markdown frontmatter
3. User sets triggers via `external_refs` (cron expressions, webhook names)
4. `omegon sentry --flynt-project ~/.flynt/project` starts, discovers the board via config
5. Trigger evaluator matches a cron expression → calls `board.list_actionable()`
6. Sentry claims the task → Flynt moves it to "Running" column
7. Sentry reads `task_spec()` → prompt from description, params from execution metadata
8. Sentry spawns `omegon run` with those parameters
9. On completion: Flynt moves task to "Done", records result, touches decay clock
10. Decay timer starts. For recurring tasks, relevance drops → task re-enters "Scheduled"

## Non-Goals

- **Flynt should not import omegon types.** The integration is via RPC (flynt-agent extension), not library dependency. Flynt's task model is self-contained.
- **Flynt should not implement trigger evaluation.** That's sentry's job. Flynt stores trigger metadata (cron expressions in `external_refs`), sentry interprets them.
- **Flynt should not manage agent execution.** No process spawning, no model selection logic, no checkpoint management. Flynt is the board, sentry is the executor.

## Priority Order for Implementation

1. **Task Update RPC** — unblocks everything else
2. **Persist external_refs to SQLite** — unblocks trigger storage
3. **Execution metadata field** — unblocks task_spec()
4. **Task filtering by column/tags/status** — unblocks list_actionable()
5. **Claim/release (simple)** — unblocks claim()/release()
6. Everything else is post-integration polish
