// Flynt flow viewer/editor — React + @xyflow/react host.
//
// Phase 3: editable. Operator can drag nodes, draw new edges, and delete
// selected elements; changes flow back to disk via a debounced onChange
// callback registered by the Rust view. Phase 4 will add agent tools.
//
// Public API matches FlyntExcalidraw's shape on purpose so the Rust view
// component can copy that pattern verbatim:
//
//   window.FlyntFlow.mount(elementId, flowJson, {
//     readOnly: false,
//     onChange: (json) => { /* called debounced; json is a Flow body */ },
//   });
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
  Handle,
  MiniMap,
  Node,
  NodeChange,
  EdgeChange,
  Connection,
  Position,
  ReactFlowProvider,
  applyEdgeChanges,
  applyNodeChanges,
  addEdge,
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
  /** Called debounced (~500ms) after node/edge mutations. The argument
   * is a JSON-stringified `FlowJson` body — caller passes it straight
   * to `flynt_flow::parse_flow`-compatible code. */
  onChange?: (json: string) => void;
}

// ── Adapters: Flynt schema ↔ react-flow wire format ─────────────────────────

// react-flow's `Node<T>` constrains `T extends Record<string, unknown>`,
// so we intersect with that index signature. The named fields are still
// the contract — the intersection just satisfies the type variable.
type NodePayload = {
  kind: string;
  payload: Record<string, unknown>;
  sockets: SocketJson[];
} & Record<string, unknown>;

// Defensive: agents (Phase 4) may send partial nodes — missing position,
// missing data, missing sockets. We default rather than throw so a
// malformed flow renders as best-effort rather than crashing the view.
function toRfNode(n: FlowNodeJson): Node<NodePayload> {
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
  return {
    id: e.id,
    source: e.source.node,
    target: e.target.node,
    sourceHandle: e.source.socket || undefined,
    targetHandle: e.target.socket || undefined,
  };
}

// Inverse of `toRfNode` — flatten react-flow's `{x,y}` back to our `[x,y]`,
// peel the editor-only `data.payload` wrapper off, and rebuild the original
// `data: Record<string, unknown>` payload. Idempotent: round-tripping
// through `toRfNode → fromRfNode` produces the same FlowNodeJson modulo
// numeric precision (f32 ↔ f64 noise).
function fromRfNode(n: Node<NodePayload>): FlowNodeJson {
  return {
    id: n.id,
    kind: n.data.kind,
    position: [n.position.x, n.position.y],
    data: n.data.payload,
    sockets: n.data.sockets,
  };
}

function fromRfEdge(e: Edge): FlowEdgeJson {
  return {
    id: e.id,
    source: { node: e.source, socket: e.sourceHandle ?? "" },
    target: { node: e.target, socket: e.targetHandle ?? "" },
  };
}

