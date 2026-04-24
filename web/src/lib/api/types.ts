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

export type ProjectStatusResponse = {
  project: ProjectRecord;
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

export type CallChainResponse = {
  from: SymbolRecord;
  to: SymbolRecord;
  max_depth: number;
  path: SymbolRecord[];
};

export type FileInterfacesResponse = {
  file_path: string;
  interfaces: SymbolRecord[];
};

export type BatchQueryKind =
  | "find_interfaces"
  | "get_interface"
  | "get_dependencies"
  | "get_call_chain"
  | "get_file_interfaces";

export type BatchQueryItem = {
  kind: BatchQueryKind;
  query?: string;
  interface?: string;
  file_path?: string;
  from_interface?: string;
  to_interface?: string;
  direction?: DependencyDirection;
  limit?: number;
  max_depth?: number;
};

export type BatchQueryResponse = {
  results: Array<{
    index: number;
    kind: BatchQueryKind;
    ok: boolean;
    result?: unknown;
    error?: string;
  }>;
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
      data: ProjectStatusResponse;
    }
  | {
      type: "error";
      error: {
        code?: number;
        message?: string;
        data?: unknown;
      };
    };
