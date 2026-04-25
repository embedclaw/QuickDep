export type ProjectTarget = {
  project_id?: string;
  path?: string;
};

export type ProjectState =
  | "NotLoaded"
  | {
      Loading: {
        total_files: number;
        scanned_files: number;
        current_file?: string | null;
        started_at: number;
      };
    }
  | {
      Loaded: {
        file_count: number;
        symbol_count: number;
        dependency_count: number;
        loaded_at: number;
        watching: boolean;
      };
    }
  | {
      WatchPaused: {
        file_count: number;
        symbol_count: number;
        dependency_count: number;
        paused_at: number;
        reason: string;
      };
    }
  | {
      Failed: {
        error: string;
        failed_at: number;
      };
    };

export type ProjectRecord = {
  id: string;
  name: string;
  path: string;
  state: ProjectState;
  is_default: boolean;
};

export type ProjectsResponse = {
  default_project_id?: string;
  projects: ProjectRecord[];
};

export type ScanProjectResponse = {
  project: ProjectRecord;
  rebuild: boolean;
  stats?: {
    symbols?: number;
    dependencies?: number;
    imports?: number;
    files?: number;
  };
};

export type SymbolRecord = {
  id: string;
  name: string;
  qualified_name: string;
  kind: string;
  file_path: string;
  line: number;
  column: number;
  visibility: string;
  signature?: string | null;
  source: string;
};

export type SearchInterfacesResponse = {
  query: string;
  limit: number;
  interfaces: SymbolRecord[];
};

export type InterfaceDetailResponse = {
  interface: SymbolRecord;
};

export type DependencyDirection = "incoming" | "outgoing" | "both";

export type DependencyNode = {
  symbol_id: string;
  name: string;
  qualified_name: string;
  file_path: string;
  depth: number;
  dep_kind?: string | null;
};

export type DependenciesResponse = {
  interface: SymbolRecord;
  direction: DependencyDirection;
  max_depth: number;
  dependencies?: DependencyNode[];
  outgoing?: DependencyNode[];
  incoming?: DependencyNode[];
};

export type FileInterfacesResponse = {
  file_path: string;
  interfaces: SymbolRecord[];
};

export type ProjectOverviewNode = {
  id: string;
  name: string;
  qualified_name: string;
  kind: string;
  file_path: string;
  line: number;
  column: number;
  visibility: string;
  source: string;
  incoming_count: number;
  outgoing_count: number;
  degree: number;
};

export type ProjectOverviewEdge = {
  id: string;
  source: string;
  target: string;
  weight: number;
  kinds: string[];
};

export type ProjectOverviewPayload = {
  total_symbols: number;
  total_edges: number;
  displayed_symbols: number;
  displayed_edges: number;
  hidden_symbols: number;
  max_symbols: number;
  max_edges: number;
  nodes: ProjectOverviewNode[];
  edges: ProjectOverviewEdge[];
};

export type ProjectOverviewResponse = {
  project: ProjectRecord;
  overview: ProjectOverviewPayload;
};

export type HealthResponse = {
  status: string;
};

export type ErrorResponse = {
  error?: {
    code?: number;
    message?: string;
    data?: unknown;
  };
};

export type ProjectStatusSocketMessage =
  | {
      type: "status";
      data: {
        project: ProjectRecord;
      };
    }
  | {
      type: "error";
      error: {
        code?: number;
        message?: string;
        data?: unknown;
      };
    };
