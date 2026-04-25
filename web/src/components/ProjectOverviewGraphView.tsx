import { forceCenter, forceCollide, forceLink, forceManyBody, forceSimulation } from "d3-force";
import { useEffect, useMemo, useRef, useState } from "react";

import { useSvgPanZoom } from "../hooks/useSvgPanZoom";
import type { ProjectOverviewPayload } from "../lib/api/types";
import { type Locale } from "../lib/i18n";

type ProjectOverviewGraphViewProps = {
  locale: Locale;
  overview?: ProjectOverviewPayload;
  onSelectInterface: (qualifiedName: string) => void;
};

type GraphNode = {
  id: string;
  name: string;
  qualifiedName: string;
  filePath: string;
  group: string;
  degree: number;
  incomingCount: number;
  outgoingCount: number;
  radius: number;
  fill: string;
  stroke: string;
  textColor: string;
  x: number;
  y: number;
};

type GraphEdge = {
  id: string;
  source: string;
  target: string;
  weight: number;
  curve: number;
};

const FALLBACK_WIDTH = 1200;
const FALLBACK_HEIGHT = 900;

function shortLabel(value: string, maxLength: number): string {
  if (value.length <= maxLength) {
    return value;
  }

  return `${value.slice(0, Math.max(1, maxLength - 1))}…`;
}

function groupKey(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const parts = normalized.split("/").filter(Boolean);
  return parts[0] ?? "root";
}

function hashHue(value: string): number {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 31 + value.charCodeAt(index)) >>> 0;
  }

  return hash % 360;
}

function bubbleColor(group: string, intensity: number): { fill: string; stroke: string; textColor: string } {
  const normalized = Math.max(0, Math.min(1, intensity));
  const hue = hashHue(group);
  const saturation = 16 + normalized * 32;
  const lightness = 95 - normalized * 48;
  const strokeLightness = Math.max(20, lightness - 18);

  return {
    fill: `hsl(${hue} ${saturation}% ${lightness}%)`,
    stroke: `hsl(${hue} ${Math.min(72, saturation + 12)}% ${strokeLightness}%)`,
    textColor: lightness < 56 ? "#ffffff" : "#111111",
  };
}

function lineColor(weight: number): { stroke: string; opacity: number; width: number } {
  const normalized = Math.max(0, Math.min(1, weight / 6));
  return {
    stroke: `hsl(0 0% ${62 - normalized * 20}%)`,
    opacity: 0.4 + normalized * 0.22,
    width: 1.2 + normalized * 1.6,
  };
}

function edgeCurve(source: string, target: string): number {
  let hash = 0;
  const combined = `${source}:${target}`;
  for (let index = 0; index < combined.length; index += 1) {
    hash = (hash * 33 + combined.charCodeAt(index)) >>> 0;
  }

  const direction = hash % 2 === 0 ? 1 : -1;
  return direction * (18 + (hash % 24));
}

function edgePath(source: GraphNode, target: GraphNode, curve: number): string {
  const midX = (source.x + target.x) / 2;
  const midY = (source.y + target.y) / 2;
  const dx = target.x - source.x;
  const dy = target.y - source.y;
  const length = Math.max(1, Math.hypot(dx, dy));
  const normalX = -dy / length;
  const normalY = dx / length;
  const controlX = midX + normalX * curve;
  const controlY = midY + normalY * curve;

  return `M ${source.x} ${source.y} Q ${controlX} ${controlY} ${target.x} ${target.y}`;
}

function useContainerSize() {
  const ref = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ width: FALLBACK_WIDTH, height: FALLBACK_HEIGHT });

  useEffect(() => {
    const element = ref.current;
    if (!element) {
      return;
    }

    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) {
        return;
      }

      setSize({
        width: Math.max(320, Math.round(entry.contentRect.width)),
        height: Math.max(320, Math.round(entry.contentRect.height)),
      });
    });

    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return { ref, size };
}

