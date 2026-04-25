import { forceCenter, forceCollide, forceLink, forceManyBody, forceSimulation } from "d3-force";
import { useEffect, useMemo, useRef, useState } from "react";

import { useSvgPanZoom } from "../hooks/useSvgPanZoom";
import type { DependenciesResponse, SymbolRecord } from "../lib/api/types";
import { type Locale } from "../lib/i18n";
import { compactPath, dependencyBuckets, presentDependencyKind } from "../lib/presenters";

type DependencyGraphViewProps = {
  locale: Locale;
  symbol: SymbolRecord | null;
  dependencies?: DependenciesResponse;
  onSelectInterface: (qualifiedName: string) => void;
};

type DirectionVariant = "focus" | "incoming" | "outgoing" | "both";

type GraphNode = {
  id: string;
  name: string;
  qualifiedName: string;
  filePath: string;
  direction: DirectionVariant;
  depth: number;
  size: number;
  x: number;
  y: number;
  fill: string;
  stroke: string;
  textColor: string;
  relationLabel: string;
};

type GraphEdge = {
  id: string;
  source: string;
  target: string;
  direction: "incoming" | "outgoing";
  depth: number;
  path: string;
  stroke: string;
  strokeWidth: number;
  opacity: number;
  dasharray?: string;
};

const FALLBACK_WIDTH = 1280;
const FALLBACK_HEIGHT = 900;

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
        width: Math.max(360, Math.round(entry.contentRect.width)),
        height: Math.max(360, Math.round(entry.contentRect.height)),
      });
    });

    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return { ref, size };
}

function shortLabel(value: string, maxLength: number): string {
  if (value.length <= maxLength) {
    return value;
  }

  return `${value.slice(0, Math.max(1, maxLength - 1))}…`;
}

function hashValue(value: string): number {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 31 + value.charCodeAt(index)) >>> 0;
  }

  return hash;
}

function nodeStyle(
  direction: DirectionVariant,
  depth: number,
): Pick<GraphNode, "size" | "fill" | "stroke" | "textColor"> {
  if (direction === "focus") {
    return {
      size: 104,
      fill: "#111111",
      stroke: "#111111",
      textColor: "#ffffff",
    };
  }

  if (direction === "both") {
    const size = Math.max(48, 74 - (depth - 1) * 8);
    return {
      size,
      fill: "#3b3b3b",
      stroke: "#111111",
      textColor: "#ffffff",
    };
  }

  const size = Math.max(44, (direction === "outgoing" ? 76 : 70) - (depth - 1) * 8);
  const lightness = direction === "outgoing" ? 82 - (depth - 1) * 7 : 90 - (depth - 1) * 6;

  return {
    size,
    fill: `hsl(0 0% ${lightness}%)`,
    stroke: direction === "outgoing" ? "#1c1c1c" : "#7f7f7f",
    textColor: lightness < 58 ? "#ffffff" : "#111111",
  };
}

function edgeStyle(
  direction: "incoming" | "outgoing",
  depth: number,
): Pick<GraphEdge, "stroke" | "strokeWidth" | "opacity" | "dasharray"> {
  const direct = depth <= 1;

  if (direction === "outgoing") {
    return {
      stroke: direct ? "#2c2c2c" : "#767676",
      strokeWidth: direct ? 1.9 : 1.45,
      opacity: direct ? 0.84 : 0.54,
      dasharray: direct ? undefined : "5 7",
    };
  }

  return {
    stroke: direct ? "#8d8d8d" : "#b7b7b7",
    strokeWidth: direct ? 1.7 : 1.3,
    opacity: direct ? 0.74 : 0.46,
    dasharray: direct ? undefined : "5 7",
  };
}

