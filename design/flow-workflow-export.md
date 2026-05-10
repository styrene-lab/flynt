+++
id = "flow-workflow-export"
kind = "design_node"
title = "Flow → Omegon workflow export (was Phase 5)"
status = "deferred"
tags = ["flow", "omegon", "workflows", "speculative"]

[data]
parent = "flow-editor"
issue_type = "feature"
priority = 3
depends_on = ["flow-editor-shipped"]
trigger = "operators-actually-using-flow-editor"
+++

# Flow → Omegon workflow export

## Status

**Deferred.** Phase 5 in the original flow-editor plan. Held back deliberately
until operators actually exercise the agent → editor → operator loop shipped
in Phases 1–4. Premature work here is the kind of speculative generality
that sinks node editors.

## What it would do

Convert a `Flow` (from `flynt-flow`) into an Omegon workflow descriptor that
sentry/omegon can execute. The conversion lives in `flynt-flow::omegon`
(currently a placeholder gate, not implemented).

Concretely: the operator (or an agent) sketches an architecture in the flow
editor — `input → agent_call → output` with maybe a `branch`. Today that's
just a static graph. Phase 5 would let them right-click → "Run as workflow"
or call a tool `flow_export_workflow(path) → omegon-workflow JSON`.

## Why we held back

Three reasons, in order of weight:

1. **No operator demand yet.** Phase 4 just shipped; the agent loop hasn't
   been used in anger. Building export against an unproven editor is the
   kind of speculative generality that earns the "easy to start, impossible
   to finish" tax.
2. **Mapping isn't 1:1.** Flow nodes are loose — `data` is
   `serde_json::Value`. Omegon workflows have a defined schema. The
   translation layer needs decisions (which `agent_call.data` fields map
   to which workflow fields? what happens to `note` nodes?) that are
   easier to make once we've seen actual graphs.
3. **Validation overlap.** `Flow::validate` reports dangling refs; an
   exporter would also need to enforce workflow-specific invariants
   (every path reaches an output, no orphan nodes, type-checked sockets).
   Those invariants are easier to design after seeing real workflows.

## When to revisit

The trigger is **"an operator asks to run a flow they drew"** — not a date,
not "after N flows are created." If three months pass with no such ask, the
right move is probably to delete this design note rather than build the
feature.

## Sketch (don't take this as a plan)

If we did build it:

- New module `flynt-flow/src/omegon.rs` — gated behind a feature flag so
  flynt-flow stays minimal-deps for the editor consumers.
- A `Flow::to_omegon_workflow(self) -> Result<OmegonWorkflow>` method.
  Returns Err with the missing-invariant list when the flow doesn't form
  a valid workflow.
- A new agent tool `flow_export_workflow(path) → workflow JSON` so the
  agent can ask "is this flow ready to run?" and surface the failure list.
- Right-click menu item in the editor that calls the tool and shows the
  result inline (or invokes omegon directly if it's installed).
- Round-trip tests: omegon workflow → Flow → omegon workflow ≈ identity
  (modulo position metadata that the workflow doesn't care about).

## Out of scope when revisited

- Flow execution inside Flynt (no — that's omegon's job)
- Editing the workflow output as text (no — Flow is the source of truth)
- Live execution status overlaid on the flow canvas (maybe later, but a
  separate phase)
