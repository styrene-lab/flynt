// Flynt flow viewer/editor — React + @xyflow/react host.
//
// Phase 2: read-only viewer. Operator can pan, zoom, select; no edits
// flow back to disk. Phase 3 will add an `onChange` debounced bridge to
// the Rust side, mirroring how FlyntExcalidraw signals changes.
//
// Public API matches FlyntExcalidraw's shape on purpose so the Rust view
// component can copy that pattern verbatim:
//
//   window.FlyntFlow.mount(elementId, flowJson, { readOnly: true });
//   window.FlyntFlow.unmount();
//
// The bundle is loaded eagerly via document::Script in app.rs.

import * as React from "react";
import { createRoot, Root } from "react-dom/client";
import {
  ReactFlow,
  Background,
  Controls,
  Edge,
  MiniMap,
  Node,
  ReactFlowProvider,
} from "@xyflow/react";
import reactFlowCss from "@xyflow/react/dist/style.css";

// ── Schema mirror ───────────────────────────────────────────────────────────
//
// These types mirror the `Flow` struct from `flynt-flow` (Rust). They're
// duplicated rather than generated because the Rust → TS code-gen story
// in this repo hasn't landed; with one schema and a small surface,
// hand-mirroring is cheaper than wiring up ts-rs / specta.

interface FlowJson {
  meta?: { title?: string; description?: string };
  nodes: FlowNodeJson[];
  edges: FlowEdgeJson[];
}

interface FlowNodeJson {
  id: string;
  kind: string;
  position: [number, number];
  data?: Record<string, unknown>;
  sockets?: SocketJson[];
}

interface SocketJson {
  name: string;
  direction: "input" | "output";
  ty?: string;
}

interface FlowEdgeJson {
  id: string;
  source: { node: string; socket: string };
  target: { node: string; socket: string };
}

interface MountOptions {
  readOnly?: boolean;
  onChange?: (json: string) => void;
}

// ── Adapters: Flynt schema ↔ react-flow wire format ─────────────────────────
//
// react-flow wants `{x, y}` for position; we serialize as `[x, y]` (a
// tuple in Rust) so the bridge swaps shapes here. Sockets become
// react-flow handles (the visible connection points) — for v1 we render
// them as a single bundled handle on each side of the node, which keeps
// the visual minimal until we wire per-socket handles in Phase 3.

// Defensive: agents (Phase 4) may send partial nodes — missing position,
// missing data, missing sockets. We default rather than throw so a
// malformed flow renders as best-effort rather than crashing the view.
function toRfNode(n: FlowNodeJson): Node {
  const pos = Array.isArray(n.position) ? n.position : [0, 0];
  return {
    id: n.id,
    type: "flynt",
    position: { x: Number(pos[0]) || 0, y: Number(pos[1]) || 0 },
    data: {
      kind: n.kind ?? "custom",
      payload: n.data ?? {},
      sockets: Array.isArray(n.sockets) ? n.sockets : [],
    },
  };
}

function toRfEdge(e: FlowEdgeJson): Edge {
  // Edges with no source/target ids would cascade-crash react-flow's
  // diff. Filter at the caller; this function trusts its input but
  // guards socket fields against `undefined`.
  return {
    id: e.id,
    source: e.source.node,
    target: e.target.node,
    sourceHandle: e.source.socket || undefined,
    targetHandle: e.target.socket || undefined,
  };
}

// ── Custom node renderer ────────────────────────────────────────────────────
//
// All Flynt node kinds render through the same component for v1. The
// `kind` shows as a small uppercase label; the title (if present in
// `data`) is the body. Style is intentionally bland — the editor's job
// is to show structure, not to be pretty. Per-kind theming lands when
// the agent-rendering loop generates enough flows that visual
// differentiation earns its keep.

import { Handle, Position } from "@xyflow/react";