function buildCurve(
  sourceX: number,
  sourceY: number,
  targetX: number,
  targetY: number,
  bend: number,
): string {
  const midX = (sourceX + targetX) / 2;
  const midY = (sourceY + targetY) / 2;
  const dx = targetX - sourceX;
  const dy = targetY - sourceY;
  const length = Math.max(1, Math.hypot(dx, dy));
  const normalX = -dy / length;
  const normalY = dx / length;
  const controlX = midX + normalX * bend;
  const controlY = midY + normalY * bend;

  return `M ${sourceX} ${sourceY} Q ${controlX} ${controlY} ${targetX} ${targetY}`;
}

function buildGraph(
  symbol: SymbolRecord,
  dependencies: DependenciesResponse | undefined,
  width: number,
  height: number,
  locale: Locale,
) {
  const focusX = width * 0.2;
  const focusY = height * 0.2;
  type MergedNode = {
    id: string;
    name: string;
    qualifiedName: string;
    filePath: string;
    incomingDepth?: number;
    outgoingDepth?: number;
    incomingKind?: string | null;
    outgoingKind?: string | null;
  };

  const merged = new Map<string, MergedNode>();
  const buckets = dependencyBuckets(dependencies);

  for (const entry of buckets.incoming) {
    const current: MergedNode = merged.get(entry.qualified_name) ?? {
      id: entry.symbol_id,
      name: entry.name,
      qualifiedName: entry.qualified_name,
      filePath: entry.file_path,
    };
    current.incomingDepth = Math.min(current.incomingDepth ?? entry.depth, entry.depth);
    current.incomingKind = current.incomingKind ?? entry.dep_kind;
    merged.set(entry.qualified_name, current);
  }

  for (const entry of buckets.outgoing) {
    const current: MergedNode = merged.get(entry.qualified_name) ?? {
      id: entry.symbol_id,
      name: entry.name,
      qualifiedName: entry.qualified_name,
      filePath: entry.file_path,
    };
    current.outgoingDepth = Math.min(current.outgoingDepth ?? entry.depth, entry.depth);
    current.outgoingKind = current.outgoingKind ?? entry.dep_kind;
    merged.set(entry.qualified_name, current);
  }

  const nodes: GraphNode[] = [
    {
      id: symbol.id,
      name: symbol.name,
      qualifiedName: symbol.qualified_name,
      filePath: symbol.file_path,
      direction: "focus",
      depth: 0,
      relationLabel: locale === "zh-CN" ? "当前接口" : "focus",
      x: focusX,
      y: focusY,
      ...nodeStyle("focus", 0),
    },
  ];

  const mergedItems = [...merged.values()].sort((left, right) => {
    const leftDepth = Math.min(left.incomingDepth ?? 9, left.outgoingDepth ?? 9);
    const rightDepth = Math.min(right.incomingDepth ?? 9, right.outgoingDepth ?? 9);
    return leftDepth - rightDepth || left.name.localeCompare(right.name);
  });

  mergedItems.forEach((item, index) => {
    const direction: DirectionVariant =
      item.incomingDepth !== undefined && item.outgoingDepth !== undefined
        ? "both"
        : item.incomingDepth !== undefined
          ? "incoming"
          : "outgoing";
    const depth =
      direction === "both"
        ? Math.min(item.incomingDepth ?? 1, item.outgoingDepth ?? 1)
        : direction === "incoming"
          ? (item.incomingDepth ?? 1)
          : (item.outgoingDepth ?? 1);
    const sectorBase =
      direction === "incoming"
        ? Math.PI * 1.05
        : direction === "outgoing"
          ? -Math.PI * 0.05
          : -Math.PI / 2;
    const sectorSpan =
      direction === "both"
        ? Math.PI * 0.9
        : Math.PI * 0.96;
    const phase = ((hashValue(item.qualifiedName) % 1000) / 1000 - 0.5) * 0.22;
    const angle =
      sectorBase +
      ((index + 0.5) / Math.max(1, mergedItems.length)) * sectorSpan +
      phase;
    const ring = 192 + (depth - 1) * 72 + (hashValue(item.id) % 24);
    const x = focusX + Math.cos(angle) * ring;
    const y = focusY + Math.sin(angle) * ring * 0.78;
    const relationLabel =
      direction === "both"
        ? locale === "zh-CN"
          ? "双向依赖"
          : "bidirectional"
        : direction === "incoming"
          ? item.incomingKind
            ? `${presentDependencyKind(item.incomingKind, locale)} · ${locale === "zh-CN" ? "流入" : "incoming"}`
            : locale === "zh-CN"
              ? "流入"
              : "incoming"
          : item.outgoingKind
            ? `${presentDependencyKind(item.outgoingKind, locale)} · ${locale === "zh-CN" ? "流出" : "outgoing"}`
            : locale === "zh-CN"
              ? "流出"
              : "outgoing";

    nodes.push({
      id: item.id,
      name: item.name,
      qualifiedName: item.qualifiedName,
      filePath: item.filePath,
      direction,
      depth,
      relationLabel,
      x,
      y,
      ...nodeStyle(direction, depth),
    });
  });

  const simulationNodes = nodes.map((node, index) =>
    index === 0
      ? { ...node, fx: focusX, fy: focusY }
      : { ...node },
  );
  const rawEdges: GraphEdge[] = [];

  for (const node of simulationNodes.slice(1)) {
    const directions =
      node.direction === "both"
        ? (["incoming", "outgoing"] as const)
        : node.direction === "incoming" || node.direction === "outgoing"
          ? ([node.direction] as const)
          : ([] as const);

    directions.forEach((direction) => {
      const style = edgeStyle(direction, node.depth);
      rawEdges.push({
        id: `${node.id}:${direction}`,
        source: direction === "incoming" ? node.id : simulationNodes[0].id,
        target: direction === "incoming" ? simulationNodes[0].id : node.id,
        direction,
        depth: node.depth,
        path: "",
        stroke: style.stroke,
        strokeWidth: style.strokeWidth,
        opacity: style.opacity,
        dasharray: style.dasharray,
      });
    });
  }

  const linkEdges = rawEdges.map((edge) => ({ ...edge }));
  const simulation = forceSimulation<GraphNode & { fx?: number; fy?: number }>(simulationNodes)
    .force(
      "link",
      forceLink<GraphNode & { fx?: number; fy?: number }, GraphEdge>(linkEdges)
        .id((node) => node.id)
        .distance((edge: GraphEdge) => 86 + edge.depth * 30)
        .strength((edge: GraphEdge) => (edge.depth <= 1 ? 0.4 : 0.2)),
    )
    .force(
      "charge",
      forceManyBody<GraphNode & { fx?: number; fy?: number }>().strength(
        (node) => -240 - node.size * 12,
      ),
    )
    .force(
      "collision",
      forceCollide<GraphNode & { fx?: number; fy?: number }>()
        .radius((node) => node.size / 2 + 20)
        .iterations(4),
    )
    .force("center", forceCenter(width * 0.58, height * 0.54));

  for (let tick = 0; tick < 320; tick += 1) {
    simulation.tick();
  }
  simulation.stop();

  const nodeById = new Map(simulationNodes.map((node) => [node.id, node]));
  const edges: GraphEdge[] = rawEdges.map((edge) => {
    const source = nodeById.get(edge.source)!;
    const target = nodeById.get(edge.target)!;
    const curve =
      edge.direction === "incoming"
        ? -20 - edge.depth * 4
        : 20 + edge.depth * 4;

    return {
      ...edge,
      path: buildCurve(source.x, source.y, target.x, target.y, curve),
    };
  });

  const adjacency = new Map<string, Set<string>>();
  for (const edge of edges) {
    if (!adjacency.has(edge.source)) {
      adjacency.set(edge.source, new Set());
    }
    if (!adjacency.has(edge.target)) {
      adjacency.set(edge.target, new Set());
    }
    adjacency.get(edge.source)!.add(edge.target);
    adjacency.get(edge.target)!.add(edge.source);
  }

  return { nodes: simulationNodes as GraphNode[], edges, nodeById, adjacency };
}