// UUID generator. crypto.randomUUID is available in modern WebViews
// (WKWebView 14+, WebKit2GTK 2.30+) and the wry shells we ship target
// those. Fallback uses Math.random — collision probability is negligible
// for the small graph sizes we expect (<200 nodes).
function uuid(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    const v = c === "x" ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

// ── Custom node renderer ────────────────────────────────────────────────────

function FlyntNode({ data }: { data: NodePayload }) {
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

// ── App + state management ──────────────────────────────────────────────────

function FlowApp({
  flow,
  readOnly,
  onChange,
}: {
  flow: FlowJson;
  readOnly: boolean;
  onChange?: (body: FlowJson) => void;
}) {
  // Local state seeded from the parsed flow. We don't keep `flow` itself
  // as state because react-flow operates on its own typed structures —
  // we round-trip into our schema only when emitting changes.
  const [nodes, setNodes] = React.useState<Node<NodePayload>[]>(() =>
    flow.nodes.map(toRfNode)
  );
  const [edges, setEdges] = React.useState<Edge[]>(() => flow.edges.map(toRfEdge));

  // Keep a ref to the current state so the debounced emitter doesn't
  // capture stale closures. React's setState batching makes "read latest
  // after change" tricky without this.
  const latestRef = React.useRef({ nodes, edges, meta: flow.meta ?? {} });
  latestRef.current = { nodes, edges, meta: flow.meta ?? {} };

  // Debounced change emit. 500ms matches the Excalidraw save cadence —
  // long enough to coalesce a drag, short enough to feel snappy on
  // discrete edits. Cmd+S triggers an immediate flush via flushEmit.
  const emitTimerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  const flushEmit = React.useCallback(() => {
    if (!onChange) return;
    if (emitTimerRef.current) {
      clearTimeout(emitTimerRef.current);
      emitTimerRef.current = null;
    }
    const { nodes, edges, meta } = latestRef.current;
    const body: FlowJson = {
      meta,
      nodes: nodes.map(fromRfNode),
      edges: edges.map(fromRfEdge),
    };
    // Guard host crashes — if onChange (the Rust bridge wrapper) throws
    // because the host is mid-unmount or the queue isn't writable, we
    // log and keep the editor responsive rather than letting an
    // uncaught throw kill the React tree.
    try {
      onChange(body);
    } catch (err) {
      console.error("[FlyntFlow] onChange threw", err);
    }
  }, [onChange]);

  const scheduleEmit = React.useCallback(() => {
    if (!onChange) return;
    if (emitTimerRef.current) clearTimeout(emitTimerRef.current);
    emitTimerRef.current = setTimeout(flushEmit, 500);
  }, [onChange, flushEmit]);

  // Cmd+S / Ctrl+S → immediate flush. Mirrors the Excalidraw keybind
  // so muscle memory carries across views.
  React.useEffect(() => {
    if (readOnly) return;
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        flushEmit();
      }
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [readOnly, flushEmit]);

  // Flush any pending debounced change on unmount so the operator's last
  // drag/edit isn't lost when navigating away mid-debounce.
  React.useEffect(() => {
    return () => {
      if (emitTimerRef.current) {
        clearTimeout(emitTimerRef.current);
        emitTimerRef.current = null;
        // Best-effort flush — onChange may already be unbound by the
        // host but the call is cheap.
        flushEmit();
      }
    };
  }, [flushEmit]);

  // Node changes: position drags, selection, dimensions. Selection-only
  // changes don't dirty the file — filter those out so we don't flood
  // disk with no-op writes when the operator clicks around.
  const onNodesChange = React.useCallback(
    (changes: NodeChange[]) => {
      setNodes((ns) => applyNodeChanges(changes, ns) as Node<NodePayload>[]);
      const dirty = changes.some(
        (c) => c.type !== "select" && c.type !== "dimensions"
      );
      if (dirty) scheduleEmit();
    },
    [scheduleEmit]
  );

  const onEdgesChange = React.useCallback(
    (changes: EdgeChange[]) => {
      setEdges((es) => applyEdgeChanges(changes, es));
      const dirty = changes.some((c) => c.type !== "select");
      if (dirty) scheduleEmit();
    },
    [scheduleEmit]
  );

  const onConnect = React.useCallback(
    (conn: Connection) => {
      // react-flow generates edges without ids; we stamp a UUID so the
      // schema's id contract is satisfied and round-trips remain stable.
      const e: Edge = {
        id: uuid(),
        source: conn.source!,
        target: conn.target!,
        sourceHandle: conn.sourceHandle ?? undefined,
        targetHandle: conn.targetHandle ?? undefined,
      };
      setEdges((es) => addEdge(e, es));
      scheduleEmit();
    },
    [scheduleEmit]
  );

  return (
    <div style={{ width: "100%", height: "100%" }}>
      <ReactFlowProvider>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={NODE_TYPES}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          nodesDraggable={!readOnly}
          nodesConnectable={!readOnly}
          edgesFocusable={!readOnly}
          elementsSelectable
          fitView
          deleteKeyCode={readOnly ? null : ["Backspace", "Delete"]}
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
    parsed.nodes = Array.isArray(parsed.nodes) ? parsed.nodes : [];
    parsed.edges = Array.isArray(parsed.edges) ? parsed.edges : [];
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
    const onChangeWrapper = options.onChange
      ? (body: FlowJson) => options.onChange!(JSON.stringify(body))
      : undefined;
    api._root.render(
      <FlowApp
        flow={parsed}
        readOnly={options.readOnly ?? false}
        onChange={onChangeWrapper}
      />
    );
  },
  unmount() {
    if (api._root) {
      try { api._root.unmount(); } catch { /* ignore */ }
      api._root = null;
    }
  },
};

window.FlyntFlow = api;