function FlyntNode({ data }: { data: { kind: string; payload: Record<string, unknown>; sockets: SocketJson[] } }) {
  const { kind, payload, sockets } = data;
  const title =
    (typeof payload.title === "string" && payload.title) ||
    (typeof payload.name === "string" && payload.name) ||
    (typeof payload.skill === "string" && payload.skill) ||
    kind;

  // Group sockets by direction so input handles render on the left,
  // output handles on the right. Note nodes (no sockets) get nothing —
  // the FlowEndpoint.socket="" fallback handles connection lookup.
  const inputs = sockets.filter((s) => s.direction === "input");
  const outputs = sockets.filter((s) => s.direction === "output");

  return (
    <div
      style={{
        padding: "8px 12px",
        background: kind === "note" ? "#1e293b" : "#0f172a",
        border: "1px solid #334155",
        borderRadius: 6,
        color: "#e2e8f0",
        fontSize: 12,
        minWidth: 140,
        boxShadow: "0 1px 3px rgba(0,0,0,0.3)",
      }}
    >
      <div style={{ fontSize: 10, color: "#64748b", textTransform: "uppercase", letterSpacing: 0.5 }}>
        {kind}
      </div>
      <div style={{ fontWeight: 500, marginTop: 2 }}>{title}</div>
      {inputs.map((s, i) => (
        <Handle
          key={`in-${s.name}`}
          type="target"
          position={Position.Left}
          id={s.name}
          // Stack handles vertically when there are several
          style={{ top: 24 + i * 14 }}
        />
      ))}
      {outputs.map((s, i) => (
        <Handle
          key={`out-${s.name}`}
          type="source"
          position={Position.Right}
          id={s.name}
          style={{ top: 24 + i * 14 }}
        />
      ))}
    </div>
  );
}

const NODE_TYPES = { flynt: FlyntNode };

// ── Mount point ─────────────────────────────────────────────────────────────

function FlowApp({ flow, readOnly }: { flow: FlowJson; readOnly: boolean }) {
  const nodes = React.useMemo(() => flow.nodes.map(toRfNode), [flow]);
  const edges = React.useMemo(() => flow.edges.map(toRfEdge), [flow]);

  return (
    <div style={{ width: "100%", height: "100%" }}>
      <ReactFlowProvider>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={NODE_TYPES}
          nodesDraggable={!readOnly}
          nodesConnectable={!readOnly}
          edgesFocusable={!readOnly}
          elementsSelectable
          fitView
          // Dark-on-dark to match the Flynt theme. The bundled CSS
          // (injected at startup) handles control colors.
          style={{ background: "#020617" }}
        >
          <Background gap={20} color="#1e293b" />
          <Controls position="bottom-right" showInteractive={false} />
          <MiniMap
            position="bottom-left"
            maskColor="rgba(2,6,23,0.8)"
            style={{ background: "#0f172a", border: "1px solid #1e293b" }}
            nodeColor="#475569"
          />
        </ReactFlow>
      </ReactFlowProvider>
    </div>
  );
}

// ── Public API ──────────────────────────────────────────────────────────────

interface FlyntFlowGlobal {
  mount: (elementId: string, flowJson: string, options?: MountOptions) => void;
  unmount: () => void;
  _root?: Root | null;
}

declare global {
  interface Window {
    FlyntFlow?: FlyntFlowGlobal;
  }
}

// Inject react-flow's stylesheet once. The bundle ships the CSS as a
// string (esbuild --loader:.css=text) so we don't depend on the host
// page for styling.
function injectStyles() {
  if (document.getElementById("flynt-flow-styles")) return;
  const style = document.createElement("style");
  style.id = "flynt-flow-styles";
  style.textContent = reactFlowCss;
  document.head.appendChild(style);
}

const api: FlyntFlowGlobal = {
  _root: null,
  mount(elementId, flowJson, options = {}) {
    injectStyles();
    const container = document.getElementById(elementId);
    if (!container) {
      console.error(`[FlyntFlow] no element with id ${elementId}`);
      return;
    }
    let parsed: FlowJson;
    try {
      parsed = JSON.parse(flowJson);
    } catch (err) {
      console.error("[FlyntFlow] invalid flow JSON", err);
      return;
    }
    // Defensive defaults — Rust guarantees these via #[serde(default)],
    // but the tool surface in Phase 4 may pass partial bodies, and a
    // hand-edited .flow file might omit either array entirely.
    parsed.nodes = Array.isArray(parsed.nodes) ? parsed.nodes : [];
    parsed.edges = Array.isArray(parsed.edges) ? parsed.edges : [];
    // Drop edges referencing non-existent or unset endpoints — react-flow
    // tolerates dangling refs but the diff log gets noisy. Validation
    // (Flow::validate) happens server-side; this is just a render-time
    // guard so the canvas doesn't show ghost edges.
    const nodeIds = new Set(parsed.nodes.map((n) => n.id));
    parsed.edges = parsed.edges.filter(
      (e) =>
        e &&
        e.id &&
        e.source?.node &&
        e.target?.node &&
        nodeIds.has(e.source.node) &&
        nodeIds.has(e.target.node)
    );

    if (api._root) api._root.unmount();
    api._root = createRoot(container);
    api._root.render(<FlowApp flow={parsed} readOnly={options.readOnly ?? true} />);
  },
  unmount() {
    if (api._root) {
      try { api._root.unmount(); } catch { /* ignore */ }
      api._root = null;
    }
  },
};

window.FlyntFlow = api;
