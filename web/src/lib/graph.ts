import { MarkerType, type Edge, type Node } from "@xyflow/react";

import type {
  DependenciesResponse,
  DependencyNode,
  SymbolRecord,
} from "./api/types";
import { dependencyBuckets } from "./presenters";

export type DependencyFlowNodeData = {
  title: string;
  subtitle: string;
  depth: number;
  edgeKind: string;
  variant: "focus" | "incoming" | "outgoing";
  selector: string;
};

export type DependencyGraphNode = Node<DependencyFlowNodeData, "dependencyNode">;

export function buildDependencyFlow(
  symbol: SymbolRecord | null,
  dependencies?: DependenciesResponse,
): {
  nodes: DependencyGraphNode[];
  edges: Edge[];
} {
  if (!symbol) {
    return { nodes: [], edges: [] };
  }

  const nodes: DependencyGraphNode[] = [
    {
      id: symbol.qualified_name,
      type: "dependencyNode",
      position: { x: 0, y: 0 },
      data: {
        title: symbol.name,
        subtitle: `${symbol.kind} · ${symbol.file_path}`,
        depth: 0,
        edgeKind: "focus",
        variant: "focus",
        selector: symbol.qualified_name,
      },
    },
  ];

  const edges: Edge[] = [];
  const seen = new Set([symbol.qualified_name]);
  const buckets = dependencyBuckets(dependencies);

  appendDirectionNodes(symbol, buckets.outgoing, "outgoing", nodes, edges, seen);
  appendDirectionNodes(symbol, buckets.incoming, "incoming", nodes, edges, seen);

  return { nodes, edges };
}

function appendDirectionNodes(
  center: SymbolRecord,
  entries: DependencyNode[],
  direction: "incoming" | "outgoing",
  nodes: DependencyGraphNode[],
  edges: Edge[],
  seen: Set<string>,
): void {
  const groups = new Map<number, DependencyNode[]>();

  for (const entry of entries) {
    if (!groups.has(entry.depth)) {
      groups.set(entry.depth, []);
    }
    groups.get(entry.depth)!.push(entry);
  }

  for (const depth of Array.from(groups.keys()).sort((left, right) => left - right)) {
    const group = groups.get(depth)!;

    group.forEach((entry, index) => {
      if (seen.has(entry.qualified_name)) {
        return;
      }

      seen.add(entry.qualified_name);

      const spread = 154;
      const y = (index - (group.length - 1) / 2) * spread;
      const x = (direction === "incoming" ? -1 : 1) * (270 + (depth - 1) * 228);
      const stroke = direction === "incoming" ? "#d97706" : "#0f9d8a";
      const label = entry.dep_kind ? `${entry.dep_kind} · d${entry.depth}` : `d${entry.depth}`;

      nodes.push({
        id: entry.qualified_name,
        type: "dependencyNode",
        position: { x, y },
        data: {
          title: entry.name,
          subtitle: entry.file_path,
          depth: entry.depth,
          edgeKind: label,
          variant: direction,
          selector: entry.qualified_name,
        },
      });

      edges.push({
        id: `${direction}:${entry.qualified_name}`,
        source: direction === "incoming" ? entry.qualified_name : center.qualified_name,
        target: direction === "incoming" ? center.qualified_name : entry.qualified_name,
        type: "smoothstep",
        label,
        animated: entry.depth === 1,
        markerEnd: {
          type: MarkerType.ArrowClosed,
          color: stroke,
          width: 16,
          height: 16,
        },
        style: {
          stroke,
          strokeWidth: 1.8,
          strokeDasharray: entry.depth > 1 ? "6 5" : undefined,
        },
        labelStyle: {
          fill: "#d6e4e1",
          fontSize: 11,
          fontWeight: 600,
        },
      });
    });
  }
}
