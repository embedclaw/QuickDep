import {
  Background,
  Controls,
  MiniMap,
  ReactFlow,
  type NodeProps,
} from "@xyflow/react";

import {
  buildDependencyFlow,
  type DependencyGraphNode,
} from "../lib/graph";
import type { DependenciesResponse, SymbolRecord } from "../lib/api/types";

type DependencyGraphViewProps = {
  symbol: SymbolRecord | null;
  dependencies?: DependenciesResponse;
  onSelectInterface: (qualifiedName: string) => void;
};

function DependencyNodeCard({ data }: NodeProps<DependencyGraphNode>) {
  return (
    <div className={`dependency-node dependency-node--${data.variant}`}>
      <span className="dependency-node__meta">
        {data.depth === 0 ? "focus" : `depth ${data.depth}`}
      </span>
      <strong className="dependency-node__title">{data.title}</strong>
      <span className="dependency-node__subtitle">{data.subtitle}</span>
      <span className="dependency-node__edge">{data.edgeKind}</span>
    </div>
  );
}

const nodeTypes = {
  dependencyNode: DependencyNodeCard,
};

export function DependencyGraphView({
  symbol,
  dependencies,
  onSelectInterface,
}: DependencyGraphViewProps) {
  const flow = buildDependencyFlow(symbol, dependencies);

  if (!symbol) {
    return (
      <div className="empty-state empty-state--graph">
        <p className="empty-state__eyebrow">Select a symbol</p>
        <h3>Dependency neighborhood will appear here.</h3>
        <p>
          Search a function, class, trait, or file-level interface to render the
          layered graph.
        </p>
      </div>
    );
  }

  return (
    <div className="graph-shell">
      <div className="graph-shell__caption">
        <span>Layered neighborhood view</span>
        <span>Returned nodes are anchored to the focus symbol by depth.</span>
      </div>
      <ReactFlow<DependencyGraphNode>
        fitView
        nodes={flow.nodes}
        edges={flow.edges}
        nodeTypes={nodeTypes}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable
        onNodeClick={(_, node) => {
          const selector = node.data?.selector;
          if (selector) {
            onSelectInterface(selector);
          }
        }}
      >
        <MiniMap pannable zoomable />
        <Controls position="bottom-left" />
        <Background gap={24} size={1.2} color="#1b3d3b" />
      </ReactFlow>
    </div>
  );
}
