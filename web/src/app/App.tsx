import { startTransition, useDeferredValue, useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { DependencyGraphView } from "../components/DependencyGraphView";
import { ProjectOverviewGraphView } from "../components/ProjectOverviewGraphView";
import { useLocalStorage } from "../hooks/useLocalStorage";
import { useProjectStatusSocket } from "../hooks/useProjectStatusSocket";
import { projectTarget, quickdepApi, resolveDefaultBaseUrl } from "../lib/api/client";
import type {
  DependenciesResponse,
  DependencyDirection,
  ProjectOverviewPayload,
  ProjectRecord,
  ProjectsResponse,
  SymbolRecord,
} from "../lib/api/types";
import { errorText } from "../lib/error-utils";
import { resolveDefaultLocale, type Locale } from "../lib/i18n";
import {
  compactPath,
  formatCount,
  presentSymbolKind,
  summarizeProjectState,
  splitDependencies,
} from "../lib/presenters";

type Copy = {
  appTitle: string;
  backend: string;
  backendPlaceholder: string;
  projects: string;
  search: string;
  searchPlaceholder: string;
  focus: string;
  noFocus: string;
  noFocusHint: string;
  siblings: string;
  noSiblings: string;
  scanPath: string;
  scan: string;
  rebuild: string;
  loadingProjects: string;
  noProjects: string;
  cloud: string;
  backToCloud: string;
  direction: string;
  depth: string;
  graphScale: string;
  relationLegend: string;
  directionLegend: string;
  directRelation: string;
  indirectRelation: string;
  weightedRelation: string;
  outgoingRelation: string;
  incomingRelation: string;
  incoming: string;
  outgoing: string;
  both: string;
  backendOk: string;
  backendDown: string;
  socketIdle: string;
  socketOpen: string;
  socketConnecting: string;
  socketClosed: string;
  socketError: string;
  projectPath: string;
  location: string;
  kind: string;
  files: string;
  symbols: string;
  dependencies: string;
  nodes: string;
  edges: string;
  results: string;
  overviewEmpty: string;
  overviewLoading: string;
  searchEmpty: string;
};

const zhCN: Copy = {
  appTitle: "QuickDep",
  backend: "后端地址",
  backendPlaceholder: "http://127.0.0.1:8080",
  projects: "项目",
  search: "搜索接口",
  searchPlaceholder: "输入函数、方法、类名",
  focus: "当前焦点",
  noFocus: "还没有选中接口",
  noFocusHint: "先从左边搜索，或者直接点击右侧云图节点。",
  siblings: "同文件接口",
  noSiblings: "这个文件里没有更多接口",
  scanPath: "扫描路径",
  scan: "扫描 / 注册",
  rebuild: "重建索引",
  loadingProjects: "正在读取项目列表",
  noProjects: "还没有项目，先填路径再扫描。",
  cloud: "项目云图",
  backToCloud: "返回项目云图",
  direction: "依赖方向",
  depth: "深度",
  graphScale: "云图规模",
  relationLegend: "关系说明",
  directionLegend: "方向说明",
  directRelation: "实线：直接关系 / 一阶依赖",
  indirectRelation: "虚线：更深层的间接关系",
  weightedRelation: "线越粗越深：关系越强或连接越多",
  outgoingRelation: "深色线：以当前接口为主的关联",
  incomingRelation: "浅色线：从周边收敛过来的关联",
  incoming: "流入",
  outgoing: "流出",
  both: "双向",
  backendOk: "后端正常",
  backendDown: "后端离线",
  socketIdle: "实时状态未连接",
  socketOpen: "实时状态已连接",
  socketConnecting: "实时状态连接中",
  socketClosed: "实时状态已断开",
  socketError: "实时状态异常",
  projectPath: "项目路径",
  location: "位置",
  kind: "类型",
  files: "文件",
  symbols: "接口",
  dependencies: "依赖",
  nodes: "节点",
  edges: "连线",
  results: "结果",
  overviewEmpty: "这个项目还没有可展示的接口关系。",
  overviewLoading: "项目正在扫描，接口关系会在索引完成后显示。",
  searchEmpty: "没有命中结果",
};

const enUS: Copy = {
  appTitle: "QuickDep",
  backend: "Backend",
  backendPlaceholder: "http://127.0.0.1:8080",
  projects: "Projects",
  search: "Search",
  searchPlaceholder: "Search function, method, or class",
  focus: "Focus",
  noFocus: "No interface selected",
  noFocusHint: "Search from the left or click a node in the graph.",
  siblings: "Same-file interfaces",
  noSiblings: "No additional interfaces in this file",
  scanPath: "Scan path",
  scan: "Scan / Register",
  rebuild: "Rebuild index",
  loadingProjects: "Loading projects",
  noProjects: "No project yet. Enter a path and scan.",
  cloud: "Project cloud",
  backToCloud: "Back to cloud",
  direction: "Direction",
  depth: "Depth",
  graphScale: "Graph scale",
  relationLegend: "Relation guide",
  directionLegend: "Direction guide",
  directRelation: "Solid line: direct or first-hop relation",
  indirectRelation: "Dashed line: deeper indirect relation",
  weightedRelation: "Darker and thicker lines mean stronger or denser relations",
  outgoingRelation: "Dark line: relation centered on the current symbol",
  incomingRelation: "Light line: relation converging from surrounding symbols",
  incoming: "Incoming",
  outgoing: "Outgoing",
  both: "Both",
  backendOk: "Backend ok",
  backendDown: "Backend down",
  socketIdle: "Socket idle",
  socketOpen: "Socket open",
  socketConnecting: "Socket connecting",
  socketClosed: "Socket closed",
  socketError: "Socket error",
  projectPath: "Project path",
  location: "Location",
  kind: "Kind",
  files: "Files",
  symbols: "Symbols",
  dependencies: "Dependencies",
  nodes: "Nodes",
  edges: "Edges",
  results: "Results",
  overviewEmpty: "No graphable interfaces in this project yet.",
  overviewLoading: "Project is still scanning. The graph will appear after indexing finishes.",
  searchEmpty: "No matches",
};

function getCopy(locale: Locale): Copy {
  return locale === "zh-CN" ? zhCN : enUS;
}

function socketLabel(state: string, copy: Copy): string {
  switch (state) {
    case "open":
      return copy.socketOpen;
    case "connecting":
    case "reconnecting":
      return copy.socketConnecting;
    case "closed":
      return copy.socketClosed;
    case "error":
      return copy.socketError;
    default:
      return copy.socketIdle;
  }
}

function healthLabel(status: string | undefined, copy: Copy): string {
  return status === "ok" ? copy.backendOk : copy.backendDown;
}

function selectProject(
  projects: ProjectRecord[],
  currentProjectId: string | null,
  defaultProjectId?: string,
): ProjectRecord | null {
  if (!projects.length) {
    return null;
  }

  if (currentProjectId) {
    const matched = projects.find((project) => project.id === currentProjectId);
    if (matched) {
      return matched;
    }
  }

  return (
    projects.find((project) => project.id === defaultProjectId) ??
    projects.find((project) => project.is_default) ??
    projects[0]
  );
}

export default function App() {
  const queryClient = useQueryClient();
  const [locale, setLocale] = useLocalStorage<Locale>(
    "quickdep.web.locale",
    resolveDefaultLocale(),
  );
  const copy = getCopy(locale);

  const [backendUrl, setBackendUrl] = useLocalStorage<string>(
    "quickdep.web.backend-url",
    resolveDefaultBaseUrl(),
  );
  const [backendDraft, setBackendDraft] = useState(backendUrl);
  const [activeProjectId, setActiveProjectId] = useLocalStorage<string | null>(
    "quickdep.web.active-project-id",
    null,
  );
  const [scanPath, setScanPath] = useLocalStorage<string>("quickdep.web.scan-path", "");
  const [searchInput, setSearchInput] = useState("");
  const deferredSearch = useDeferredValue(searchInput.trim());
  const [selectedInterfaceKey, setSelectedInterfaceKey] = useState<string | null>(null);
  const [selectedSeed, setSelectedSeed] = useState<SymbolRecord | null>(null);
  const [direction, setDirection] = useLocalStorage<DependencyDirection>(
    "quickdep.web.direction",
    "both",
  );
  const [maxDepth, setMaxDepth] = useLocalStorage<number>("quickdep.web.max-depth", 2);
  const [overviewLimit, setOverviewLimit] = useLocalStorage<number>(
    "quickdep.web.overview-limit",
    80,
  );

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
    setBackendDraft(backendUrl);
  }, [backendUrl]);

  useEffect(() => {
    document.documentElement.lang = locale;
    document.title = copy.appTitle;
  }, [copy.appTitle, locale]);

  useEffect(() => {
    const nextProject = selectProject(
      projectList,
      activeProjectId,
      projectsQuery.data?.default_project_id,
    );

    if (nextProject && nextProject.id !== activeProjectId) {
      setActiveProjectId(nextProject.id);
    }
  }, [activeProjectId, projectList, projectsQuery.data?.default_project_id, setActiveProjectId]);

  const registryProject = useMemo(
    () =>
      selectProject(projectList, activeProjectId, projectsQuery.data?.default_project_id),
    [activeProjectId, projectList, projectsQuery.data?.default_project_id],
  );

  const socket = useProjectStatusSocket(backendUrl, registryProject);
  const statusMessage = socket.message?.type === "status" ? socket.message : null;
  const socketProject = statusMessage?.data.project ?? null;
  const activeProject =
    socketProject && socketProject.id === registryProject?.id ? socketProject : registryProject;

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
            project.id === statusMessage.data.project.id ? statusMessage.data.project : project,
          ),
        };
      },
    );
  }, [backendUrl, queryClient, statusMessage]);

  useEffect(() => {
    setSelectedInterfaceKey(null);
    setSelectedSeed(null);
  }, [activeProject?.id, backendUrl]);

  const searchQuery = useQuery({
    queryKey: ["search", backendUrl, activeProject?.id ?? "workspace", deferredSearch],
    enabled: Boolean(activeProject && deferredSearch.length >= 1),
    queryFn: () =>
      quickdepApi.findInterfaces(backendUrl, {
        project: projectTarget(activeProject),
        query: deferredSearch,
        limit: 10,
      }),
  });

  const interfaceQuery = useQuery({
    queryKey: ["interface", backendUrl, activeProject?.id ?? "workspace", selectedInterfaceKey],
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

  const overviewQuery = useQuery({
    queryKey: ["overview", backendUrl, activeProject?.id ?? "workspace", overviewLimit],
    enabled: Boolean(activeProject),
    queryFn: () =>
      quickdepApi.getProjectOverview(backendUrl, {
        project: projectTarget(activeProject),
        max_symbols: overviewLimit,
        max_edges: Math.max(96, overviewLimit * 2),
      }),
  });

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

  const scanMutation = useMutation({
    mutationFn: (rebuild: boolean) =>
      quickdepApi.scanProject(backendUrl, {
        project: scanPath.trim() ? { path: scanPath.trim() } : projectTarget(activeProject),
        rebuild,
      }),
    onSuccess: (response) => {
      queryClient.invalidateQueries({ queryKey: ["projects", backendUrl] });
      queryClient.invalidateQueries({ queryKey: ["overview", backendUrl] });
      startTransition(() => {
        setActiveProjectId(response.project.id);
      });
    },
  });

  const rebuildMutation = useMutation({
    mutationFn: () =>
      quickdepApi.rebuildDatabase(backendUrl, {
        project: scanPath.trim() ? { path: scanPath.trim() } : projectTarget(activeProject),
      }),
    onSuccess: (response) => {
      queryClient.invalidateQueries({ queryKey: ["projects", backendUrl] });
      queryClient.invalidateQueries({ queryKey: ["overview", backendUrl] });
      startTransition(() => {
        setActiveProjectId(response.project.id);
      });
    },
  });

  const projectSummary = activeProject ? summarizeProjectState(activeProject.state, locale) : null;
  const splitData = splitDependencies(dependenciesQuery.data as DependenciesResponse | undefined);
  const projectOverview: ProjectOverviewPayload | undefined = overviewQuery.data?.overview;
  const sameFileInterfaces =
    fileInterfacesQuery.data?.interfaces.filter(
      (item) => item.qualified_name !== currentSymbol?.qualified_name,
    ) ?? [];

  function focusInterface(symbolKey: string, seed?: SymbolRecord) {
    startTransition(() => {
      setSelectedInterfaceKey(symbolKey);
      setSelectedSeed(seed ?? null);
    });
  }

  function clearFocus() {
    startTransition(() => {
      setSelectedInterfaceKey(null);
      setSelectedSeed(null);
    });
  }

  return (
    <div className="shell">
      <aside className="sidebar">
        <section className="sidebar-block sidebar-block--brand">
          <div className="brand-row">
            <div>
              <h1 className="brand-title">{copy.appTitle}</h1>
            </div>
            <div className="locale-switch" aria-label="locale switch">
              {(["zh-CN", "en"] as Locale[]).map((option) => (
                <button
                  key={option}
                  type="button"
                  className={`locale-switch__button${
                    locale === option ? " locale-switch__button--active" : ""
                  }`}
                  onClick={() => setLocale(option)}
                >
                  {option === "zh-CN" ? "中文" : "EN"}
                </button>
              ))}
            </div>
          </div>

          <label className="field">
            <span className="field__label">{copy.backend}</span>
            <input
              className="text-input"
              value={backendDraft}
              onChange={(event) => setBackendDraft(event.target.value)}
              onBlur={() => setBackendUrl(backendDraft.trim())}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  setBackendUrl(backendDraft.trim());
                }
              }}
              placeholder={copy.backendPlaceholder}
            />
          </label>

          <div className="chip-row">
            <span
              className={`chip ${
                healthQuery.data?.status === "ok" ? "chip--good" : "chip--muted"
              }`}
            >
              {healthLabel(healthQuery.data?.status, copy)}
            </span>
            <span className="chip chip--muted">{socketLabel(socket.socketState, copy)}</span>
            {projectSummary ? <span className="chip chip--muted">{projectSummary.label}</span> : null}
          </div>
        </section>

        <section className="sidebar-block">
          <div className="block-heading">
            <h2>{copy.projects}</h2>
          </div>

          <label className="field">
            <span className="field__label">{copy.scanPath}</span>
            <input
              className="text-input"
              value={scanPath}
              onChange={(event) => setScanPath(event.target.value)}
              placeholder="/path/to/project"
            />
          </label>

          <div className="action-row">
            <button
              type="button"
              className="action-button action-button--primary"
              onClick={() => scanMutation.mutate(false)}
              disabled={scanMutation.isPending}
            >
              {copy.scan}
            </button>
            <button
              type="button"
              className="action-button"
              onClick={() => rebuildMutation.mutate()}
              disabled={rebuildMutation.isPending}
            >
              {copy.rebuild}
            </button>
          </div>

          <div className="project-list">
            {projectsQuery.isLoading ? (
              <div className="empty-card">{copy.loadingProjects}</div>
            ) : projectList.length ? (
              projectList.map((project) => {
                const state = summarizeProjectState(project.state, locale);
                const isActive = project.id === activeProject?.id;

                return (
                  <button
                    key={project.id}
                    type="button"
                    className={`project-card${isActive ? " project-card--active" : ""}`}
                    onClick={() => setActiveProjectId(project.id)}
                  >
                    <strong>{project.name}</strong>
                    <span>{compactPath(project.path, 4)}</span>
                    <span>{state.detail}</span>
                  </button>
                );
              })
            ) : (
              <div className="empty-card">{copy.noProjects}</div>
            )}
          </div>
        </section>

        <section className="sidebar-block">
          <div className="block-heading">
            <h2>{copy.search}</h2>
            {searchQuery.data ? (
              <span className="block-meta">
                {copy.results} {formatCount(searchQuery.data.interfaces.length)}
              </span>
            ) : null}
          </div>

          <label className="field">
            <input
              className="text-input"
              value={searchInput}
              onChange={(event) => setSearchInput(event.target.value)}
              placeholder={copy.searchPlaceholder}
            />
          </label>

          <div className="result-list">
            {searchQuery.isError ? (
              <div className="empty-card">{errorText(searchQuery.error, locale)}</div>
            ) : searchQuery.data?.interfaces.length ? (
              searchQuery.data.interfaces.map((symbol) => (
                <button
                  key={symbol.id}
                  type="button"
                  className={`result-card${
                    symbol.qualified_name === currentSymbol?.qualified_name
                      ? " result-card--active"
                      : ""
                  }`}
                  onClick={() => focusInterface(symbol.qualified_name, symbol)}
                >
                  <strong>{symbol.name}</strong>
                  <span>{presentSymbolKind(symbol.kind, locale)}</span>
                  <span>{compactPath(symbol.file_path, 3)}</span>
                </button>
              ))
            ) : deferredSearch ? (
              <div className="empty-card">{copy.searchEmpty}</div>
            ) : null}
          </div>
        </section>

        <section className="sidebar-block sidebar-block--focus">
          <div className="block-heading">
            <h2>{copy.focus}</h2>
          </div>

          {currentSymbol ? (
            <>
              <div className="focus-card">
                <strong className="focus-card__title">{currentSymbol.qualified_name}</strong>
                <div className="focus-metrics">
                  <span className="metric-pill">
                    {copy.incoming} {formatCount(splitData.incoming.length)}
                  </span>
                  <span className="metric-pill">
                    {copy.outgoing} {formatCount(splitData.outgoing.length)}
                  </span>
                </div>
                <dl className="info-list">
                  <div>
                    <dt>{copy.kind}</dt>
                    <dd>{presentSymbolKind(currentSymbol.kind, locale)}</dd>
                  </div>
                  <div>
                    <dt>{copy.location}</dt>
                    <dd>
                      {compactPath(currentSymbol.file_path, 4)}:{currentSymbol.line}
                    </dd>
                  </div>
                </dl>
              </div>

              <div className="sub-list">
                <div className="block-subheading">{copy.siblings}</div>
                {fileInterfacesQuery.isLoading ? (
                  <div className="empty-inline">...</div>
                ) : sameFileInterfaces.length ? (
                  sameFileInterfaces.slice(0, 12).map((symbol) => (
                    <button
                      key={symbol.id}
                      type="button"
                      className="sub-item"
                      onClick={() => focusInterface(symbol.qualified_name, symbol)}
                    >
                      <span>{symbol.name}</span>
                      <small>{presentSymbolKind(symbol.kind, locale)}</small>
                    </button>
                  ))
                ) : (
                  <div className="empty-inline">{copy.noSiblings}</div>
                )}
              </div>
            </>
          ) : (
            <div className="empty-card empty-card--tall">
              <strong>{copy.noFocus}</strong>
              <span>{copy.noFocusHint}</span>
            </div>
          )}
        </section>
      </aside>

      <main className="workspace">
        <div className="graph-board">
          <div className="graph-overlay graph-overlay--left">
            {currentSymbol ? (
              <div className="overlay-card overlay-card--title overlay-card--focus-title">
                <strong>{currentSymbol.name}</strong>
                <span>{currentSymbol.qualified_name}</span>
                <div className="overlay-card__metric-line">
                  <span>
                    {copy.incoming} {formatCount(splitData.incoming.length)}
                  </span>
                  <span>
                    {copy.outgoing} {formatCount(splitData.outgoing.length)}
                  </span>
                </div>
                <dl className="overlay-info-list">
                  <div>
                    <dt>{copy.kind}</dt>
                    <dd>{presentSymbolKind(currentSymbol.kind, locale)}</dd>
                  </div>
                  <div>
                    <dt>{copy.location}</dt>
                    <dd>
                      {compactPath(currentSymbol.file_path, 4)}:{currentSymbol.line}
                    </dd>
                  </div>
                </dl>
              </div>
            ) : (
              <>
                <div className="overlay-card overlay-card--title">
                  <strong>{activeProject?.name ?? copy.cloud}</strong>
                  <span>
                    {activeProject
                      ? compactPath(activeProject.path, 5)
                      : copy.overviewEmpty}
                  </span>
                </div>

                {activeProject ? (
                  <div className="overlay-card overlay-card--stats">
                    <span>
                      {copy.files}{" "}
                      {formatCount(
                        projectSummary?.stats?.files ?? projectOverview?.displayed_symbols,
                      )}
                    </span>
                    <span>
                      {copy.symbols}{" "}
                      {formatCount(
                        projectSummary?.stats?.symbols ?? projectOverview?.displayed_symbols,
                      )}
                    </span>
                    <span>
                      {copy.dependencies}{" "}
                      {formatCount(
                        projectSummary?.stats?.dependencies ?? projectOverview?.displayed_edges,
                      )}
                    </span>
                  </div>
                ) : null}
              </>
            )}
          </div>

          <div className="graph-overlay graph-overlay--right">
            {currentSymbol ? (
              <>
                <button type="button" className="overlay-button" onClick={clearFocus}>
                  {copy.backToCloud}
                </button>
                <label className="overlay-control">
                  <span>{copy.direction}</span>
                  <select
                    value={direction}
                    onChange={(event) =>
                      setDirection(event.target.value as DependencyDirection)
                    }
                  >
                    <option value="both">{copy.both}</option>
                    <option value="incoming">{copy.incoming}</option>
                    <option value="outgoing">{copy.outgoing}</option>
                  </select>
                </label>
                <label className="overlay-control">
                  <span>{copy.depth}</span>
                  <select
                    value={maxDepth}
                    onChange={(event) => setMaxDepth(Number(event.target.value))}
                  >
                    {[1, 2, 3, 4].map((depth) => (
                      <option key={depth} value={depth}>
                        {depth}
                      </option>
                    ))}
                  </select>
                </label>
              </>
            ) : (
              <>
                <label className="overlay-control">
                  <span>{copy.graphScale}</span>
                  <select
                    value={overviewLimit}
                    onChange={(event) => setOverviewLimit(Number(event.target.value))}
                  >
                    {[40, 60, 80, 120].map((limit) => (
                      <option key={limit} value={limit}>
                        {limit}
                      </option>
                    ))}
                  </select>
                </label>
              </>
            )}
          </div>

          <div className="graph-overlay graph-overlay--bottom-left">
            {currentSymbol ? (
              <div className="overlay-card overlay-card--legend">
                <strong>{copy.relationLegend}</strong>
                <span className="legend-row">
                  <i className="legend-line legend-line--direct legend-line--outgoing" />
                  {copy.directRelation}
                </span>
                <span className="legend-row">
                  <i className="legend-line legend-line--indirect legend-line--outgoing" />
                  {copy.indirectRelation}
                </span>
                <span className="legend-row">
                  <i className="legend-line legend-line--weighted" />
                  {copy.weightedRelation}
                </span>
                <strong>{copy.directionLegend}</strong>
                <span className="legend-row">
                  <i className="legend-line legend-line--direct legend-line--outgoing" />
                  {copy.outgoingRelation}
                </span>
                <span className="legend-row">
                  <i className="legend-line legend-line--direct legend-line--incoming" />
                  {copy.incomingRelation}
                </span>
              </div>
            ) : (
              <div className="overlay-card overlay-card--legend">
                <strong>{copy.relationLegend}</strong>
                <span className="legend-row">
                  <i className="legend-line legend-line--cloud" />
                  {copy.directRelation}
                </span>
                <span className="legend-row">
                  <i className="legend-line legend-line--weighted" />
                  {copy.weightedRelation}
                </span>
              </div>
            )}
          </div>

          <div className="graph-canvas">
            {currentSymbol ? (
              dependenciesQuery.isError ? (
                <div className="graph-empty">{errorText(dependenciesQuery.error, locale)}</div>
              ) : (
                <DependencyGraphView
                  locale={locale}
                  symbol={currentSymbol}
                  dependencies={dependenciesQuery.data}
                  onSelectInterface={(key) => focusInterface(key)}
                />
              )
            ) : !activeProject ? (
              <div className="graph-empty">{copy.noProjects}</div>
            ) : overviewQuery.isError ? (
              <div className="graph-empty">{errorText(overviewQuery.error, locale)}</div>
            ) : activeProject &&
              typeof activeProject.state === "object" &&
              "Loading" in activeProject.state ? (
              <div className="graph-empty">{copy.overviewLoading}</div>
            ) : projectOverview?.nodes.length ? (
              <ProjectOverviewGraphView
                locale={locale}
                overview={projectOverview}
                onSelectInterface={(key) => focusInterface(key)}
              />
            ) : (
              <div className="graph-empty">{copy.overviewEmpty}</div>
            )}
          </div>
        </div>
      </main>
    </div>
  );
}
