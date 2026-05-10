+++
id = "flow-canvas-bidirectional"
kind = "design_node"
title = "Flow ↔ Canvas bidirectional integration (was Phase 6)"
status = "deferred"
tags = ["flow", "canvas", "design", "speculative"]

[data]
parent = "flow-editor"
issue_type = "feature"
priority = 4
depends_on = ["flow-editor-shipped", "flow-workflow-export"]
trigger = "operator-asks-for-it"
+++

# Flow ↔ Canvas bidirectional integration

## Status

**Distant.** Phase 6 in the original flow-editor plan. The pitch was: sketch
an architecture in the flow editor, hop to the canvas (Flynt's design canvas
view) and have the agent generate a UI design from that structure — or
sketch in canvas, derive the flow from the result.

I argued against shipping this when the editor was being designed. The
argument hasn't changed: it's the strongest "wow" feature in the original
pitch and also the hardest sync problem on the table.

## Why this is harder than it looks

Two-way sync between two visual representations is a notoriously deep
problem. ComfyUI, UE Blueprints, Node-Red — none of them have it. The
nearest analogues are CAD tools (model ↔ drawing) which have decades of
investment behind their bidirectional projection logic.

The hard parts, ordered by sharpness:

1. **What's the source of truth?** If the operator edits the flow, does
   the canvas regenerate from scratch (losing manual canvas tweaks) or
   incrementally update (requires diffing)? Same in reverse.
2. **What's the schema mapping?** A flow node maps to... a canvas
   element? A canvas frame? Multiple canvas elements? The right answer
   depends on what canvas elements are *for* — and the canvas system
   itself is still finding its shape.
3. **Layout preservation across edits.** Operator hand-arranges canvas
   elements in a non-grid pattern. Operator then renames a flow node.
   Canvas regenerates. Where do hand-arranged elements go?
4. **Live update vs explicit sync.** "Flow changes immediately mirror to
   canvas" feels magic but breaks the operator's ability to iterate
   either side independently. "Sync on demand" loses the live-feel that
   was the original pitch.

None of these are unsolvable. All of them eat months.

## When to revisit

The trigger is the operator asking "I drew this flow, can the agent
turn it into a canvas?" or vice versa — *combined with* the flow
editor being used routinely (i.e., `flow-workflow-export` shipped or
firmly demanded). Without the second condition, this would be building
a sync engine for a graph nobody draws.

## Sketch (don't take this as a plan)

If we did build it, the cheap-first approach:

- **One-shot generation, not bidirectional sync.** New tool
  `canvas_create_from_flow(flow_path) → canvas_path` that produces a
  *fresh* canvas from a flow. No syncing back. Operator can iterate
  the canvas freely; if they want it to track the flow, they
  regenerate.
- The reverse: `flow_create_from_canvas(canvas_path)` for the cases
  where the operator sketched in canvas first.
- These two tools together get 80% of the pitch's value at <10% of
  the cost. Real bidirectional sync stays out unless someone proves
  they need it.

## Out of scope when revisited

- Continuous bidirectional sync — explicitly punted
- Live execution overlay (flow + canvas + execution state in one view)
- Multi-flow → single-canvas composition (one flow per canvas at most)
