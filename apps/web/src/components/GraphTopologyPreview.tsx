import { useMemo } from "react";
import {
  Background,
  Handle,
  MarkerType,
  Position,
  ReactFlow,
  type Edge,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import type { GraphEdge, GraphNode } from "../types";

interface GraphTopologyPreviewProps {
  nodes: GraphNode[];
  edges: GraphEdge[];
  selectedNodeId?: string;
  onSelectNode: (id: string) => void;
}

interface StrategyNodeData extends Record<string, unknown> {
  kind: string;
  label: string;
  detail: string;
  active: boolean;
}

const kindOrder: Record<string, number> = {
  data: 0,
  signal: 1,
  filter: 2,
  action: 3,
};

const nodeTypes = {
  strategyNode: StrategyNode,
};

function StrategyNode({ data }: NodeProps<Node<StrategyNodeData>>) {
  return (
    <button
      className={`graph-node-card ${data.kind} ${data.active ? "active" : ""}`}
      type="button"
      tabIndex={-1}
    >
      <Handle className="graph-node-handle" type="target" position={Position.Left} />
      <span className="graph-node-kind">{data.kind}</span>
      <span className="graph-node-label">{data.label}</span>
      <span className="graph-node-detail">{data.detail}</span>
      <Handle className="graph-node-handle" type="source" position={Position.Right} />
    </button>
  );
}

function graphLevels(nodes: GraphNode[], edges: GraphEdge[]) {
  const ids = new Set(nodes.map((node) => node.id));
  const incoming = new Map<string, string[]>();
  const outgoing = new Map<string, string[]>();

  nodes.forEach((node) => {
    incoming.set(node.id, []);
    outgoing.set(node.id, []);
  });

  edges.forEach((edge) => {
    if (!ids.has(edge.from) || !ids.has(edge.to)) return;
    outgoing.get(edge.from)?.push(edge.to);
    incoming.get(edge.to)?.push(edge.from);
  });

  const levels = new Map<string, number>();
  const queue = nodes
    .filter((node) => incoming.get(node.id)?.length === 0)
    .map((node) => node.id);

  queue.forEach((id) => {
    const node = nodes.find((item) => item.id === id);
    levels.set(id, kindOrder[node?.kind ?? ""] ?? 0);
  });

  while (queue.length) {
    const id = queue.shift()!;
    const nextLevel = (levels.get(id) ?? 0) + 1;
    outgoing.get(id)?.forEach((target) => {
      levels.set(target, Math.max(levels.get(target) ?? 0, nextLevel));
      queue.push(target);
    });
  }

  nodes.forEach((node) => {
    if (!levels.has(node.id)) levels.set(node.id, kindOrder[node.kind] ?? 1);
  });

  return levels;
}

export function GraphTopologyPreview({
  nodes,
  edges,
  selectedNodeId,
  onSelectNode,
}: GraphTopologyPreviewProps) {
  const visibleEdges = useMemo(() => {
    const nodeIds = new Set(nodes.map((node) => node.id));
    return edges.filter((edge) => nodeIds.has(edge.from) && nodeIds.has(edge.to));
  }, [edges, nodes]);

  const flowNodes = useMemo<Node<StrategyNodeData>[]>(() => {
    const levels = graphLevels(nodes, visibleEdges);
    const compactLevels = new Map(
      Array.from(new Set(Array.from(levels.values())))
        .sort((a, b) => a - b)
        .map((level, index) => [level, index]),
    );
    const grouped = new Map<number, GraphNode[]>();

    nodes.forEach((node) => {
      const level = compactLevels.get(levels.get(node.id) ?? 0) ?? 0;
      grouped.set(level, [...(grouped.get(level) ?? []), node]);
    });

    grouped.forEach((items) => {
      items.sort((a, b) => (kindOrder[a.kind] ?? 9) - (kindOrder[b.kind] ?? 9) || a.label.localeCompare(b.label));
    });

    return nodes.map((node) => {
      const level = compactLevels.get(levels.get(node.id) ?? 0) ?? 0;
      const levelNodes = grouped.get(level) ?? [];
      const index = Math.max(0, levelNodes.findIndex((item) => item.id === node.id));
      const columnHeight = Math.max(1, levelNodes.length);
      const y = index * 96 - ((columnHeight - 1) * 96) / 2;

      return {
        id: node.id,
        type: "strategyNode",
        position: { x: level * 220, y },
        draggable: false,
        selectable: false,
        data: {
          kind: node.kind,
          label: node.label,
          detail: node.detail,
          active: node.id === selectedNodeId,
        },
      };
    });
  }, [nodes, selectedNodeId, visibleEdges]);

  const flowEdges = useMemo<Edge[]>(
    () =>
      visibleEdges.map((edge) => ({
        id: edge.id,
        source: edge.from,
        target: edge.to,
        type: "smoothstep",
        markerEnd: { type: MarkerType.ArrowClosed, width: 14, height: 14 },
        className:
          edge.from === selectedNodeId || edge.to === selectedNodeId
            ? "graph-edge active"
            : "graph-edge",
      })),
    [selectedNodeId, visibleEdges],
  );

  return (
    <div className="graph-topology-preview" aria-label="Read-only strategy graph topology">
      <ReactFlow
        nodes={flowNodes}
        edges={flowEdges}
        nodeTypes={nodeTypes}
        fitView
        fitViewOptions={{ padding: 0.2 }}
        minZoom={0.45}
        maxZoom={1.1}
        ariaLabelConfig={{
          "node.a11yDescription.default": "Read-only strategy graph node.",
          "node.a11yDescription.keyboardDisabled": "Read-only strategy graph node.",
          "edge.a11yDescription.default": "Read-only strategy graph edge.",
          "handle.ariaLabel": "Read-only graph connection",
        }}
        nodesDraggable={false}
        nodesConnectable={false}
        nodesFocusable={false}
        edgesFocusable={false}
        disableKeyboardA11y
        elementsSelectable={false}
        deleteKeyCode={null}
        selectionKeyCode={null}
        panOnDrag={false}
        zoomOnScroll={false}
        zoomOnPinch={false}
        zoomOnDoubleClick={false}
        preventScrolling={false}
        onNodeClick={(_, node) => onSelectNode(node.id)}
      >
        <Background gap={18} size={1} />
      </ReactFlow>
    </div>
  );
}
