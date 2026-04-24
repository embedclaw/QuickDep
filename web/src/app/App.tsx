import { useDeferredValue, useEffect, useState, startTransition } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { DependencyGraphView } from "../components/DependencyGraphView";
import { Panel } from "../components/Panel";
import { StatusBadge } from "../components/StatusBadge";
import { useLocalStorage } from "../hooks/useLocalStorage";
import {
  useProjectStatusSocket,
  type SocketState,
} from "../hooks/useProjectStatusSocket";
import { projectTarget, quickdepApi, resolveDefaultBaseUrl } from "../lib/api/client";
import type {
  BatchQueryItem,
  BatchQueryKind,
  BatchQueryResponse,
  DependenciesResponse,
  DependencyDirection,
  DependencyNode,
  ProjectRecord,
  ProjectsResponse,
  SymbolRecord,
} from "../lib/api/types";
import {
  compactPath,
  describeDependencyNode,
  formatCount,
  summarizeProjectState,
  symbolAccent,
  totalDependencies,
  type Tone,
} from "../lib/presenters";

type StageTab = "graph" | "table" | "batch";
type DetailTab = "overview" | "deps" | "callchain" | "raw";

type BatchRow = {
  id: string;
  kind: BatchQueryKind;
  query: string;
  interfaceName: string;
  filePath: string;
  fromInterface: string;
  toInterface: string;
  direction: DependencyDirection;
  limit: number;
  maxDepth: number;
};

function createBatchRow(kind: BatchQueryKind = "find_interfaces"): BatchRow {
  return {
    id: crypto.randomUUID(),
    kind,
    query: "",
    interfaceName: "",
    filePath: "",
    fromInterface: "",
    toInterface: "",
    direction: "both",
    limit: 10,
    maxDepth: 3,
  };
}

function socketTone(state: SocketState): Tone {
  switch (state) {
    case "open":
      return "good";
    case "connecting":
      return "info";
    case "closed":
      return "warn";
    case "error":
      return "danger";
    default:
      return "neutral";
  }
}

function errorText(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  return "Unexpected error";
}

function stageTabLabel(tab: StageTab): string {
  switch (tab) {
    case "graph":
      return "Graph Deck";
    case "table":
      return "Tables";
    case "batch":
      return "Batch Lab";
  }
}

function detailTabLabel(tab: DetailTab): string {
  switch (tab) {
    case "overview":
      return "Overview";
    case "deps":
      return "Dependencies";
    case "callchain":
      return "Call Chain";
    case "raw":
      return "Raw";
  }
}

function symbolLabel(symbol: SymbolRecord | null, fallback: string | null): string {
  if (symbol) {
    return symbol.qualified_name;
  }

  return fallback ?? "Not set";
}