export function DependencyGraphView({
  locale,
  symbol,
  dependencies,
  onSelectInterface,
}: DependencyGraphViewProps) {
  const { ref, size } = useContainerSize();
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const panZoom = useSvgPanZoom();

  const graph = useMemo(() => {
    if (!symbol) {
      return {
        nodes: [] as GraphNode[],
        edges: [] as GraphEdge[],
        nodeById: new Map<string, GraphNode>(),
        adjacency: new Map<string, Set<string>>(),
      };
    }

    return buildGraph(symbol, dependencies, size.width, size.height, locale);
  }, [dependencies, locale, size.height, size.width, symbol]);

  if (!symbol) {
    return null;
  }

  const activeNeighbors = hoveredId ? graph.adjacency.get(hoveredId) ?? new Set<string>() : null;

  return (
    <div ref={ref} className="dependency-map">
      <svg
        className={`dependency-map__svg${panZoom.dragging ? " dependency-map__svg--dragging" : ""}`}
        viewBox={`0 0 ${size.width} ${size.height}`}
        role="img"
        aria-label={locale === "zh-CN" ? "局部依赖关系图" : "Focused dependency graph"}
        {...panZoom.svgHandlers}
      >
        <defs>
          <filter id="dependency-node-shadow" x="-30%" y="-30%" width="160%" height="160%">
            <feDropShadow dx="0" dy="3" stdDeviation="5" floodColor="#000000" floodOpacity="0.08" />
          </filter>
        </defs>

        <g transform={panZoom.transform}>
          <g className="dependency-map__edges">
            {graph.edges.map((edge) => {
              const active =
                hoveredId === null ||
                edge.source === hoveredId ||
                edge.target === hoveredId ||
                activeNeighbors?.has(edge.source) ||
                activeNeighbors?.has(edge.target);

              return (
                <g
                  key={edge.id}
                  className={`dependency-map__edge${active ? " dependency-map__edge--active" : ""}`}
                >
                  <path
                    d={edge.path}
                    fill="none"
                    stroke={edge.stroke}
                    strokeWidth={active ? edge.strokeWidth + 0.9 : edge.strokeWidth}
                    strokeOpacity={active ? Math.min(0.92, edge.opacity + 0.16) : Math.max(0.12, edge.opacity * 0.45)}
                    strokeDasharray={edge.dasharray}
                  />
                </g>
              );
            })}
          </g>

          <g className="dependency-map__nodes">
            {graph.nodes.filter((node) => node.direction !== "focus").map((node) => {
              const active =
                hoveredId === null ||
                hoveredId === node.id ||
                activeNeighbors?.has(node.id);
              const showLabel =
                node.direction === "both" ||
                node.depth <= 1 ||
                hoveredId === node.id;
              const labelLength = node.depth <= 1 ? 12 : 9;

              return (
                <g
                  key={node.id}
                  transform={`translate(${node.x}, ${node.y})`}
                  className="dependency-map__node"
                  opacity={active ? 1 : 0.24}
                  onMouseEnter={() => setHoveredId(node.id)}
                  onMouseLeave={() => setHoveredId(null)}
                  onClick={() => onSelectInterface(node.qualifiedName)}
                >
                  <title>
                    {`${node.qualifiedName}\n${compactPath(node.filePath, 4)}\n${node.relationLabel}`}
                  </title>
                  <circle
                    r={node.size / 2}
                    fill={node.fill}
                    stroke={node.stroke}
                    strokeWidth={hoveredId === node.id ? 2.8 : 1.4}
                    filter="url(#dependency-node-shadow)"
                  />
                  {showLabel ? (
                    <text
                      className="dependency-map__label"
                      textAnchor="middle"
                      dy="0.1em"
                      fill={node.textColor}
                    >
                      {hoveredId === node.id ? node.qualifiedName : shortLabel(node.name, labelLength)}
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