function buildGraph(
  overview: ProjectOverviewPayload,
  width: number,
  height: number,
): {
  nodes: GraphNode[];
  edges: GraphEdge[];
} {
  const maxDegree = Math.max(...overview.nodes.map((node) => Math.max(1, node.degree)));

  const nodes: GraphNode[] = overview.nodes.map((node, index) => {
    const intensity = node.degree / maxDegree;
    const group = groupKey(node.file_path);
    const colors = bubbleColor(group, intensity);
    const radius = 14 + intensity * 44;
    const angle = (index / Math.max(1, overview.nodes.length)) * Math.PI * 2;
    const ring = 152 + (index % 7) * 42;

    return {
      id: node.id,
      name: node.name,
      qualifiedName: node.qualified_name,
      filePath: node.file_path,
      group,
      degree: node.degree,
      incomingCount: node.incoming_count,
      outgoingCount: node.outgoing_count,
      radius,
      fill: colors.fill,
      stroke: colors.stroke,
      textColor: colors.textColor,
      x: width / 2 + Math.cos(angle) * ring,
      y: height / 2 + Math.sin(angle) * ring,
    };
  });

  const edges: GraphEdge[] = overview.edges.map((edge) => ({
    id: edge.id,
    source: edge.source,
    target: edge.target,
    weight: edge.weight,
    curve: edgeCurve(edge.source, edge.target),
  }));

  const simulationEdges = edges.map((edge) => ({ ...edge }));

  const simulation = forceSimulation<GraphNode>(nodes)
    .force(
      "link",
      forceLink<GraphNode, GraphEdge>(simulationEdges)
        .id((node: GraphNode) => node.id)
        .distance((edge: GraphEdge) => 72 + Math.max(0, 112 - edge.weight * 8))
        .strength((edge: GraphEdge) => Math.min(0.72, 0.14 + edge.weight * 0.05)),
    )
    .force(
      "charge",
      forceManyBody<GraphNode>().strength((node: GraphNode) => -180 - node.radius * 18),
    )
    .force("center", forceCenter(width / 2, height / 2))
    .force(
      "collision",
      forceCollide<GraphNode>().radius((node: GraphNode) => node.radius + 18).iterations(4),
    );

  for (let tick = 0; tick < 320; tick += 1) {
    simulation.tick();
  }
  simulation.stop();

  return { nodes, edges };
}

