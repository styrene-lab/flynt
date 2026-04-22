# OpenSpec Integration — Auditability from Design to Test

Design document for connecting design nodes, tasks, and OpenSpec scenarios
into an auditable chain within Codex vaults.

## The Audit Chain

```
DesignNode (decision)
  ├── Task (implementation work)
  └── OpenSpecScenario (validation)
        └── TestResult (evidence)
```

Every design decision can be traced to: what work was done to implement it,
what scenarios validate it, and whether those scenarios pass.

## Data Model

### OpenSpecScenario Entity

A new entity kind stored as markdown in the vault:

```toml
+++
id = "uuid"
kind = "openspec_scenario"

[data]
title = "Expired token rejected"
domain = "auth"
status = "passing"          # draft | active | passing | failing | deferred
design_node = "uuid"        # the design decision this validates
change = "token-rotation"   # the OpenSpec change name (optional)
last_verified = "2026-04-21T12:00:00Z"
last_result = "pass"        # pass | fail | error | skip
+++

## Scenario: Expired token rejected

Given a user has a JWT token that expired 5 minutes ago
When they make a GET request to /api/protected
Then the response status is 401
And the body contains {"error": "token_expired"}

## Implementation Notes

Verified by: `tests/auth/test_token_expiry.rs::test_expired_token_rejected`
```

### TestResult Record

Stored as entries in a test results log (not individual files):

```toml
# .codex/test-results.toml
[[results]]
scenario_id = "uuid"
timestamp = "2026-04-21T12:00:00Z"
result = "pass"          # pass | fail | error | skip
duration_ms = 42
runner = "cargo test"     # what ran the test
source = "tests/auth/test_token_expiry.rs"
commit = "abc1234"        # git commit at time of run
```

### Design Node ← Scenario Link

`design_node` field on the scenario points to the design node UUID.
The design node doesn't need to know about its scenarios — they're
discovered via `list_entities_by_kind(OpenSpecScenario)` filtered
by `design_node == node_id`.

### Task ← Design Node Link

`Task.design_node_id: Option<Uuid>` (added to the Task model).
When a design node moves to "implementing", tasks can be spawned
on the project board. When all tasks complete, the design node
moves to "implemented" and scenarios should be verified.

## Lifecycle Integration

```
Design Node: seed → exploring → resolved → decided → implementing → implemented
                                                │                        │
                                                ▼                        ▼
                                         Tasks spawned            Scenarios verified
                                         on project board         (test suite runs)
```

### Status Derivation

A design node's health is derived from its scenarios:

| Scenario Status | Meaning |
|----------------|---------|
| All passing | Design validated — node is healthy |
| Any failing | Design has regression — needs attention |
| No scenarios | Unvalidated — acceptable for seed/exploring, flag for decided+ |
| All deferred | Scenarios exist but not actively run |

### Agent Capabilities

Omegon tools for OpenSpec:

| Tool | Description |
|------|-------------|
| `create_scenario` | Create a new OpenSpec scenario linked to a design node |
| `list_scenarios` | List scenarios, optionally filtered by design node or status |
| `record_test_result` | Record a pass/fail result for a scenario |
| `verify_design_node` | Check all scenarios for a design node and report health |
| `promote_task` | Link an existing task to a design node |

### Query Blocks

Users can embed scenario status in notes:

````
```query
TASK FROM "" WHERE design_node = "uuid-of-auth-design"
```
````

And scenario health:

````
```query
TABLE title, status, last_result FROM "" WHERE kind = "openspec_scenario" AND design_node = "uuid"
```
````

## Graph Integration

Scenarios appear as graph nodes with edges:

- `GraphNodeKind::Scenario` (green when passing, red when failing)
- `GraphEdgeKind::Validates` (scenario → design node)
- Color-coded: green border = all passing, red = any failing, gray = no scenarios

## Implementation Plan

### Phase 1: Data Model (codex-core)

- Add `EntityKind::OpenSpecScenario` to datum.rs
- Add `OpenSpecScenarioView` with typed accessors
- Add `Task.design_node_id: Option<Uuid>` field
- Add `GraphNodeKind::Scenario` + `GraphEdgeKind::Validates`
- Test result storage model

### Phase 2: Storage (codex-store)

- Index scenarios from vault (reindex picks up `kind = "openspec_scenario"`)
- `vault.record_test_result()` writes to `.codex/test-results.toml`
- `vault.scenario_health(design_node_id)` aggregates scenario statuses
- Query engine: support `design_node` filter in WHERE clauses

### Phase 3: Agent Tools (codex-agent)

- `create_scenario` tool
- `list_scenarios` tool
- `record_test_result` tool
- `verify_design_node` tool
- `promote_task` tool (adds design_node_id to existing task)

### Phase 4: UI (codex-app)

- Graph: scenario nodes with health coloring
- Board: design-tree badge on tasks linked to design nodes
- Notes: scenario status inline via query blocks
- Settings: test runner configuration (optional)

## What This Provides

1. **Traceability**: "Why does this test exist?" → because design node X decided Y
2. **Coverage visibility**: which design decisions have validation, which don't
3. **Regression detection**: design node health derived from scenario results
4. **Audit trail**: decision → implementation → validation → evidence, all in git
5. **Agent awareness**: Omegon can verify design decisions against their specs