function splitDependencies(data?: DependenciesResponse): {
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

export default function App() {
  const queryClient = useQueryClient();
  const [backendUrl, setBackendUrl] = useLocalStorage<string>(
    "quickdep.web.backend-url",
    resolveDefaultBaseUrl(),
  );
  const [backendDraft, setBackendDraft] = useState(backendUrl);
  const [activeProjectId, setActiveProjectId] = useLocalStorage<string | null>(
    "quickdep.web.active-project-id",
    null,
  );
  const [scanPath, setScanPath] = useLocalStorage<string>(
    "quickdep.web.scan-path",
    "",
  );
  const [searchTerm, setSearchTerm] = useState("");
  const deferredSearch = useDeferredValue(searchTerm.trim());
  const [searchLimit, setSearchLimit] = useState(12);
  const [selectedInterfaceKey, setSelectedInterfaceKey] = useState<string | null>(
    null,
  );
  const [selectedSeed, setSelectedSeed] = useState<SymbolRecord | null>(null);
  const [direction, setDirection] = useState<DependencyDirection>("both");
  const [maxDepth, setMaxDepth] = useState(2);
  const [stageTab, setStageTab] = useState<StageTab>("graph");
  const [detailTab, setDetailTab] = useState<DetailTab>("overview");
  const [callChainSourceKey, setCallChainSourceKey] = useState<string | null>(null);
  const [callChainSourceSeed, setCallChainSourceSeed] = useState<SymbolRecord | null>(
    null,
  );
  const [batchRows, setBatchRows] = useState<BatchRow[]>([
    createBatchRow("find_interfaces"),
    createBatchRow("get_dependencies"),
  ]);
  const [batchResults, setBatchResults] = useState<BatchQueryResponse["results"] | null>(
    null,
  );
  const [batchError, setBatchError] = useState<string | null>(null);

  const healthQuery = useQuery({
    queryKey: ["health", backendUrl],
    queryFn: () => quickdepApi.health(backendUrl),
    refetchInterval: 15_000,
  });

  const projectsQuery = useQuery({
    queryKey: ["projects", backendUrl],
    queryFn: () => quickdepApi.listProjects(backendUrl),
    refetchInterval: 15_000,
  });

  const projectList = projectsQuery.data?.projects ?? [];

  useEffect(() => {
    if (!projectList.length) {
      return;
    }

    const currentExists = activeProjectId
      ? projectList.some((project) => project.id === activeProjectId)
      : false;

    if (currentExists) {
      return;
    }

    const nextProject =
      projectList.find((project) => project.id === projectsQuery.data?.default_project_id) ??
      projectList.find((project) => project.is_default) ??
      projectList[0];

    setActiveProjectId(nextProject.id);
  }, [activeProjectId, projectList, projectsQuery.data?.default_project_id, setActiveProjectId]);

  useEffect(() => {
    setBackendDraft(backendUrl);
  }, [backendUrl]);

  useEffect(() => {
    setSelectedInterfaceKey(null);
    setSelectedSeed(null);
    setCallChainSourceKey(null);
    setCallChainSourceSeed(null);
    setBatchResults(null);
    setBatchError(null);
  }, [activeProjectId, backendUrl]);

  const registryProject =
    projectList.find((project) => project.id === activeProjectId) ?? null;
  const socket = useProjectStatusSocket(backendUrl, registryProject);
  const statusMessage =
    socket.message?.type === "status" ? socket.message : null;
  const socketProject = statusMessage?.data.project ?? null;
  const activeProject =
    socketProject && socketProject.id === registryProject?.id
      ? socketProject
      : registryProject;

  useEffect(() => {
    if (!statusMessage) {
      return;
    }

    queryClient.setQueryData<ProjectsResponse | undefined>(
      ["projects", backendUrl],
      (previous) => {
        if (!previous) {
          return previous;
        }

        return {
          ...previous,
          projects: previous.projects.map((project) =>
            project.id === statusMessage.data.project.id
              ? statusMessage.data.project
              : project,
          ),
        };
      },
    );
  }, [backendUrl, queryClient, statusMessage]);

  const searchQuery = useQuery({
    queryKey: [
      "search",
      backendUrl,
      activeProject?.id ?? "workspace",
      deferredSearch,
      searchLimit,
    ],
    enabled: Boolean(activeProject && deferredSearch.length >= 2),
    queryFn: () =>
      quickdepApi.findInterfaces(backendUrl, {
        project: projectTarget(activeProject),
        query: deferredSearch,
        limit: searchLimit,
      }),
  });

  const interfaceQuery = useQuery({
    queryKey: [
      "interface",
      backendUrl,
      activeProject?.id ?? "workspace",
      selectedInterfaceKey,
    ],
    enabled: Boolean(activeProject && selectedInterfaceKey),
    queryFn: () =>
      quickdepApi.getInterface(backendUrl, {
        project: projectTarget(activeProject),
        interface: selectedInterfaceKey!,
      }),
  });

  const currentSymbol =
    interfaceQuery.data?.interface ??
    (selectedSeed?.qualified_name === selectedInterfaceKey ? selectedSeed : null);

  const dependenciesQuery = useQuery({
    queryKey: [
      "dependencies",
      backendUrl,
      activeProject?.id ?? "workspace",
      selectedInterfaceKey,
      direction,
      maxDepth,
    ],
    enabled: Boolean(activeProject && selectedInterfaceKey),
    queryFn: () =>
      quickdepApi.getDependencies(backendUrl, {
        project: projectTarget(activeProject),
        interface: selectedInterfaceKey!,
        direction,
        max_depth: maxDepth,
      }),
  });

  const fileInterfacesQuery = useQuery({
    queryKey: [
      "file-interfaces",
      backendUrl,
      activeProject?.id ?? "workspace",
      currentSymbol?.file_path ?? "",
    ],
    enabled: Boolean(activeProject && currentSymbol?.file_path),
    queryFn: () =>
      quickdepApi.getFileInterfaces(backendUrl, {
        project: projectTarget(activeProject),
        file_path: currentSymbol!.file_path,
      }),
  });

  const callChainQuery = useQuery({
    queryKey: [
      "call-chain",
      backendUrl,
      activeProject?.id ?? "workspace",
      callChainSourceKey,
      selectedInterfaceKey,
      maxDepth,
    ],
    enabled: Boolean(
      activeProject &&
        selectedInterfaceKey &&
        callChainSourceKey &&
        callChainSourceKey !== selectedInterfaceKey,
    ),
    queryFn: () =>
      quickdepApi.getCallChain(backendUrl, {
        project: projectTarget(activeProject),
        from_interface: callChainSourceKey!,
        to_interface: selectedInterfaceKey!,
        max_depth: maxDepth,
      }),
  });

  const scanMutation = useMutation({
    mutationFn: (rebuild: boolean) =>
      quickdepApi.scanProject(backendUrl, {
        project: scanPath.trim()
          ? { path: scanPath.trim() }
          : projectTarget(activeProject),
        rebuild,
      }),
    onSuccess: (response) => {
      queryClient.invalidateQueries({ queryKey: ["projects", backendUrl] });
      startTransition(() => {
        setActiveProjectId(response.project.id);
      });
    },
  });

  const rebuildMutation = useMutation({
    mutationFn: () =>
      quickdepApi.rebuildDatabase(backendUrl, {
        project: scanPath.trim()
          ? { path: scanPath.trim() }
          : projectTarget(activeProject),
      }),
    onSuccess: (response) => {
      queryClient.invalidateQueries({ queryKey: ["projects", backendUrl] });
      startTransition(() => {
        setActiveProjectId(response.project.id);
      });
    },
  });

  const batchMutation = useMutation({
    mutationFn: (queries: BatchQueryItem[]) =>
      quickdepApi.batchQuery(backendUrl, {
        project: projectTarget(activeProject),
        queries,
      }),
    onSuccess: (response) => {
      setBatchResults(response.results);
      setBatchError(null);
    },
    onError: (error) => {
      setBatchError(errorText(error));
    },
  });

  function focusInterface(symbolKey: string, seed?: SymbolRecord) {
    startTransition(() => {
      setSelectedInterfaceKey(symbolKey);
      setSelectedSeed(seed ?? null);
      setStageTab("graph");
      setDetailTab("overview");
    });
  }

  function setChainSource(symbol: SymbolRecord | null, fallback: string | null) {
    setCallChainSourceKey(fallback);
    setCallChainSourceSeed(symbol);
    setDetailTab("callchain");
  }

  function buildBatchQueries(): BatchQueryItem[] | null {
    const queries: BatchQueryItem[] = [];

    for (const row of batchRows) {
      if (row.kind === "find_interfaces") {
        const query = row.query.trim() || currentSymbol?.name || deferredSearch;
        if (!query) {
          setBatchError("`find_interfaces` needs a query or a focused symbol.");
          return null;
        }

        queries.push({
          kind: row.kind,
          query,
          limit: row.limit,
        });
        continue;
      }

      if (row.kind === "get_interface") {
        const interfaceName = row.interfaceName.trim() || selectedInterfaceKey;
        if (!interfaceName) {
          setBatchError("`get_interface` needs an interface selector.");
          return null;
        }

        queries.push({
          kind: row.kind,
          interface: interfaceName,
        });
        continue;
      }

      if (row.kind === "get_dependencies") {
        const interfaceName = row.interfaceName.trim() || selectedInterfaceKey;
        if (!interfaceName) {
          setBatchError("`get_dependencies` needs an interface selector.");
          return null;
        }

        queries.push({
          kind: row.kind,
          interface: interfaceName,
          direction: row.direction,
          max_depth: row.maxDepth,
        });
        continue;
      }

      if (row.kind === "get_call_chain") {
        const fromInterface = row.fromInterface.trim() || callChainSourceKey;
        const toInterface = row.toInterface.trim() || selectedInterfaceKey;
        if (!fromInterface || !toInterface) {
          setBatchError(
            "`get_call_chain` needs both source and target interfaces.",
          );
          return null;
        }

        queries.push({
          kind: row.kind,
          from_interface: fromInterface,
          to_interface: toInterface,
          max_depth: row.maxDepth,
        });
        continue;
      }

      const filePath = row.filePath.trim() || currentSymbol?.file_path;
      if (!filePath) {
        setBatchError("`get_file_interfaces` needs a file path or focused symbol.");
        return null;
      }

      queries.push({
        kind: row.kind,
        file_path: filePath,
      });
    }

    return queries;
  }

  function runBatchQuery() {
    const queries = buildBatchQueries();
    if (!queries) {
      return;
    }

    setBatchError(null);
    batchMutation.mutate(queries);
  }

  const healthTone = healthQuery.data?.status === "ok" ? "good" : "danger";
  const activeProjectState = activeProject
    ? summarizeProjectState(activeProject.state)
    : null;
  const currentSymbolTone = currentSymbol ? symbolAccent(currentSymbol) : "neutral";
  const splitData = splitDependencies(dependenciesQuery.data);

  return (
    <div className="app-shell">
      <div className="app-shell__texture" />
      <header className="topbar">
        <div className="brand-block">
          <p className="brand-block__eyebrow">QuickDep local analysis cockpit</p>
          <h1 className="brand-block__title">Dependency Operations Deck</h1>
          <p className="brand-block__subtitle">
            Project registry, interface search, dependency graph, and batch
            queries in one industrial console.
          </p>
        </div>

        <div className="topbar__controls">
          <label className="field field--inline">
            <span className="field__label">Backend</span>
            <input
              className="input"
              value={backendDraft}
              onChange={(event) => setBackendDraft(event.target.value)}
              placeholder="http://127.0.0.1:8080"
            />
          </label>
          <button
            className="button button--primary"
            type="button"
            onClick={() => setBackendUrl(backendDraft.trim())}
          >
            Connect
          </button>
          <StatusBadge
            label={healthQuery.isLoading ? "Health checking" : `Health ${healthQuery.data?.status ?? "down"}`}
            tone={healthQuery.isError ? "danger" : healthTone}
          />
          <StatusBadge
            label={`WS ${socket.socketState}`}
            tone={socketTone(socket.socketState)}
          />
          {activeProjectState ? (
            <StatusBadge
              label={`${activeProject?.name ?? "No project"} · ${activeProjectState.label}`}
              tone={activeProjectState.tone}
            />
          ) : null}
        </div>
      </header>

      <main className="workspace-grid">
        <aside className="rail rail--left">
          <Panel
            eyebrow="Registry"
            title="Projects"
            actions={
              <button
                className="button button--ghost"
                type="button"
                onClick={() =>
                  queryClient.invalidateQueries({ queryKey: ["projects", backendUrl] })
                }
              >
                Refresh
              </button>
            }
          >
            <div className="project-ops">
              <label className="field">
                <span className="field__label">Scan path</span>
                <input
                  className="input"
                  value={scanPath}
                  onChange={(event) => setScanPath(event.target.value)}
                  placeholder="/absolute/path/to/project"
                />
              </label>
              <div className="button-row">
                <button
                  className="button button--primary"
                  type="button"
                  onClick={() => scanMutation.mutate(false)}
                  disabled={scanMutation.isPending}
                >
                  {scanMutation.isPending ? "Scanning..." : "Scan / Register"}
                </button>
                <button
                  className="button"
                  type="button"
                  onClick={() => rebuildMutation.mutate()}
                  disabled={rebuildMutation.isPending}
                >
                  {rebuildMutation.isPending ? "Rebuilding..." : "Rebuild DB"}
                </button>
              </div>
              {scanMutation.isError ? (
                <p className="notice notice--danger">{errorText(scanMutation.error)}</p>
              ) : null}
              {rebuildMutation.isError ? (
                <p className="notice notice--danger">
                  {errorText(rebuildMutation.error)}
                </p>
              ) : null}
            </div>

            {projectsQuery.isLoading ? (
              <p className="empty-copy">Loading project registry...</p>
            ) : null}
            {projectsQuery.isError ? (
              <p className="notice notice--danger">{errorText(projectsQuery.error)}</p>
            ) : null}

            <div className="project-list">
              {projectList.map((project) => {
                const state = summarizeProjectState(project.state);
                const selected = project.id === activeProject?.id;
                return (
                  <button
                    key={project.id}
                    className={`project-card${selected ? " project-card--active" : ""}`}
                    type="button"
                    onClick={() => {
                      startTransition(() => {
                        setActiveProjectId(project.id);
                      });
                    }}
                  >
                    <div className="project-card__header">
                      <strong>{project.name}</strong>
                      <StatusBadge label={state.label} tone={state.tone} />
                    </div>
                    <p className="project-card__path">{project.path}</p>
                    <p className="project-card__detail">{state.detail}</p>
                    <div className="project-card__stats">
                      <span>{formatCount(state.stats?.files)} files</span>
                      <span>{formatCount(state.stats?.symbols)} symbols</span>
                      <span>{formatCount(state.stats?.dependencies)} deps</span>
                    </div>
                  </button>
                );
              })}
            </div>
          </Panel>

          <Panel eyebrow="Search" title="Interface Probe">
            <label className="field">
              <span className="field__label">Query</span>
              <input
                className="input"
                value={searchTerm}
                onChange={(event) => setSearchTerm(event.target.value)}
                placeholder="entry, helper, Client, Parser..."
              />
            </label>
            <div className="field-grid">
              <label className="field">
                <span className="field__label">Limit</span>
                <select
                  className="select"
                  value={searchLimit}
                  onChange={(event) => setSearchLimit(Number(event.target.value))}
                >
                  {[8, 12, 20, 40].map((value) => (
                    <option key={value} value={value}>
                      {value}
                    </option>
                  ))}
                </select>
              </label>
              <div className="search-meta">
                <p>{deferredSearch ? `Search "${deferredSearch}"` : "Type at least 2 characters"}</p>
                <p>{formatCount(searchQuery.data?.interfaces.length)} hits</p>
              </div>
            </div>
            {searchQuery.isFetching ? <p className="empty-copy">Searching...</p> : null}
            {searchQuery.isError ? (
              <p className="notice notice--danger">{errorText(searchQuery.error)}</p>
            ) : null}
            <div className="result-list">
              {(searchQuery.data?.interfaces ?? []).map((symbol) => (
                <button
                  key={symbol.id}
                  className={`result-card${
                    selectedInterfaceKey === symbol.qualified_name
                      ? " result-card--active"
                      : ""
                  }`}
                  type="button"
                  onClick={() => focusInterface(symbol.qualified_name, symbol)}
                >
                  <div className="result-card__header">
                    <strong>{symbol.name}</strong>
                    <StatusBadge label={symbol.kind} tone={symbolAccent(symbol)} />
                  </div>
                  <p>{symbol.qualified_name}</p>
                  <div className="result-card__meta">
                    <span>{compactPath(symbol.file_path, 4)}</span>
                    <span>
                      L{symbol.line}:{symbol.column}
                    </span>
                  </div>
                </button>
              ))}
            </div>
          </Panel>
        </aside>

        <section className="stage">
          <div className="tab-strip">
            {(["graph", "table", "batch"] as StageTab[]).map((tab) => (
              <button
                key={tab}
                className={`tab-strip__tab${stageTab === tab ? " tab-strip__tab--active" : ""}`}
                type="button"
                onClick={() => setStageTab(tab)}
              >
                {stageTabLabel(tab)}
              </button>
            ))}
          </div>

          {stageTab === "graph" ? (
            <Panel
              eyebrow="Observe"
              title="Dependency Theatre"
              tone="dark"
              actions={
                <div className="toolbar">
                  <select
                    className="select select--dark"
                    value={direction}
                    onChange={(event) =>
                      setDirection(event.target.value as DependencyDirection)
                    }
                  >
                    <option value="both">Both directions</option>
                    <option value="outgoing">Outgoing</option>
                    <option value="incoming">Incoming</option>
                  </select>
                  <select
                    className="select select--dark"
                    value={maxDepth}
                    onChange={(event) => setMaxDepth(Number(event.target.value))}
                  >
                    {[1, 2, 3, 4].map((depth) => (
                      <option key={depth} value={depth}>
                        Depth {depth}
                      </option>
                    ))}
                  </select>
                </div>
              }
            >
              <div className="stage-summary">
                <div className="metric-card">
                  <span className="metric-card__label">Focus</span>
                  <strong>{currentSymbol?.name ?? "No symbol selected"}</strong>
                  <p>{currentSymbol ? currentSymbol.qualified_name : "Search and focus any symbol."}</p>
                </div>
                <div className="metric-card">
                  <span className="metric-card__label">Total returned</span>
                  <strong>{formatCount(totalDependencies(dependenciesQuery.data))}</strong>
                  <p>Across the current dependency direction and depth window.</p>
                </div>
                <div className="metric-card">
                  <span className="metric-card__label">Project</span>
                  <strong>{activeProject?.name ?? "No project"}</strong>
                  <p>{activeProject ? compactPath(activeProject.path, 5) : "Connect to a QuickDep backend."}</p>
                </div>
              </div>

              {dependenciesQuery.isError ? (
                <p className="notice notice--danger">{errorText(dependenciesQuery.error)}</p>
              ) : null}

              <div className="graph-panel">
                <DependencyGraphView
                  symbol={currentSymbol}
                  dependencies={dependenciesQuery.data}
                  onSelectInterface={(qualifiedName) => focusInterface(qualifiedName)}
                />
              </div>
            </Panel>
          ) : null}

          {stageTab === "table" ? (
            <Panel eyebrow="Inspect" title="Dependency Tables">
              {!currentSymbol ? (
                <div className="empty-state">
                  <p className="empty-state__eyebrow">No focus symbol</p>
                  <h3>Search and select an interface first.</h3>
                  <p>
                    The table view exposes the raw dependency rows with depth and
                    relationship hints.
                  </p>
                </div>
              ) : (
                <div className="table-stack">
                  <div className="metric-inline">
                    <StatusBadge
                      label={currentSymbol.kind}
                      tone={currentSymbolTone}
                    />
                    <span>{currentSymbol.qualified_name}</span>
                  </div>

                  <DependencyTable
                    title="Outgoing"
                    rows={splitData.outgoing}
                    onSelect={focusInterface}
                  />
                  <DependencyTable
                    title="Incoming"
                    rows={splitData.incoming}
                    onSelect={focusInterface}
                  />
                </div>
              )}
            </Panel>
          ) : null}

          {stageTab === "batch" ? (
            <Panel
              eyebrow="Compose"
              title="Batch Query Lab"
              actions={
                <div className="button-row">
                  <button
                    className="button button--ghost"
                    type="button"
                    onClick={() => setBatchRows((rows) => [...rows, createBatchRow()])}
                  >
                    Add row
                  </button>
                  <button
                    className="button button--primary"
                    type="button"
                    onClick={runBatchQuery}
                    disabled={batchMutation.isPending}
                  >
                    {batchMutation.isPending ? "Running..." : "Run batch"}
                  </button>
                </div>
              }
            >
              <div className="batch-toolbar">
                <p>
                  Rows can inherit from the current focus symbol and call-chain
                  source, so common debugging flows do not need hand-written
                  JSON.
                </p>
              </div>

              <div className="batch-list">
                {batchRows.map((row) => (
                  <BatchRowEditor
                    key={row.id}
                    row={row}
                    onChange={(nextRow) =>
                      setBatchRows((rows) =>
                        rows.map((candidate) =>
                          candidate.id === nextRow.id ? nextRow : candidate,
                        ),
                      )
                    }
                    onRemove={() =>
                      setBatchRows((rows) =>
                        rows.filter((candidate) => candidate.id !== row.id),
                      )
                    }
                  />
                ))}
              </div>

              {batchError ? <p className="notice notice--danger">{batchError}</p> : null}
              {batchResults ? (
                <div className="batch-results">
                  {batchResults.map((result) => (
                    <article key={`${result.kind}-${result.index}`} className="batch-result">
                      <div className="batch-result__header">
                        <strong>
                          #{result.index + 1} {result.kind}
                        </strong>
                        <StatusBadge
                          label={result.ok ? "ok" : "failed"}
                          tone={result.ok ? "good" : "danger"}
                        />
                      </div>
                      <pre className="json-block">
                        {JSON.stringify(
                          result.ok
                            ? result.result
                            : { error: result.error ?? "unknown error" },
                          null,
                          2,
                        )}
                      </pre>
                    </article>
                  ))}
                </div>
              ) : null}
            </Panel>
          ) : null}
        </section>

        <aside className="rail rail--right">
          <Panel eyebrow="Focus" title="Interface Lens">
            {!selectedInterfaceKey ? (
              <div className="empty-state">
                <p className="empty-state__eyebrow">No active symbol</p>
                <h3>Choose a symbol from search or the graph.</h3>
                <p>
                  The right rail becomes the precise operator surface for details,
                  file membership, and call-chain work.
                </p>
              </div>
            ) : (
              <>
                <div className="tab-strip tab-strip--compact">
                  {(["overview", "deps", "callchain", "raw"] as DetailTab[]).map(
                    (tab) => (
                      <button
                        key={tab}
                        className={`tab-strip__tab${
                          detailTab === tab ? " tab-strip__tab--active" : ""
                        }`}
                        type="button"
                        onClick={() => setDetailTab(tab)}
                      >
                        {detailTabLabel(tab)}
                      </button>
                    ),
                  )}
                </div>

                {interfaceQuery.isLoading ? <p className="empty-copy">Loading interface...</p> : null}
                {interfaceQuery.isError ? (
                  <p className="notice notice--danger">{errorText(interfaceQuery.error)}</p>
                ) : null}

                {detailTab === "overview" && currentSymbol ? (
                  <div className="detail-stack">
                    <div className="detail-hero">
                      <div>
                        <h3>{currentSymbol.name}</h3>
                        <p>{currentSymbol.qualified_name}</p>
                      </div>
                      <StatusBadge
                        label={currentSymbol.kind}
                        tone={currentSymbolTone}
                      />
                    </div>
                    <div className="button-row">
                      <button
                        className="button button--primary"
                        type="button"
                        onClick={() =>
                          setChainSource(currentSymbol, currentSymbol.qualified_name)
                        }
                      >
                        Mark as chain source
                      </button>
                      <button
                        className="button"
                        type="button"
                        onClick={() =>
                          queryClient.invalidateQueries({
                            queryKey: [
                              "dependencies",
                              backendUrl,
                              activeProject?.id ?? "workspace",
                              selectedInterfaceKey,
                              direction,
                              maxDepth,
                            ],
                          })
                        }
                      >
                        Refresh deps
                      </button>
                    </div>
                    <dl className="detail-grid">
                      <DetailItem label="Kind" value={currentSymbol.kind} />
                      <DetailItem label="Visibility" value={currentSymbol.visibility} />
                      <DetailItem label="Source" value={currentSymbol.source} />
                      <DetailItem
                        label="Location"
                        value={`${currentSymbol.file_path}:${currentSymbol.line}:${currentSymbol.column}`}
                      />
                    </dl>
                    {currentSymbol.signature ? (
                      <div className="code-chip">{currentSymbol.signature}</div>
                    ) : null}
                  </div>
                ) : null}

                {detailTab === "deps" ? (
                  <div className="detail-stack">
                    <div className="metric-inline">
                      <StatusBadge label="Incoming" tone="warn" />
                      <span>{formatCount(splitData.incoming.length)}</span>
                    </div>
                    <div className="metric-inline">
                      <StatusBadge label="Outgoing" tone="good" />
                      <span>{formatCount(splitData.outgoing.length)}</span>
                    </div>
                    <div className="list-compact">
                      {[...splitData.outgoing.slice(0, 4), ...splitData.incoming.slice(0, 4)].map(
                        (node) => (
                          <button
                            key={node.symbol_id}
                            className="list-compact__item"
                            type="button"
                            onClick={() => focusInterface(node.qualified_name)}
                          >
                            <strong>{node.name}</strong>
                            <span>{describeDependencyNode(node)}</span>
                          </button>
                        ),
                      )}
                    </div>
                  </div>
                ) : null}

                {detailTab === "callchain" ? (
                  <div className="detail-stack">
                    <p className="muted-copy">
                      Source symbol: {symbolLabel(callChainSourceSeed, callChainSourceKey)}
                    </p>
                    <p className="muted-copy">
                      Target symbol: {symbolLabel(currentSymbol, selectedInterfaceKey)}
                    </p>
                    <div className="button-row">
                      <button
                        className="button"
                        type="button"
                        onClick={() =>
                          currentSymbol
                            ? setChainSource(currentSymbol, currentSymbol.qualified_name)
                            : undefined
                        }
                      >
                        Set focus as source
                      </button>
                      <button
                        className="button button--ghost"
                        type="button"
                        onClick={() => {
                          setCallChainSourceKey(null);
                          setCallChainSourceSeed(null);
                        }}
                      >
                        Clear source
                      </button>
                    </div>
                    {callChainQuery.isLoading ? <p className="empty-copy">Calculating call chain...</p> : null}
                    {callChainQuery.isError ? (
                      <p className="notice notice--danger">
                        {errorText(callChainQuery.error)}
                      </p>
                    ) : null}
                    {callChainQuery.data?.path?.length ? (
                      <ol className="chain-list">
                        {callChainQuery.data.path.map((symbol, index) => (
                          <li key={`${symbol.id}-${index}`}>
                            <button
                              className="chain-list__item"
                              type="button"
                              onClick={() => focusInterface(symbol.qualified_name, symbol)}
                            >
                              <strong>{symbol.name}</strong>
                              <span>{symbol.qualified_name}</span>
                            </button>
                          </li>
                        ))}
                      </ol>
                    ) : (
                      <p className="empty-copy">
                        Mark one symbol as source, then focus another symbol to query
                        the path.
                      </p>
                    )}
                  </div>
                ) : null}

                {detailTab === "raw" ? (
                  <pre className="json-block">
                    {JSON.stringify(
                      {
                        interface: interfaceQuery.data?.interface ?? selectedSeed,
                        dependencies: dependenciesQuery.data,
                        file_interfaces: fileInterfacesQuery.data,
                      },
                      null,
                      2,
                    )}
                  </pre>
                ) : null}
              </>
            )}
          </Panel>

          <Panel eyebrow="File slice" title="File Interfaces">
            {fileInterfacesQuery.isLoading ? <p className="empty-copy">Loading file members...</p> : null}
            {fileInterfacesQuery.isError ? (
              <p className="notice notice--danger">{errorText(fileInterfacesQuery.error)}</p>
            ) : null}
            <div className="list-compact">
              {(fileInterfacesQuery.data?.interfaces ?? []).map((symbol) => (
                <button
                  key={symbol.id}
                  className={`list-compact__item${
                    selectedInterfaceKey === symbol.qualified_name
                      ? " list-compact__item--active"
                      : ""
                  }`}
                  type="button"
                  onClick={() => focusInterface(symbol.qualified_name, symbol)}
                >
                  <strong>{symbol.name}</strong>
                  <span>
                    {symbol.kind} · {compactPath(symbol.file_path, 4)}
                  </span>
                </button>
              ))}
              {!fileInterfacesQuery.data?.interfaces?.length ? (
                <p className="empty-copy">
                  Pick a symbol to inspect all interfaces declared in the same file.
                </p>
              ) : null}
            </div>
          </Panel>
        </aside>
      </main>
    </div>
  );
}

function DependencyTable({
  title,
  rows,
  onSelect,
}: {
  title: string;
  rows: DependencyNode[];
  onSelect: (qualifiedName: string) => void;
}) {
  return (
    <section className="table-card">
      <header className="table-card__header">
        <strong>{title}</strong>
        <span>{formatCount(rows.length)} nodes</span>
      </header>
      <div className="table-wrap">
        <table className="data-table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Qualified name</th>
              <th>File</th>
              <th>Depth</th>
              <th>Kind</th>
            </tr>
          </thead>
          <tbody>
            {rows.length ? (
              rows.map((row) => (
                <tr key={row.symbol_id}>
                  <td>
                    <button
                      className="table-link"
                      type="button"
                      onClick={() => onSelect(row.qualified_name)}
                    >
                      {row.name}
                    </button>
                  </td>
                  <td>{row.qualified_name}</td>
                  <td>{compactPath(row.file_path, 4)}</td>
                  <td>{row.depth}</td>
                  <td>{row.dep_kind ?? "n/a"}</td>
                </tr>
              ))
            ) : (
              <tr>
                <td colSpan={5} className="data-table__empty">
                  No rows in this direction.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function DetailItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="detail-item">
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  );
}

function BatchRowEditor({
  row,
  onChange,
  onRemove,
}: {
  row: BatchRow;
  onChange: (row: BatchRow) => void;
  onRemove: () => void;
}) {
  const showQuery = row.kind === "find_interfaces";
  const showInterface =
    row.kind === "get_interface" || row.kind === "get_dependencies";
  const showDirection = row.kind === "get_dependencies";
  const showCallChain = row.kind === "get_call_chain";
  const showFile = row.kind === "get_file_interfaces";
  const showLimit = row.kind === "find_interfaces";
  const showDepth =
    row.kind === "get_dependencies" || row.kind === "get_call_chain";

  return (
    <article className="batch-editor">
      <div className="batch-editor__top">
        <label className="field">
          <span className="field__label">Kind</span>
          <select
            className="select"
            value={row.kind}
            onChange={(event) =>
              onChange({
                ...row,
                kind: event.target.value as BatchQueryKind,
              })
            }
          >
            <option value="find_interfaces">find_interfaces</option>
            <option value="get_interface">get_interface</option>
            <option value="get_dependencies">get_dependencies</option>
            <option value="get_call_chain">get_call_chain</option>
            <option value="get_file_interfaces">get_file_interfaces</option>
          </select>
        </label>

        <button className="button button--ghost" type="button" onClick={onRemove}>
          Remove
        </button>
      </div>

      <div className="batch-editor__grid">
        {showQuery ? (
          <label className="field">
            <span className="field__label">Query</span>
            <input
              className="input"
              value={row.query}
              onChange={(event) => onChange({ ...row, query: event.target.value })}
              placeholder="helper"
            />
          </label>
        ) : null}

        {showInterface ? (
          <label className="field">
            <span className="field__label">Interface</span>
            <input
              className="input"
              value={row.interfaceName}
              onChange={(event) =>
                onChange({ ...row, interfaceName: event.target.value })
              }
              placeholder="src/lib.rs::helper"
            />
          </label>
        ) : null}

        {showCallChain ? (
          <>
            <label className="field">
              <span className="field__label">From</span>
              <input
                className="input"
                value={row.fromInterface}
                onChange={(event) =>
                  onChange({ ...row, fromInterface: event.target.value })
                }
                placeholder="source symbol"
              />
            </label>
            <label className="field">
              <span className="field__label">To</span>
              <input
                className="input"
                value={row.toInterface}
                onChange={(event) =>
                  onChange({ ...row, toInterface: event.target.value })
                }
                placeholder="target symbol"
              />
            </label>
          </>
        ) : null}

        {showFile ? (
          <label className="field">
            <span className="field__label">File path</span>
            <input
              className="input"
              value={row.filePath}
              onChange={(event) => onChange({ ...row, filePath: event.target.value })}
              placeholder="src/lib.rs"
            />
          </label>
        ) : null}

        {showDirection ? (
          <label className="field">
            <span className="field__label">Direction</span>
            <select
              className="select"
              value={row.direction}
              onChange={(event) =>
                onChange({
                  ...row,
                  direction: event.target.value as DependencyDirection,
                })
              }
            >
              <option value="both">both</option>
              <option value="outgoing">outgoing</option>
              <option value="incoming">incoming</option>
            </select>
          </label>
        ) : null}

        {showLimit ? (
          <label className="field">
            <span className="field__label">Limit</span>
            <input
              className="input"
              type="number"
              min={1}
              max={100}
              value={row.limit}
              onChange={(event) =>
                onChange({ ...row, limit: Number(event.target.value) })
              }
            />
          </label>
        ) : null}

        {showDepth ? (
          <label className="field">
            <span className="field__label">Max depth</span>
            <input
              className="input"
              type="number"
              min={1}
              max={8}
              value={row.maxDepth}
              onChange={(event) =>
                onChange({ ...row, maxDepth: Number(event.target.value) })
              }
            />
          </label>
        ) : null}
      </div>
    </article>
  );
}
