import type {
  DependenciesResponse,
  DependencyNode,
  ProjectState,
  SymbolRecord,
} from "./api/types";

export type Tone = "neutral" | "good" | "info" | "warn" | "danger";

export function summarizeProjectState(state: ProjectState): {
  label: string;
  tone: Tone;
  detail: string;
  stats?: {
    files?: number;
    symbols?: number;
    dependencies?: number;
  };
} {
  if (state === "NotLoaded") {
    return {
      label: "Not Loaded",
      tone: "neutral",
      detail: "Project is registered but has not been scanned yet.",
    };
  }

  if ("Loading" in state) {
    const payload = state.Loading;
    return {
      label: "Loading",
      tone: "info",
      detail: `${payload.scanned_files}/${payload.total_files} files`,
    };
  }

  if ("Loaded" in state) {
    const payload = state.Loaded;
    return {
      label: payload.watching ? "Live" : "Loaded",
      tone: "good",
      detail: `Loaded ${formatTimestamp(payload.loaded_at)}`,
      stats: {
        files: payload.file_count,
        symbols: payload.symbol_count,
        dependencies: payload.dependency_count,
      },
    };
  }

  if ("WatchPaused" in state) {
    const payload = state.WatchPaused;
    return {
      label: "Watch Paused",
      tone: "warn",
      detail: payload.reason,
      stats: {
        files: payload.file_count,
        symbols: payload.symbol_count,
        dependencies: payload.dependency_count,
      },
    };
  }

  const payload = state.Failed;
  return {
    label: "Failed",
    tone: "danger",
    detail: payload.error,
  };
}

export function formatTimestamp(unixSeconds?: number): string {
  if (!unixSeconds) {
    return "n/a";
  }

  const date = new Date(unixSeconds * 1000);
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

export function compactPath(path: string, keep = 3): string {
  const parts = path.split("/").filter(Boolean);
  if (parts.length <= keep) {
    return path;
  }

  return `.../${parts.slice(-keep).join("/")}`;
}

export function dependencyBuckets(data?: DependenciesResponse): {
  incoming: DependencyNode[];
  outgoing: DependencyNode[];
} {
  if (!data) {
    return { incoming: [], outgoing: [] };
  }

  if (data.direction === "incoming") {
    return { incoming: data.dependencies ?? [], outgoing: [] };
  }

  if (data.direction === "outgoing") {
    return { incoming: [], outgoing: data.dependencies ?? [] };
  }

  return {
    incoming: data.incoming ?? [],
    outgoing: data.outgoing ?? [],
  };
}

export function totalDependencies(data?: DependenciesResponse): number {
  const buckets = dependencyBuckets(data);
  return buckets.incoming.length + buckets.outgoing.length;
}

export function describeDependencyNode(node: DependencyNode): string {
  const kind = node.dep_kind ? `${node.dep_kind} / ` : "";
  return `${kind}depth ${node.depth}`;
}

export function symbolAccent(symbol: Pick<SymbolRecord, "kind" | "source">): Tone {
  if (symbol.source === "External" || symbol.source === "Builtin") {
    return "neutral";
  }

  switch (symbol.kind) {
    case "Function":
    case "Method":
      return "info";
    case "Class":
    case "Struct":
      return "good";
    case "Interface":
    case "Trait":
      return "warn";
    default:
      return "neutral";
  }
}

export function formatCount(value?: number): string {
  if (value === undefined || Number.isNaN(value)) {
    return "0";
  }

  return new Intl.NumberFormat().format(value);
}