export function ProjectOverviewGraphView({
  locale,
  overview,
  onSelectInterface,
}: ProjectOverviewGraphViewProps) {
  const { ref, size } = useContainerSize();
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const panZoom = useSvgPanZoom();

  const graph = useMemo(() => {
    if (!overview?.nodes.length) {
      return {
        nodes: [] as GraphNode[],
        edges: [] as GraphEdge[],
        adjacency: new Map<string, Set<string>>(),
        nodeById: new Map<string, GraphNode>(),
        labeledIds: new Set<string>(),
      };
    }

    const built = buildGraph(overview, size.width, size.height);
    const adjacency = new Map<string, Set<string>>();
    const nodeById = new Map<string, GraphNode>(built.nodes.map((node) => [node.id, node]));
    const labeledIds = new Set(
      [...built.nodes]
        .sort((left, right) => right.degree - left.degree)
        .slice(0, Math.min(18, Math.max(8, Math.floor(built.nodes.length * 0.12))))
        .map((node) => node.id),
    );

    for (const edge of built.edges) {
      if (!adjacency.has(edge.source)) {
        adjacency.set(edge.source, new Set());
      }
      if (!adjacency.has(edge.target)) {
        adjacency.set(edge.target, new Set());
      }
      adjacency.get(edge.source)!.add(edge.target);
      adjacency.get(edge.target)!.add(edge.source);
    }

    return { ...built, adjacency, nodeById, labeledIds };
  }, [overview, size.height, size.width]);

  if (!overview?.nodes.length) {
    return null;
  }

  const activeNeighbors = hoveredId ? graph.adjacency.get(hoveredId) ?? new Set<string>() : null;

  return (
    <div ref={ref} className="graph-cloud">
      <svg
        className={`graph-cloud__svg${panZoom.dragging ? " graph-cloud__svg--dragging" : ""}`}
        viewBox={`0 0 ${size.width} ${size.height}`}
        role="img"
        aria-label={locale === "zh-CN" ? "项目依赖云图" : "Project dependency graph"}
        {...panZoom.svgHandlers}
      >
        <defs>
          <filter id="graph-node-shadow" x="-30%" y="-30%" width="160%" height="160%">
            <feDropShadow dx="0" dy="3" stdDeviation="5" floodColor="#000000" floodOpacity="0.08" />
          </filter>
        </defs>

        <g transform={panZoom.transform}>
          <g className="graph-cloud__edges">
            {graph.edges.map((edge) => {
              const source = graph.nodeById.get(edge.source);
              const target = graph.nodeById.get(edge.target);
              if (!source || !target) {
                return null;
              }

              const active =
                hoveredId === null ||
                edge.source === hoveredId ||
                edge.target === hoveredId ||
                activeNeighbors?.has(edge.source) ||
                activeNeighbors?.has(edge.target);
              const line = lineColor(edge.weight);

              return (
                <g
                  key={edge.id}
                  className={`graph-cloud__edge${active ? " graph-cloud__edge--active" : ""}`}
                >
                  <path
                    d={edgePath(source, target, edge.curve)}
                    fill="none"
                    stroke={line.stroke}
                    strokeOpacity={active ? Math.min(0.9, line.opacity + 0.18) : Math.max(0.12, line.opacity * 0.48)}
                    strokeWidth={active ? line.width + 1.1 : line.width}
                  />
                </g>
              );
            })}
          </g>

          <g className="graph-cloud__nodes">
            {graph.nodes.map((node) => {
              const active =
                hoveredId === null ||
                hoveredId === node.id ||
                activeNeighbors?.has(node.id);
              const showLabel = hoveredId === node.id || graph.labeledIds.has(node.id);
              const label =
                hoveredId === node.id
                  ? node.qualifiedName
                  : node.radius >= 34
                    ? shortLabel(node.name, 10)
                    : shortLabel(node.name, 2);

              return (
                <g
                  key={node.id}
                  transform={`translate(${node.x}, ${node.y})`}
                  className="graph-cloud__node"
                  opacity={active ? 1 : 0.22}
                  onMouseEnter={() => setHoveredId(node.id)}
                  onMouseLeave={() => setHoveredId(null)}
                  onClick={() => onSelectInterface(node.qualifiedName)}
                >
                  <title>
                    {`${node.qualifiedName}\n${node.filePath}\n${
                      locale === "zh-CN" ? "流入" : "Incoming"
                    } ${node.incomingCount} · ${
                      locale === "zh-CN" ? "流出" : "Outgoing"
                    } ${node.outgoingCount}`}
                  </title>
                  <circle
                    r={node.radius}
                    fill={node.fill}
                    stroke={node.stroke}
                    strokeWidth={hoveredId === node.id ? 2.6 : 1.2}
                    filter="url(#graph-node-shadow)"
                  />
                  {showLabel ? (
                    <text
                      className="graph-cloud__label"
                      textAnchor="middle"
                      dy="0.35em"
                      fill={node.textColor}
                      fontSize={hoveredId === node.id ? Math.max(10, Math.min(14, node.radius * 0.3)) : Math.max(8, Math.min(11, node.radius * 0.24))}
                    >
                      {label}
                    </text>
                  ) : null}
                </g>
              );
            })}
          </g>
        </g>
      </svg>
      <div className="graph-toolbar graph-toolbar--bottom-right">
        <button type="button" className="graph-toolbar__button" onClick={() => panZoom.zoomBy(0.18)}>
          +
        </button>
        <button type="button" className="graph-toolbar__button" onClick={() => panZoom.zoomBy(-0.18)}>
          -
        </button>
        <button type="button" className="graph-toolbar__button graph-toolbar__button--wide" onClick={panZoom.reset}>
          {locale === "zh-CN" ? "还原" : "Reset"}
        </button>
      </div>
    </div>
  );
}
