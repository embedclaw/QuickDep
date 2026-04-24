import type {
  BatchQueryItem,
  BatchQueryResponse,
  CallChainResponse,
  DependenciesResponse,
  ErrorResponse,
  FileInterfacesResponse,
  HealthResponse,
  InterfaceDetailResponse,
  ProjectRecord,
  ProjectsResponse,
  ProjectTarget,
  ScanProjectResponse,
  SearchInterfacesResponse,
} from "./types";

function trimBaseUrl(baseUrl: string): string {
  return baseUrl.replace(/\/+$/, "");
}

export function resolveDefaultBaseUrl(): string {
  const envBase = import.meta.env.VITE_QUICKDEP_BASE_URL?.trim();

  if (envBase) {
    return trimBaseUrl(envBase);
  }

  if (typeof window === "undefined") {
    return "http://127.0.0.1:8080";
  }

  const url = new URL(window.location.href);
  if (url.port === "4173" || url.port === "5173" || url.port === "3000") {
    return "http://127.0.0.1:8080";
  }

  return trimBaseUrl(url.origin);
}

export function toWebSocketUrl(baseUrl: string): string {
  const url = new URL(trimBaseUrl(baseUrl));
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return trimBaseUrl(url.toString());
}

async function requestJson<T>(
  baseUrl: string,
  path: string,
  init?: RequestInit,
): Promise<T> {
  const response = await fetch(`${trimBaseUrl(baseUrl)}${path}`, {
    ...init,
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
      ...init?.headers,
    },
  });

  if (!response.ok) {
    let errorMessage = `${response.status} ${response.statusText}`;

    try {
      const errorPayload = (await response.json()) as ErrorResponse;
      errorMessage = errorPayload.error?.message ?? errorMessage;
    } catch {
      // Ignore malformed error payloads and fall back to status text.
    }

    throw new Error(errorMessage);
  }

  return (await response.json()) as T;
}

export function projectTarget(project: ProjectRecord | null): ProjectTarget {
  if (!project) {
    return {};
  }

  return { project_id: project.id };
}

export const quickdepApi = {
  health(baseUrl: string) {
    return requestJson<HealthResponse>(baseUrl, "/health", {
      method: "GET",
    });
  },

  listProjects(baseUrl: string) {
    return requestJson<ProjectsResponse>(baseUrl, "/api/projects", {
      method: "GET",
    });
  },

  scanProject(
    baseUrl: string,
    request: {
      project?: ProjectTarget;
      rebuild?: boolean;
    },
  ) {
    return requestJson<ScanProjectResponse>(baseUrl, "/api/projects/scan", {
      method: "POST",
      body: JSON.stringify(request),
    });
  },

  rebuildDatabase(
    baseUrl: string,
    request: {
      project?: ProjectTarget;
    },
  ) {
    return requestJson<ScanProjectResponse>(baseUrl, "/api/projects/rebuild", {
      method: "POST",
      body: JSON.stringify(request),
    });
  },

  findInterfaces(
    baseUrl: string,
    request: {
      project?: ProjectTarget;
      query: string;
      limit?: number;
    },
  ) {
    return requestJson<SearchInterfacesResponse>(
      baseUrl,
      "/api/interfaces/search",
      {
        method: "POST",
        body: JSON.stringify(request),
      },
    );
  },

  getInterface(
    baseUrl: string,
    request: {
      project?: ProjectTarget;
      interface: string;
    },
  ) {
    return requestJson<InterfaceDetailResponse>(
      baseUrl,
      "/api/interfaces/detail",
      {
        method: "POST",
        body: JSON.stringify(request),
      },
    );
  },

  getDependencies(
    baseUrl: string,
    request: {
      project?: ProjectTarget;
      interface: string;
      direction?: string;
      max_depth?: number;
    },
  ) {
    return requestJson<DependenciesResponse>(baseUrl, "/api/dependencies", {
      method: "POST",
      body: JSON.stringify(request),
    });
  },

  getCallChain(
    baseUrl: string,
    request: {
      project?: ProjectTarget;
      from_interface: string;
      to_interface: string;
      max_depth?: number;
    },
  ) {
    return requestJson<CallChainResponse>(baseUrl, "/api/call-chain", {
      method: "POST",
      body: JSON.stringify(request),
    });
  },

  getFileInterfaces(
    baseUrl: string,
    request: {
      project?: ProjectTarget;
      file_path: string;
    },
  ) {
    return requestJson<FileInterfacesResponse>(
      baseUrl,
      "/api/files/interfaces",
      {
        method: "POST",
        body: JSON.stringify(request),
      },
    );
  },

  batchQuery(
    baseUrl: string,
    request: {
      project?: ProjectTarget;
      queries: BatchQueryItem[];
    },
  ) {
    return requestJson<BatchQueryResponse>(baseUrl, "/api/query/batch", {
      method: "POST",
      body: JSON.stringify(request),
    });
  },
};
