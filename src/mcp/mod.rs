//! MCP server implementation for QuickDep.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::time::Duration;

use anyhow::{anyhow, Context};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        AnnotateAble, ListResourceTemplatesResult, ListResourcesResult, PaginatedRequestParams,
        RawResource, RawResourceTemplate, ReadResourceRequestParams, ReadResourceResult,
        ResourceContents, ServerCapabilities, ServerInfo,
    },
    schemars::JsonSchema,
    tool, tool_handler, tool_router, ErrorData as McpError, Json, ServerHandler, ServiceExt,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{sync::mpsc, task::spawn_blocking};
use tracing::{debug, info, warn};

use rmcp::schemars;

use crate::{
    cache::{QueryCache, SymbolIndexCache},
    config::{load_settings, Settings},
    core::Symbol,
    project::{
        get_manifest_path, ManagerError, Project, ProjectConfig, ProjectId, ProjectManager,
        ProjectState,
    },
    security::validate_project_id,
    storage::{Storage, StorageError},
    watcher::{EventDebouncer, FileSystemWatcher},
};

const DEFAULT_SEARCH_LIMIT: usize = 20;
const DEFAULT_DEPENDENCY_DEPTH: u32 = 3;
const DEFAULT_OVERVIEW_SYMBOL_LIMIT: usize = 80;
const DEFAULT_OVERVIEW_EDGE_LIMIT: usize = 160;
const MAX_OVERVIEW_SYMBOL_LIMIT: usize = 500;
const MAX_OVERVIEW_EDGE_LIMIT: usize = 1_000;
const DEFAULT_CONTEXT_SYMBOL_LIMIT: usize = 5;
const DEFAULT_CONTEXT_FILE_LIMIT: usize = 5;
const DEFAULT_CONTEXT_EXPANSIONS: u32 = 1;
const SOURCE_SNIPPET_CONTEXT_BEFORE: usize = 1;
const SOURCE_SNIPPET_CONTEXT_AFTER: usize = 8;
const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(30);
const WATCH_DEBOUNCE_DELAY: Duration = Duration::from_millis(500);
const PROJECT_LOAD_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const PROJECT_LOAD_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(25);

const WORKFLOW_STATUS_TERMS: &[&str] = &[
    "queued",
    "running",
    "waitingapproval",
    "waiting approval",
    "blocked",
    "stuck",
    "排队",
    "队列",
    "运行",
    "卡住",
    "停留",
    "状态",
];
const WORKFLOW_TRANSITION_TERMS: &[&str] = &[
    "after",
    "still",
    "enter",
    "into",
    "transition",
    "flow",
    "resume",
    "dispatch",
    "through",
    "之后",
    "通过后",
    "仍然",
    "进入",
    "流转",
    "恢复",
    "调度",
];
const WORKFLOW_SCHEDULING_TERMS: &[&str] = &[
    "approval",
    "approve",
    "resume",
    "dispatch",
    "scheduler",
    "worker",
    "claim",
    "queue",
    "queued",
    "admit",
    "dispatchable",
    "execution",
    "turn",
    "审批",
    "通过",
    "恢复",
    "调度",
    "worker",
    "执行",
];

const WORKFLOW_APPROVAL_QUESTION_TERMS: &[&str] = &["approval", "approve", "审批", "通过"];
const WORKFLOW_APPROVAL_SEARCH_TERMS: &[&str] = &[
    "approve_pending_approval",
    "approval_resolve",
    "approval",
    "approve",
];
const WORKFLOW_RESUME_QUESTION_TERMS: &[&str] = &["resume", "恢复"];
const WORKFLOW_RESUME_SEARCH_TERMS: &[&str] = &["resume_approved_execution", "resume", "approved"];
const WORKFLOW_DISPATCH_QUESTION_TERMS: &[&str] = &["dispatch", "running", "进入", "运行", "调度"];
const WORKFLOW_DISPATCH_SEARCH_TERMS: &[&str] = &[
    "dispatch_execution",
    "dispatch",
    "prepare_execution_dispatch",
    "prepare",
];
const WORKFLOW_QUEUE_QUESTION_TERMS: &[&str] = &["queue", "queued", "排队", "队列", "停留"];
const WORKFLOW_QUEUE_SEARCH_TERMS: &[&str] = &[
    "next_conflict_queue_head",
    "queue_for_conflict",
    "queue",
    "queued",
    "conflict",
];
const WORKFLOW_SCHEDULER_QUESTION_TERMS: &[&str] =
    &["scheduler", "claim", "worker", "admit", "调度"];
const WORKFLOW_SCHEDULER_SEARCH_TERMS: &[&str] = &[
    "admit",
    "dispatchable_head",
    "dispatchable",
    "claim",
    "scheduler",
];
const WORKFLOW_BOUNDARY_FILE_TERMS: &[&str] =
    &["control-plane", "server", "cli", "platform", "rpc", "sdk"];
const WORKFLOW_INTERNAL_FILE_TERMS: &[&str] = &[
    "store",
    "core_flow_service",
    "flow",
    "execution",
    "scheduler",
    "runtime",
];
const WORKFLOW_APPROVAL_PRIORITY_TERMS: &[&str] = &["approve_pending_approval", "approval_resolve"];
const WORKFLOW_RESUME_PRIORITY_TERMS: &[&str] = &["resume_approved_execution"];
const WORKFLOW_DISPATCH_PRIORITY_TERMS: &[&str] =
    &["dispatch_execution", "prepare_execution_dispatch"];
const WORKFLOW_QUEUE_PRIORITY_TERMS: &[&str] = &["next_conflict_queue_head", "queue_for_conflict"];
const WORKFLOW_SCHEDULER_PRIORITY_TERMS: &[&str] = &["admit", "dispatchable_head"];

type McpResult<T> = Result<T, McpError>;

/// Shared project selector used by MCP tools.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProjectTarget {
    /// Registered project ID.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Project root path. When omitted, the server workspace is used.
    #[serde(default)]
    pub path: Option<String>,
}

/// Parameters for `scan_project`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ScanProjectRequest {
    /// Target project.
    #[serde(default)]
    pub project: ProjectTarget,
    /// Whether to force a rebuild.
    #[serde(default)]
    pub rebuild: bool,
}

/// Parameters for status-like project operations.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ProjectStatusRequest {
    /// Target project.
    #[serde(default)]
    pub project: ProjectTarget,
}

/// Parameters for `get_project_overview`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ProjectOverviewRequest {
    /// Target project.
    #[serde(default)]
    pub project: ProjectTarget,
    /// Maximum number of graph nodes to return.
    #[serde(default)]
    pub max_symbols: Option<usize>,
    /// Maximum number of graph edges to return.
    #[serde(default)]
    pub max_edges: Option<usize>,
}

/// Parameters for `find_interfaces`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FindInterfacesRequest {
    /// Target project.
    #[serde(default)]
    pub project: ProjectTarget,
    /// Fuzzy interface query.
    pub query: String,
    /// Maximum number of interfaces to return.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Parameters for interface lookup tools.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct InterfaceLookupRequest {
    /// Target project.
    #[serde(default)]
    pub project: ProjectTarget,
    /// Symbol ID, qualified name, or exact symbol name.
    pub interface: String,
}

/// Parameters for `get_dependencies`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DependenciesRequest {
    /// Target project.
    #[serde(default)]
    pub project: ProjectTarget,
    /// Symbol ID, qualified name, or exact symbol name.
    pub interface: String,
    /// Dependency direction: `outgoing`, `incoming`, or `both`.
    #[serde(default)]
    pub direction: Option<String>,
    /// Maximum graph depth.
    #[serde(default)]
    pub max_depth: Option<u32>,
}

/// Parameters for `get_call_chain`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CallChainRequest {
    /// Target project.
    #[serde(default)]
    pub project: ProjectTarget,
    /// Source symbol ID, qualified name, or exact symbol name.
    pub from_interface: String,
    /// Destination symbol ID, qualified name, or exact symbol name.
    pub to_interface: String,
    /// Maximum graph depth.
    #[serde(default)]
    pub max_depth: Option<u32>,
}

/// Parameters for `get_file_interfaces`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FileInterfacesRequest {
    /// Target project.
    #[serde(default)]
    pub project: ProjectTarget,
    /// File path relative to the project root.
    pub file_path: String,
}

/// Workspace-local context supplied by an agent UI.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TaskContextWorkspace {
    /// Currently active file, relative to the project root when possible.
    #[serde(default)]
    pub active_file: Option<String>,
    /// Explicit symbol selected by the agent or editor.
    #[serde(default)]
    pub selection_symbol: Option<String>,
    /// Current cursor or selection line inside `active_file`.
    #[serde(default)]
    pub selection_line: Option<u32>,
    /// Recently viewed or edited files.
    #[serde(default)]
    pub recent_files: Vec<String>,
}

/// Runtime hints supplied by the agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TaskContextRuntime {
    /// Symbols observed in a stack trace or runtime report.
    #[serde(default)]
    pub stacktrace_symbols: Vec<String>,
    /// Name of a failing test related to the current task.
    #[serde(default)]
    pub failing_test: Option<String>,
}

/// Conversation hints from earlier turns.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TaskContextConversation {
    /// Previously discussed target symbols.
    #[serde(default)]
    pub previous_targets: Vec<String>,
    /// Previously inferred scene, if any.
    #[serde(default)]
    pub previous_scene: Option<String>,
}

/// Parameters for `get_task_context`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskContextRequest {
    /// Target project.
    #[serde(default)]
    pub project: ProjectTarget,
    /// Natural-language question from the agent or user.
    #[serde(default)]
    pub question: Option<String>,
    /// Explicit symbol anchors provided by the caller.
    #[serde(default)]
    pub anchor_symbols: Vec<String>,
    /// Explicit file anchors provided by the caller.
    #[serde(default)]
    pub anchor_files: Vec<String>,
    /// Requested scene: `auto`, `locate`, `behavior`, `impact`, `workflow`, `call_chain`, or `watcher`.
    #[serde(default)]
    pub mode: Option<String>,
    /// Context budget: `lean`, `normal`, or `wide`.
    #[serde(default)]
    pub budget: Option<String>,
    /// Whether small source snippets may be returned.
    #[serde(default)]
    pub allow_source_snippets: Option<bool>,
    /// Maximum number of automatic context expansions the server may attempt.
    #[serde(default)]
    pub max_expansions: Option<u32>,
    /// Editor or workspace context.
    #[serde(default)]
    pub workspace: Option<TaskContextWorkspace>,
    /// Runtime hints such as stack traces.
    #[serde(default)]
    pub runtime: Option<TaskContextRuntime>,
    /// Conversation hints from earlier turns.
    #[serde(default)]
    pub conversation: Option<TaskContextConversation>,
}

/// One query inside `batch_query`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BatchQueryItem {
    /// Query kind.
    pub kind: String,
    /// Search query for `find_interfaces`.
    #[serde(default)]
    pub query: Option<String>,
    /// Interface selector for `get_interface` and `get_dependencies`.
    #[serde(default)]
    pub interface: Option<String>,
    /// File path for `get_file_interfaces`.
    #[serde(default)]
    pub file_path: Option<String>,
    /// Source interface for `get_call_chain`.
    #[serde(default)]
    pub from_interface: Option<String>,
    /// Destination interface for `get_call_chain`.
    #[serde(default)]
    pub to_interface: Option<String>,
    /// Dependency direction for `get_dependencies`.
    #[serde(default)]
    pub direction: Option<String>,
    /// Search limit for `find_interfaces`.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Graph depth for dependency or call-chain queries.
    #[serde(default)]
    pub max_depth: Option<u32>,
}

/// Parameters for `batch_query`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BatchQueryRequest {
    /// Target project.
    #[serde(default)]
    pub project: ProjectTarget,
    /// Queries to execute.
    pub queries: Vec<BatchQueryItem>,
}

#[derive(Debug, Clone, Serialize)]
struct ProjectRecord {
    id: String,
    name: String,
    path: String,
    state: ProjectState,
    is_default: bool,
}

#[derive(Debug)]
struct ProjectCacheState {
    symbol_index: SymbolIndexCache,
    query_cache: QueryCache<Value>,
    symbol_index_ready: AtomicBool,
}

impl Default for ProjectCacheState {
    fn default() -> Self {
        Self {
            symbol_index: SymbolIndexCache::new(),
            query_cache: QueryCache::default(),
            symbol_index_ready: AtomicBool::new(false),
        }
    }
}

impl ProjectCacheState {
    fn clear(&self) {
        self.symbol_index.clear();
        self.query_cache.clear();
        self.symbol_index_ready.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug)]
enum WatchCommand {
    Pause,
    Resume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskContextMode {
    Auto,
    Locate,
    Behavior,
    Impact,
    Workflow,
    CallChain,
    Watcher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TaskContextBudget {
    Lean,
    Normal,
    Wide,
}

impl TaskContextMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Locate => "locate",
            Self::Behavior => "behavior",
            Self::Impact => "impact",
            Self::Workflow => "workflow",
            Self::CallChain => "call_chain",
            Self::Watcher => "watcher",
        }
    }
}

impl TaskContextBudget {
    fn as_str(self) -> &'static str {
        match self {
            Self::Lean => "lean",
            Self::Normal => "normal",
            Self::Wide => "wide",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct TaskContextLimits {
    primary_symbols: usize,
    primary_files: usize,
    related_files: usize,
    direct_neighbors: usize,
    same_file_symbols: usize,
    source_snippets: usize,
    workflow_symbols: usize,
    workflow_depth: u32,
}

#[derive(Debug, Clone)]
struct ResolvedTaskContextAnchor {
    symbol: Symbol,
    source: &'static str,
}

#[derive(Debug, Clone)]
struct ContextDependency {
    symbol: Symbol,
    direction: &'static str,
    dep_kind: Option<String>,
}

#[derive(Debug, Clone)]
struct SymbolContext {
    callers: Vec<ContextDependency>,
    callees: Vec<ContextDependency>,
    same_file_symbols: Vec<Symbol>,
}

#[derive(Debug, Clone, Default)]
struct ResolvedTaskContext {
    symbols: Vec<ResolvedTaskContextAnchor>,
    files: Vec<String>,
    sources: Vec<String>,
    penalties: Vec<String>,
}

#[derive(Debug)]
struct TaskContextBuild {
    status: &'static str,
    coverage: &'static str,
    package: Value,
    graph_signals: Vec<String>,
    expansion_hint: Option<&'static str>,
    next_tool_calls: Vec<Value>,
    fallback_to_code: bool,
    note: String,
    confidence_delta: f64,
}

#[derive(Debug, Clone, Copy)]
struct WorkflowPhaseSpec {
    key: &'static str,
    label: &'static str,
    question_terms: &'static [&'static str],
    search_terms: &'static [&'static str],
}

#[derive(Debug, Clone)]
struct WorkflowCandidate {
    symbol: Symbol,
    depth: u32,
}

#[derive(Debug, Clone)]
struct WorkflowPhaseSupport {
    symbol: Symbol,
    depth: u32,
    score: i32,
}

#[derive(Debug, Clone)]
struct WorkflowPhaseSelection {
    phase: WorkflowPhaseSpec,
    symbol: Symbol,
    depth: u32,
    score: i32,
    supporting: Vec<WorkflowPhaseSupport>,
}

const WORKFLOW_PHASES: [WorkflowPhaseSpec; 5] = [
    WorkflowPhaseSpec {
        key: "approval",
        label: "approval gate",
        question_terms: WORKFLOW_APPROVAL_QUESTION_TERMS,
        search_terms: WORKFLOW_APPROVAL_SEARCH_TERMS,
    },
    WorkflowPhaseSpec {
        key: "resume",
        label: "resume step",
        question_terms: WORKFLOW_RESUME_QUESTION_TERMS,
        search_terms: WORKFLOW_RESUME_SEARCH_TERMS,
    },
    WorkflowPhaseSpec {
        key: "dispatch",
        label: "dispatch step",
        question_terms: WORKFLOW_DISPATCH_QUESTION_TERMS,
        search_terms: WORKFLOW_DISPATCH_SEARCH_TERMS,
    },
    WorkflowPhaseSpec {
        key: "queue",
        label: "queue gate",
        question_terms: WORKFLOW_QUEUE_QUESTION_TERMS,
        search_terms: WORKFLOW_QUEUE_SEARCH_TERMS,
    },
    WorkflowPhaseSpec {
        key: "scheduler",
        label: "scheduler gate",
        question_terms: WORKFLOW_SCHEDULER_QUESTION_TERMS,
        search_terms: WORKFLOW_SCHEDULER_SEARCH_TERMS,
    },
];

#[derive(Debug, Clone)]
struct ProjectWatcherState {
    dirty: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
struct ProjectWatcherHandle {
    command_tx: mpsc::UnboundedSender<WatchCommand>,
    state: ProjectWatcherState,
}

async fn run_project_watcher(
    project_id: ProjectId,
    project_root: PathBuf,
    cache_dir: PathBuf,
    manager: Arc<ProjectManager>,
    cache: Arc<ProjectCacheState>,
    state: ProjectWatcherState,
    mut command_rx: mpsc::UnboundedReceiver<WatchCommand>,
) -> anyhow::Result<()> {
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let watcher = FileSystemWatcher::new(&project_root, event_tx)?;
    let mut debouncer = EventDebouncer::new(WATCH_DEBOUNCE_DELAY);

    loop {
        if debouncer.is_empty() {
            tokio::select! {
                maybe_command = command_rx.recv() => {
                    let Some(command) = maybe_command else {
                        break;
                    };
                    handle_watch_command(
                        &watcher,
                        &manager,
                        &cache,
                        &project_id,
                        &state,
                        command,
                    )
                        .await;
                }
                maybe_event = event_rx.recv() => {
                    let Some(event) = maybe_event else {
                        break;
                    };
                    if should_ignore_watch_path(&cache_dir, &event.path) {
                        continue;
                    }
                    state.dirty.store(true, Ordering::SeqCst);
                    debouncer.push(event);
                }
            }
            continue;
        }

        tokio::select! {
            maybe_command = command_rx.recv() => {
                let Some(command) = maybe_command else {
                    break;
                };
                handle_watch_command(
                    &watcher,
                    &manager,
                    &cache,
                    &project_id,
                    &state,
                    command,
                )
                    .await;
            }
            maybe_event = event_rx.recv() => {
                let Some(event) = maybe_event else {
                    break;
                };
                if should_ignore_watch_path(&cache_dir, &event.path) {
                    continue;
                }
                state.dirty.store(true, Ordering::SeqCst);
                debouncer.push(event);
            }
            _ = tokio::time::sleep(debouncer.delay()) => {
                if !debouncer.ready() {
                    continue;
                }

                let event_count = debouncer.drain().len();
                match manager.scan(&project_id, false).await {
                    Ok(()) => {
                        state.dirty.store(false, Ordering::SeqCst);
                        cache.clear();
                        debug!(
                            project_id = %project_id,
                            events = event_count,
                            "applied debounced watcher update"
                        );
                    }
                    Err(ManagerError::InvalidOperation(_)) => {
                        debug!(
                            project_id = %project_id,
                            "skipping watcher update while project is already scanning"
                        );
                    }
                    Err(error) => {
                        warn!(
                            project_id = %project_id,
                            error = %error,
                            "watcher-triggered incremental scan failed"
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_watch_command(
    watcher: &FileSystemWatcher,
    manager: &ProjectManager,
    cache: &ProjectCacheState,
    project_id: &ProjectId,
    state: &ProjectWatcherState,
    command: WatchCommand,
) {
    match command {
        WatchCommand::Pause => {
            watcher.pause();
            state.paused.store(true, Ordering::SeqCst);
        }
        WatchCommand::Resume => {
            state.paused.store(false, Ordering::SeqCst);
            if watcher.resume() {
                match manager.scan(project_id, false).await {
                    Ok(()) => {
                        state.dirty.store(false, Ordering::SeqCst);
                        cache.clear();
                        debug!(
                            project_id = %project_id,
                            "resumed watcher and reconciled missed filesystem events"
                        );
                    }
                    Err(ManagerError::InvalidOperation(_)) => {
                        debug!(
                            project_id = %project_id,
                            "skipping watcher resume refresh while project is already scanning"
                        );
                    }
                    Err(error) => {
                        warn!(
                            project_id = %project_id,
                            error = %error,
                            "failed to reconcile paused watcher state"
                        );
                    }
                }
            }
        }
    }
}

fn should_ignore_watch_path(cache_dir: &Path, path: &Path) -> bool {
    path.starts_with(cache_dir)
}

/// QuickDep MCP server.
#[derive(Debug, Clone)]
pub struct QuickDepServer {
    workspace_root: PathBuf,
    default_project_id: ProjectId,
    manager: Arc<ProjectManager>,
    caches: Arc<RwLock<HashMap<ProjectId, Arc<ProjectCacheState>>>>,
    watchers: Arc<RwLock<HashMap<ProjectId, ProjectWatcherHandle>>>,
    idle_checker_started: Arc<AtomicBool>,
    enabled_tools: Arc<HashSet<String>>,
    tool_router: ToolRouter<Self>,
}

impl QuickDepServer {
    /// Create a server for the given workspace root and register the workspace project.
    pub async fn from_workspace(workspace_root: impl AsRef<Path>) -> anyhow::Result<Self> {
        Self::from_workspace_inner(workspace_root.as_ref(), None).await
    }

    /// Create a server that exposes only a selected subset of tools.
    pub async fn from_workspace_with_tools(
        workspace_root: impl AsRef<Path>,
        allowed_tools: Vec<String>,
    ) -> anyhow::Result<Self> {
        Self::from_workspace_inner(workspace_root.as_ref(), Some(allowed_tools)).await
    }

    async fn from_workspace_inner(
        workspace_root: &Path,
        allowed_tools: Option<Vec<String>>,
    ) -> anyhow::Result<Self> {
        let workspace_root = workspace_root.canonicalize().with_context(|| {
            format!(
                "failed to canonicalize workspace root {}",
                workspace_root.display()
            )
        })?;

        if !workspace_root.is_dir() {
            return Err(anyhow!(
                "workspace root must be a directory: {}",
                workspace_root.display()
            ));
        }

        let manifest_path = get_manifest_path(&workspace_root);
        let manager = Arc::new(ProjectManager::with_scanner(&manifest_path).await);
        let default_project_id =
            Self::register_project_with_manager(manager.as_ref(), &workspace_root).await?;
        let (tool_router, enabled_tools) = Self::build_tool_router(allowed_tools.as_deref())?;

        Ok(Self {
            workspace_root,
            default_project_id,
            manager,
            caches: Arc::new(RwLock::new(HashMap::new())),
            watchers: Arc::new(RwLock::new(HashMap::new())),
            idle_checker_started: Arc::new(AtomicBool::new(false)),
            enabled_tools: Arc::new(enabled_tools),
            tool_router,
        })
    }

    /// Run the MCP server over stdio.
    pub async fn serve_stdio(workspace_root: impl AsRef<Path>) -> anyhow::Result<()> {
        Self::from_workspace(workspace_root)
            .await?
            .serve(rmcp::transport::io::stdio())
            .await?
            .waiting()
            .await?;
        Ok(())
    }

    fn build_tool_router(
        allowed_tools: Option<&[String]>,
    ) -> anyhow::Result<(ToolRouter<Self>, HashSet<String>)> {
        let mut tool_router = Self::tool_router();
        let available_tools = tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect::<Vec<_>>();
        let available_tool_set = available_tools.iter().cloned().collect::<HashSet<_>>();

        let Some(allowed_tools) = allowed_tools else {
            return Ok((tool_router, available_tool_set));
        };

        let selected_tools = allowed_tools
            .iter()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .map(ToOwned::to_owned)
            .collect::<HashSet<_>>();
        if selected_tools.is_empty() {
            return Err(anyhow!("tool filter cannot be empty"));
        }

        let mut unknown_tools = selected_tools
            .difference(&available_tool_set)
            .cloned()
            .collect::<Vec<_>>();
        if !unknown_tools.is_empty() {
            unknown_tools.sort();
            return Err(anyhow!(
                "unknown tools: {}. Available tools: {}",
                unknown_tools.join(", "),
                available_tools.join(", ")
            ));
        }

        tool_router
            .map
            .retain(|name, _| selected_tools.contains(name.as_ref()));

        Ok((tool_router, selected_tools))
    }

    /// Return whether a tool is enabled on this server instance.
    #[must_use]
    pub fn is_tool_enabled(&self, name: &str) -> bool {
        self.enabled_tools.contains(name)
    }

    fn disabled_tool_error(name: &str) -> McpError {
        Self::invalid_params(format!(
            "tool '{}' is disabled by server configuration",
            name
        ))
    }

    fn project_cache(&self, project_id: &ProjectId) -> Arc<ProjectCacheState> {
        if let Some(cache) = self
            .caches
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(project_id)
        {
            return cache.clone();
        }

        let mut caches = self
            .caches
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        caches
            .entry(project_id.clone())
            .or_insert_with(|| Arc::new(ProjectCacheState::default()))
            .clone()
    }

    fn clear_project_cache(&self, project_id: &ProjectId) {
        let cache = self.project_cache(project_id);
        cache.clear();
    }

    fn handle_dirty_watcher_refresh_result(
        project_id: &ProjectId,
        dirty: &AtomicBool,
        cache: &ProjectCacheState,
        result: Result<(), ManagerError>,
    ) -> McpResult<()> {
        match result {
            Ok(()) => {
                dirty.store(false, Ordering::SeqCst);
                cache.clear();
                Ok(())
            }
            Err(ManagerError::InvalidOperation(_)) => {
                debug!(
                    project_id = %project_id,
                    "skipping watcher refresh while project is already scanning"
                );
                Ok(())
            }
            Err(error) => Err(Self::manager_error(error)),
        }
    }

    async fn wait_for_project_load_completion(&self, project_id: &ProjectId) -> McpResult<Project> {
        let deadline = tokio::time::Instant::now() + PROJECT_LOAD_WAIT_TIMEOUT;

        loop {
            let project = self
                .manager
                .get(project_id)
                .await
                .map_err(Self::manager_error)?
                .ok_or_else(|| {
                    Self::invalid_params(format!("unknown project id: {}", project_id))
                })?;

            if !matches!(project.state, ProjectState::Loading { .. }) {
                return Ok(project);
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(Self::internal_error(format!(
                    "timed out waiting for project {} to finish loading",
                    project_id
                )));
            }

            tokio::time::sleep(PROJECT_LOAD_WAIT_POLL_INTERVAL).await;
        }
    }

    async fn ensure_project_watcher(&self, project: &Project) -> McpResult<()> {
        if !project.is_loaded() {
            return Ok(());
        }

        let handle = {
            let mut watchers = self
                .watchers
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(handle) = watchers.get(&project.id) {
                handle.clone()
            } else {
                let handle = Self::spawn_project_watcher(
                    project.id.clone(),
                    project.path.clone(),
                    project.cache_dir(),
                    self.manager.clone(),
                    self.project_cache(&project.id),
                );
                watchers.insert(project.id.clone(), handle.clone());
                handle
            }
        };

        let command = if project.is_watching() {
            WatchCommand::Resume
        } else {
            WatchCommand::Pause
        };
        handle.command_tx.send(command).map_err(|error| {
            Self::internal_error(format!("failed to control watcher: {}", error))
        })?;
        Self::wait_for_watcher_pause_state(&handle, !project.is_watching()).await?;

        if handle.state.dirty.load(Ordering::SeqCst) {
            let cache = self.project_cache(&project.id);
            Self::handle_dirty_watcher_refresh_result(
                &project.id,
                handle.state.dirty.as_ref(),
                cache.as_ref(),
                self.manager.scan(&project.id, false).await,
            )?;
        }

        Ok(())
    }

    async fn wait_for_watcher_pause_state(
        handle: &ProjectWatcherHandle,
        paused: bool,
    ) -> McpResult<()> {
        for _ in 0..40 {
            if handle.state.paused.load(Ordering::SeqCst) == paused {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        Err(Self::internal_error(format!(
            "timed out waiting for watcher to {}",
            if paused { "pause" } else { "resume" }
        )))
    }

    fn spawn_project_watcher(
        project_id: ProjectId,
        project_root: PathBuf,
        cache_dir: PathBuf,
        manager: Arc<ProjectManager>,
        cache: Arc<ProjectCacheState>,
    ) -> ProjectWatcherHandle {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let state = ProjectWatcherState {
            dirty: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
        };
        let state_for_task = state.clone();
        tokio::spawn(async move {
            if let Err(error) = run_project_watcher(
                project_id,
                project_root,
                cache_dir,
                manager,
                cache,
                state_for_task,
                command_rx,
            )
            .await
            {
                warn!("Project watcher stopped: {}", error);
            }
        });
        ProjectWatcherHandle { command_tx, state }
    }

    async fn register_project_with_manager(
        manager: &ProjectManager,
        project_root: &Path,
    ) -> anyhow::Result<ProjectId> {
        let settings = load_settings(project_root).with_context(|| {
            format!(
                "failed to load quickdep settings from {}",
                project_root.display()
            )
        })?;
        let config = Self::project_config_from_settings(&settings);
        let name = project_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace")
            .to_string();

        manager
            .register_or_update(project_root, name, Some(config))
            .await
            .map_err(|error| anyhow!(error))
    }

    fn project_config_from_settings(settings: &Settings) -> ProjectConfig {
        ProjectConfig {
            include: settings.scan.include.clone(),
            exclude: settings.scan.exclude.clone(),
            languages: settings.scan.languages.clone(),
            include_tests: settings.scan.include_tests,
            parser_map: settings.parser.map.clone(),
            idle_timeout_secs: settings.watcher.idle_timeout.as_secs(),
        }
    }

    fn normalize_project_root(&self, path: &str) -> McpResult<PathBuf> {
        let candidate = PathBuf::from(path);
        let project_root = if candidate.is_absolute() {
            candidate
        } else {
            self.workspace_root.join(candidate)
        };

        if !project_root.exists() {
            return Err(Self::invalid_params(format!(
                "project path does not exist: {}",
                project_root.display()
            )));
        }

        if !project_root.is_dir() {
            return Err(Self::invalid_params(format!(
                "project path must be a directory: {}",
                project_root.display()
            )));
        }

        project_root.canonicalize().map_err(|error| {
            Self::invalid_params(format!(
                "failed to canonicalize project path {}: {}",
                project_root.display(),
                error
            ))
        })
    }

    async fn ensure_registered_project(&self, project_root: &Path) -> McpResult<ProjectId> {
        let id = ProjectId::from_path(project_root).map_err(Self::project_id_error)?;
        if self.manager.exists(&id).await {
            return Ok(id);
        }

        Self::register_project_with_manager(self.manager.as_ref(), project_root)
            .await
            .map_err(|error| Self::internal_error(error.to_string()))
    }

    async fn resolve_project_id(
        &self,
        target: &ProjectTarget,
        auto_register: bool,
    ) -> McpResult<ProjectId> {
        match (&target.project_id, &target.path) {
            (Some(project_id), Some(path)) => {
                let project_root = self.normalize_project_root(path)?;
                validate_project_id(&project_root, project_id).map_err(|error| {
                    Self::invalid_params(format!("project selection mismatch: {}", error))
                })?;

                if auto_register {
                    self.ensure_registered_project(&project_root).await
                } else {
                    let id = ProjectId::from_str(project_id).map_err(Self::project_id_error)?;
                    if self.manager.exists(&id).await {
                        Ok(id)
                    } else {
                        Err(Self::invalid_params(format!(
                            "unknown project id: {}",
                            project_id
                        )))
                    }
                }
            }
            (Some(project_id), None) => {
                let id = ProjectId::from_str(project_id).map_err(Self::project_id_error)?;
                if self.manager.exists(&id).await {
                    Ok(id)
                } else {
                    Err(Self::invalid_params(format!(
                        "unknown project id: {}",
                        project_id
                    )))
                }
            }
            (None, Some(path)) => {
                let project_root = self.normalize_project_root(path)?;
                if auto_register {
                    self.ensure_registered_project(&project_root).await
                } else {
                    let id = ProjectId::from_path(&project_root).map_err(Self::project_id_error)?;
                    if self.manager.exists(&id).await {
                        Ok(id)
                    } else {
                        Err(Self::invalid_params(format!(
                            "project is not registered: {}",
                            project_root.display()
                        )))
                    }
                }
            }
            (None, None) => Ok(self.default_project_id.clone()),
        }
    }

    async fn project_record(&self, project_id: &ProjectId) -> McpResult<ProjectRecord> {
        let manifest = self.manager.get_manifest().await;
        let state = self
            .manager
            .status(project_id)
            .await
            .map_err(Self::manager_error)?;

        if let Some(entry) = manifest.get_project(project_id) {
            return Ok(ProjectRecord {
                id: entry.id.as_str().to_string(),
                name: entry.name.clone(),
                path: entry.path.clone(),
                state,
                is_default: *project_id == self.default_project_id,
            });
        }

        if *project_id == self.default_project_id {
            let name = self
                .workspace_root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("workspace")
                .to_string();
            return Ok(ProjectRecord {
                id: project_id.as_str().to_string(),
                name,
                path: self.workspace_root.display().to_string(),
                state,
                is_default: true,
            });
        }

        Err(Self::invalid_params(format!(
            "unknown project id: {}",
            project_id
        )))
    }

    async fn ensure_project_loaded(&self, target: &ProjectTarget) -> McpResult<Project> {
        let project_id = self.resolve_project_id(target, true).await?;
        let project = match self.manager.get(&project_id).await {
            Ok(Some(project)) => project,
            Ok(None) => {
                return Err(Self::invalid_params(format!(
                    "unknown project id: {}",
                    project_id
                )));
            }
            Err(ManagerError::InvalidOperation(_)) => {
                debug!(
                    project_id = %project_id,
                    "waiting for in-flight project load to finish"
                );
                self.wait_for_project_load_completion(&project_id).await?
            }
            Err(error) => return Err(Self::manager_error(error)),
        };

        let project = if matches!(project.state, ProjectState::Loading { .. }) {
            debug!(
                project_id = %project_id,
                "project is still loading, waiting for completion"
            );
            self.wait_for_project_load_completion(&project_id).await?
        } else {
            project
        };

        if let ProjectState::Failed { error, .. } = &project.state {
            return Err(Self::internal_error(format!(
                "project {} failed to load: {}",
                project_id, error
            )));
        }

        self.ensure_project_watcher(&project).await?;

        Ok(project)
    }

    async fn project_root_for_id(&self, project_id: &ProjectId) -> McpResult<PathBuf> {
        let manifest = self.manager.get_manifest().await;
        if let Some(entry) = manifest.get_project(project_id) {
            return Ok(PathBuf::from(&entry.path));
        }

        if *project_id == self.default_project_id {
            return Ok(self.workspace_root.clone());
        }

        Err(Self::invalid_params(format!(
            "unknown project id: {}",
            project_id
        )))
    }

    async fn ensure_symbol_index_loaded(
        &self,
        project: &Project,
    ) -> McpResult<Arc<ProjectCacheState>> {
        let cache = self.project_cache(&project.id);
        if cache.symbol_index_ready.load(Ordering::SeqCst) {
            return Ok(cache);
        }

        let cache_for_load = cache.clone();
        let database_path = project.database_path();

        Self::spawn_blocking(move || {
            if cache_for_load.symbol_index_ready.load(Ordering::SeqCst) {
                return Ok(());
            }

            cache_for_load.clear();

            let storage = Storage::new(&database_path).map_err(Self::storage_error_message)?;
            let symbols = Self::all_symbols(&storage)?;
            cache_for_load.symbol_index.insert_symbols(&symbols);
            cache_for_load
                .symbol_index_ready
                .store(true, Ordering::SeqCst);
            Ok(())
        })
        .await?;

        Ok(cache)
    }

    async fn refresh_project_cache(&self, project: &Project) -> McpResult<()> {
        let cache = self.project_cache(&project.id);
        let database_path = project.database_path();

        Self::spawn_blocking(move || {
            cache.clear();
            let storage = Storage::new(&database_path).map_err(Self::storage_error_message)?;
            let symbols = Self::all_symbols(&storage)?;
            cache.symbol_index.insert_symbols(&symbols);
            cache.symbol_index_ready.store(true, Ordering::SeqCst);
            Ok(())
        })
        .await
    }

    async fn blocking_storage<T, F>(database_path: PathBuf, operation: F) -> McpResult<T>
    where
        T: Send + 'static,
        F: FnOnce(Storage) -> Result<T, String> + Send + 'static,
    {
        Self::spawn_blocking(move || {
            let storage = Storage::new(&database_path).map_err(Self::storage_error_message)?;
            operation(storage)
        })
        .await
    }

    async fn spawn_blocking<T>(
        task: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> McpResult<T>
    where
        T: Send + 'static,
    {
        spawn_blocking(task)
            .await
            .map_err(|error| Self::internal_error(format!("blocking task failed: {}", error)))?
            .map_err(Self::internal_error)
    }

    fn all_symbols(storage: &Storage) -> Result<Vec<Symbol>, String> {
        let mut symbols = Vec::new();
        for file_state in storage
            .get_all_file_states()
            .map_err(Self::storage_error_message)?
        {
            symbols.extend(
                storage
                    .get_symbols_by_file(&file_state.path)
                    .map_err(Self::storage_error_message)?,
            );
        }
        Ok(symbols)
    }

    fn resolve_symbol_from_storage(
        storage: &Storage,
        symbol_index: &SymbolIndexCache,
        identifier: &str,
    ) -> Result<Symbol, String> {
        if let Some(symbol) = storage
            .get_symbol(identifier)
            .map_err(Self::storage_error_message)?
        {
            return Ok(symbol);
        }

        if let Some(symbol) = storage
            .get_symbol_by_qualified_name(identifier)
            .map_err(Self::storage_error_message)?
        {
            return Ok(symbol);
        }

        let exact_ids = symbol_index.get(identifier);
        if exact_ids.len() == 1 {
            return storage
                .get_symbol(&exact_ids[0])
                .map_err(Self::storage_error_message)?
                .ok_or_else(|| format!("interface '{}' not found", identifier));
        }

        if exact_ids.len() > 1 {
            let mut candidates = Vec::new();
            for symbol_id in exact_ids.into_iter().take(5) {
                if let Some(symbol) = storage
                    .get_symbol(&symbol_id)
                    .map_err(Self::storage_error_message)?
                {
                    candidates.push(symbol.qualified_name);
                }
            }
            return Err(format!(
                "interface '{}' is ambiguous; try one of: {}",
                identifier,
                candidates.join(", ")
            ));
        }

        Err(format!("interface '{}' not found", identifier))
    }

    fn normalize_file_path(project_root: &Path, file_path: &str) -> Result<String, String> {
        let canonical = crate::security::validate_path(project_root, file_path)
            .map_err(|error| error.to_string())?;
        let relative = canonical
            .strip_prefix(project_root)
            .map_err(|_| format!("file '{}' is outside the project root", file_path))?;
        Ok(relative.to_string_lossy().replace('\\', "/"))
    }

    async fn list_projects_value(&self) -> McpResult<Value> {
        let manifest = self.manager.get_manifest().await;
        let mut projects = Vec::new();

        for entry in manifest.projects {
            projects.push(
                serde_json::to_value(self.project_record(&entry.id).await?)
                    .map_err(Self::serialization_error)?,
            );
        }

        Ok(json!({
            "default_project_id": self.default_project_id.as_str(),
            "projects": projects,
        }))
    }

    async fn scan_project_value(&self, request: ScanProjectRequest) -> McpResult<Value> {
        let project_id = self.resolve_project_id(&request.project, true).await?;
        self.clear_project_cache(&project_id);
        self.manager
            .scan(&project_id, request.rebuild)
            .await
            .map_err(Self::manager_error)?;

        let project = self
            .manager
            .get(&project_id)
            .await
            .map_err(Self::manager_error)?
            .ok_or_else(|| Self::invalid_params(format!("unknown project id: {}", project_id)))?;

        if let ProjectState::Failed { error, .. } = &project.state {
            return Err(Self::internal_error(error.clone()));
        }

        self.refresh_project_cache(&project).await?;
        self.ensure_project_watcher(&project).await?;
        let record = self.project_record(&project_id).await?;
        let stats = Self::blocking_storage(project.database_path(), |storage| {
            let stats = storage.get_stats().map_err(Self::storage_error_message)?;
            serde_json::to_value(stats).map_err(|error| error.to_string())
        })
        .await?;

        Ok(json!({
            "project": record,
            "rebuild": request.rebuild,
            "stats": stats,
        }))
    }

    async fn get_scan_status_value(&self, request: ProjectStatusRequest) -> McpResult<Value> {
        let project_id = self.resolve_project_id(&request.project, true).await?;
        Ok(json!({
            "project": self.project_record(&project_id).await?,
        }))
    }

    async fn cancel_scan_value(&self, request: ProjectStatusRequest) -> McpResult<Value> {
        let project_id = self.resolve_project_id(&request.project, true).await?;
        self.manager
            .cancel_scan(&project_id)
            .await
            .map_err(Self::manager_error)?;

        Ok(json!({
            "cancel_requested": true,
            "project": self.project_record(&project_id).await?,
        }))
    }

    async fn get_project_overview_value(
        &self,
        request: ProjectOverviewRequest,
    ) -> McpResult<Value> {
        let max_symbols = request.max_symbols.unwrap_or(DEFAULT_OVERVIEW_SYMBOL_LIMIT);
        let max_edges = request.max_edges.unwrap_or(DEFAULT_OVERVIEW_EDGE_LIMIT);

        if max_symbols == 0 || max_edges == 0 {
            return Err(Self::invalid_params(
                "max_symbols and max_edges must be greater than 0",
            ));
        }

        let max_symbols = max_symbols.min(MAX_OVERVIEW_SYMBOL_LIMIT);
        let max_edges = max_edges.min(MAX_OVERVIEW_EDGE_LIMIT);
        let project = self.ensure_project_loaded(&request.project).await?;
        let project_record = self.project_record(&project.id).await?;

        let overview = Self::blocking_storage(project.database_path(), move |storage| {
            Self::build_project_overview(&storage, max_symbols, max_edges)
        })
        .await?;

        Ok(json!({
            "project": project_record,
            "overview": overview,
        }))
    }

    async fn find_interfaces_value(&self, request: FindInterfacesRequest) -> McpResult<Value> {
        let query = request.query.trim().to_string();
        if query.is_empty() {
            return Err(Self::invalid_params("query cannot be empty"));
        }

        let limit = request.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        if limit == 0 {
            return Err(Self::invalid_params("limit must be greater than 0"));
        }

        let project = self.ensure_project_loaded(&request.project).await?;
        let cache = self.ensure_symbol_index_loaded(&project).await?;
        let cache_key = format!("find_interfaces:{}:{}", query, limit);
        if let Some(cached) = cache.query_cache.get(&cache_key) {
            return Ok(cached);
        }

        let exact_ids = cache.symbol_index.get(&query);
        let query_for_search = query.clone();
        let result = Self::blocking_storage(project.database_path(), move |storage| {
            let mut interfaces = Vec::new();
            let mut seen = HashSet::new();

            for symbol_id in exact_ids {
                if let Some(symbol) = storage
                    .get_symbol(&symbol_id)
                    .map_err(Self::storage_error_message)?
                {
                    if seen.insert(symbol.id.clone()) {
                        interfaces.push(Self::interface_summary_value(&symbol));
                    }
                }
            }

            for symbol in storage
                .search_symbols(&query_for_search, limit)
                .map_err(Self::storage_error_message)?
            {
                if seen.insert(symbol.id.clone()) {
                    interfaces.push(Self::interface_summary_value(&symbol));
                }
                if interfaces.len() >= limit {
                    break;
                }
            }

            Ok(json!({
                "query": query_for_search,
                "limit": limit,
                "interfaces": interfaces,
            }))
        })
        .await?;

        cache.query_cache.insert(cache_key, result.clone());
        Ok(result)
    }

    async fn get_interface_value(&self, request: InterfaceLookupRequest) -> McpResult<Value> {
        let interface = request.interface.trim().to_string();
        if interface.is_empty() {
            return Err(Self::invalid_params("interface cannot be empty"));
        }

        let project = self.ensure_project_loaded(&request.project).await?;
        let cache = self.ensure_symbol_index_loaded(&project).await?;
        let cache_key = format!("get_interface:{}", interface);
        if let Some(cached) = cache.query_cache.get(&cache_key) {
            return Ok(cached);
        }

        let symbol_index = cache.clone();
        let interface_key = interface.clone();
        let result = Self::blocking_storage(project.database_path(), move |storage| {
            let symbol = Self::resolve_symbol_from_storage(
                &storage,
                &symbol_index.symbol_index,
                &interface_key,
            )?;
            Ok(json!({
                "interface": symbol,
            }))
        })
        .await?;

        cache.query_cache.insert(cache_key, result.clone());
        Ok(result)
    }

    async fn get_dependencies_value(&self, request: DependenciesRequest) -> McpResult<Value> {
        let interface = request.interface.trim().to_string();
        if interface.is_empty() {
            return Err(Self::invalid_params("interface cannot be empty"));
        }

        let direction = request
            .direction
            .as_deref()
            .unwrap_or("outgoing")
            .to_ascii_lowercase();
        if !matches!(direction.as_str(), "outgoing" | "incoming" | "both") {
            return Err(Self::invalid_params(
                "direction must be one of: outgoing, incoming, both",
            ));
        }

        let max_depth = request.max_depth.unwrap_or(DEFAULT_DEPENDENCY_DEPTH);
        let project = self.ensure_project_loaded(&request.project).await?;
        let cache = self.ensure_symbol_index_loaded(&project).await?;
        let cache_key = format!("get_dependencies:{}:{}:{}", interface, direction, max_depth);
        if let Some(cached) = cache.query_cache.get(&cache_key) {
            return Ok(cached);
        }

        let symbol_index = cache.clone();
        let interface_key = interface.clone();
        let direction_key = direction.clone();
        let result = Self::blocking_storage(project.database_path(), move |storage| {
            let symbol = Self::resolve_symbol_from_storage(
                &storage,
                &symbol_index.symbol_index,
                &interface_key,
            )?;

            let response = match direction_key.as_str() {
                "incoming" => json!({
                    "interface": symbol,
                    "direction": "incoming",
                    "max_depth": max_depth,
                    "dependencies": storage
                        .get_dependency_chain_backward(&symbol.id, max_depth)
                        .map_err(Self::storage_error_message)?,
                }),
                "both" => json!({
                    "interface": symbol,
                    "direction": "both",
                    "max_depth": max_depth,
                    "outgoing": storage
                        .get_dependency_chain_forward(&symbol.id, max_depth)
                        .map_err(Self::storage_error_message)?,
                    "incoming": storage
                        .get_dependency_chain_backward(&symbol.id, max_depth)
                        .map_err(Self::storage_error_message)?,
                }),
                _ => json!({
                    "interface": symbol,
                    "direction": "outgoing",
                    "max_depth": max_depth,
                    "dependencies": storage
                        .get_dependency_chain_forward(&symbol.id, max_depth)
                        .map_err(Self::storage_error_message)?,
                }),
            };

            Ok(response)
        })
        .await?;

        cache.query_cache.insert(cache_key, result.clone());
        Ok(result)
    }

    async fn get_call_chain_value(&self, request: CallChainRequest) -> McpResult<Value> {
        let from_interface = request.from_interface.trim().to_string();
        let to_interface = request.to_interface.trim().to_string();
        if from_interface.is_empty() || to_interface.is_empty() {
            return Err(Self::invalid_params(
                "from_interface and to_interface cannot be empty",
            ));
        }

        let max_depth = request.max_depth.unwrap_or(DEFAULT_DEPENDENCY_DEPTH);
        let project = self.ensure_project_loaded(&request.project).await?;
        let cache = self.ensure_symbol_index_loaded(&project).await?;
        let cache_key = format!(
            "get_call_chain:{}:{}:{}",
            from_interface, to_interface, max_depth
        );
        if let Some(cached) = cache.query_cache.get(&cache_key) {
            return Ok(cached);
        }

        let symbol_index = cache.clone();
        let from_key = from_interface.clone();
        let to_key = to_interface.clone();
        let result = Self::blocking_storage(project.database_path(), move |storage| {
            let from_symbol =
                Self::resolve_symbol_from_storage(&storage, &symbol_index.symbol_index, &from_key)?;
            let to_symbol =
                Self::resolve_symbol_from_storage(&storage, &symbol_index.symbol_index, &to_key)?;

            Ok(json!({
                "from": from_symbol,
                "to": to_symbol,
                "max_depth": max_depth,
                "path": storage
                    .get_call_chain_path(&from_symbol.id, &to_symbol.id, max_depth)
                    .map_err(Self::storage_error_message)?,
            }))
        })
        .await?;

        cache.query_cache.insert(cache_key, result.clone());
        Ok(result)
    }

    async fn get_file_interfaces_value(&self, request: FileInterfacesRequest) -> McpResult<Value> {
        let project = self.ensure_project_loaded(&request.project).await?;
        let file_path = Self::normalize_file_path(&project.path, &request.file_path)
            .map_err(Self::invalid_params)?;
        let cache = self.project_cache(&project.id);
        let cache_key = format!("get_file_interfaces:{}", file_path);
        if let Some(cached) = cache.query_cache.get(&cache_key) {
            return Ok(cached);
        }

        let file_path_for_query = file_path.clone();
        let result = Self::blocking_storage(project.database_path(), move |storage| {
            let interfaces = storage
                .get_symbols_by_file(&file_path_for_query)
                .map_err(Self::storage_error_message)?
                .into_iter()
                .map(|symbol| Self::interface_summary_value(&symbol))
                .collect::<Vec<_>>();
            Ok(json!({
                "file_path": file_path_for_query,
                "interfaces": interfaces,
            }))
        })
        .await?;

        cache.query_cache.insert(cache_key, result.clone());
        Ok(result)
    }

    async fn get_task_context_value(&self, request: TaskContextRequest) -> McpResult<Value> {
        let mode = Self::parse_task_context_mode(request.mode.as_deref())?;
        let requested_budget = Self::parse_task_context_budget(request.budget.as_deref())?;
        let max_expansions = request.max_expansions.unwrap_or(DEFAULT_CONTEXT_EXPANSIONS);
        if max_expansions == 0 {
            return Err(Self::invalid_params(
                "max_expansions must be greater than 0",
            ));
        }

        let project = self.ensure_project_loaded(&request.project).await?;
        let cache = self.ensure_symbol_index_loaded(&project).await?;
        let cache_key = format!(
            "get_task_context:{}",
            serde_json::to_string(&request).map_err(Self::serialization_error)?
        );
        if let Some(cached) = cache.query_cache.get(&cache_key) {
            return Ok(cached);
        }

        let symbol_index = cache.clone();
        let project_root = project.path.clone();
        let request_for_query = request.clone();
        let result = Self::blocking_storage(project.database_path(), move |storage| {
            let question = request_for_query
                .question
                .as_deref()
                .map(str::trim)
                .filter(|question| !question.is_empty());
            let mut resolved = Self::resolve_task_context_inputs(
                &storage,
                &symbol_index.symbol_index,
                &project_root,
                &request_for_query,
                question,
            )?;
            let previous_scene = request_for_query
                .conversation
                .as_ref()
                .and_then(|conversation| conversation.previous_scene.as_deref());
            let (scene, mut confidence, question_signals) =
                Self::infer_task_context_scene(mode, question, &resolved, previous_scene);
            let allow_source_snippets = request_for_query.allow_source_snippets.unwrap_or(false);
            let (applied_budget, expanded) =
                Self::resolve_task_context_budget(requested_budget, scene, allow_source_snippets);
            let limits = Self::task_context_limits(applied_budget, allow_source_snippets);
            if scene == TaskContextMode::Workflow
                && Self::resolved_symbols_lack_workflow_anchor(&resolved)
            {
                if let Some(question) = question {
                    let seeded = Self::seed_workflow_symbols_from_question(
                        &storage,
                        &mut resolved,
                        question,
                    )?;
                    if seeded > 0 {
                        Self::clear_task_context_anchor_penalties(&mut resolved);
                    }
                }
            }
            let build = Self::build_task_context(
                &storage,
                &project_root,
                &request_for_query,
                &mut resolved,
                scene,
                limits,
            )?;
            confidence = (confidence + build.confidence_delta
                - (resolved.penalties.len() as f64 * 0.04))
                .clamp(0.1, 0.98);

            let resolved_symbols = resolved
                .symbols
                .iter()
                .map(|anchor| {
                    let mut summary = Self::interface_summary_value(&anchor.symbol);
                    summary["anchor_source"] = json!(anchor.source);
                    summary
                })
                .collect::<Vec<_>>();

            let mut response = json!({
                "scene": scene.as_str(),
                "confidence": confidence,
                "coverage": build.coverage,
                "status": build.status,
                "budget": {
                    "requested": requested_budget.as_str(),
                    "applied": applied_budget.as_str(),
                    "expanded": expanded,
                    "max_expansions": max_expansions,
                    "estimated_tokens": 0,
                    "truncated": false,
                },
                "evidence": {
                    "question_signals": question_signals,
                    "anchor_sources": resolved.sources,
                    "graph_signals": build.graph_signals,
                    "penalties": resolved.penalties,
                },
                "resolved_anchors": {
                    "symbols": resolved_symbols,
                    "files": resolved.files,
                },
                "package": build.package,
                "expansion_hint": build.expansion_hint,
                "next_tool_calls": build.next_tool_calls,
                "fallback_to_code": build.fallback_to_code,
                "note": build.note,
            });
            let estimated_tokens = Self::estimate_value_tokens(&response);
            response["budget"]["estimated_tokens"] = json!(estimated_tokens);
            Ok(response)
        })
        .await?;

        cache.query_cache.insert(cache_key, result.clone());
        Ok(result)
    }

    async fn list_project_interfaces_value(&self, target: ProjectTarget) -> McpResult<Value> {
        let project = self.ensure_project_loaded(&target).await?;
        let cache = self.project_cache(&project.id);
        let cache_key = "project_interfaces".to_string();
        if let Some(cached) = cache.query_cache.get(&cache_key) {
            return Ok(cached);
        }

        let result = Self::blocking_storage(project.database_path(), move |storage| {
            let interfaces = Self::all_symbols(&storage)?
                .into_iter()
                .map(|symbol| Self::interface_summary_value(&symbol))
                .collect::<Vec<_>>();
            Ok(json!({
                "count": interfaces.len(),
                "interfaces": interfaces,
            }))
        })
        .await?;

        cache.query_cache.insert(cache_key, result.clone());
        Ok(result)
    }

    async fn batch_query_value(&self, request: BatchQueryRequest) -> McpResult<Value> {
        if request.queries.is_empty() {
            return Err(Self::invalid_params("queries cannot be empty"));
        }

        let mut results = Vec::with_capacity(request.queries.len());
        for (index, item) in request.queries.iter().enumerate() {
            let kind = item.kind.to_ascii_lowercase();
            let response = match kind.as_str() {
                "find_interfaces" if !self.is_tool_enabled("find_interfaces") => {
                    Err(Self::disabled_tool_error("find_interfaces"))
                }
                "find_interfaces" => match item.query.clone() {
                    Some(query) => {
                        self.find_interfaces_value(FindInterfacesRequest {
                            project: request.project.clone(),
                            query,
                            limit: item.limit,
                        })
                        .await
                    }
                    None => Err(Self::invalid_params("find_interfaces requires query")),
                },
                "get_interface" if !self.is_tool_enabled("get_interface") => {
                    Err(Self::disabled_tool_error("get_interface"))
                }
                "get_interface" => match item.interface.clone() {
                    Some(interface) => {
                        self.get_interface_value(InterfaceLookupRequest {
                            project: request.project.clone(),
                            interface,
                        })
                        .await
                    }
                    None => Err(Self::invalid_params("get_interface requires interface")),
                },
                "get_dependencies" if !self.is_tool_enabled("get_dependencies") => {
                    Err(Self::disabled_tool_error("get_dependencies"))
                }
                "get_dependencies" => match item.interface.clone() {
                    Some(interface) => {
                        self.get_dependencies_value(DependenciesRequest {
                            project: request.project.clone(),
                            interface,
                            direction: item.direction.clone(),
                            max_depth: item.max_depth,
                        })
                        .await
                    }
                    None => Err(Self::invalid_params("get_dependencies requires interface")),
                },
                "get_call_chain" if !self.is_tool_enabled("get_call_chain") => {
                    Err(Self::disabled_tool_error("get_call_chain"))
                }
                "get_call_chain" => {
                    match (item.from_interface.clone(), item.to_interface.clone()) {
                        (Some(from_interface), Some(to_interface)) => {
                            self.get_call_chain_value(CallChainRequest {
                                project: request.project.clone(),
                                from_interface,
                                to_interface,
                                max_depth: item.max_depth,
                            })
                            .await
                        }
                        (None, _) => Err(Self::invalid_params(
                            "get_call_chain requires from_interface",
                        )),
                        (_, None) => {
                            Err(Self::invalid_params("get_call_chain requires to_interface"))
                        }
                    }
                }
                "get_file_interfaces" if !self.is_tool_enabled("get_file_interfaces") => {
                    Err(Self::disabled_tool_error("get_file_interfaces"))
                }
                "get_file_interfaces" => match item.file_path.clone() {
                    Some(file_path) => {
                        self.get_file_interfaces_value(FileInterfacesRequest {
                            project: request.project.clone(),
                            file_path,
                        })
                        .await
                    }
                    None => Err(Self::invalid_params(
                        "get_file_interfaces requires file_path",
                    )),
                },
                _ => Err(Self::invalid_params(format!(
                    "unsupported batch query kind: {}",
                    item.kind
                ))),
            };

            match response {
                Ok(value) => results.push(json!({
                    "index": index,
                    "kind": item.kind,
                    "ok": true,
                    "result": value,
                })),
                Err(error) => results.push(json!({
                    "index": index,
                    "kind": item.kind,
                    "ok": false,
                    "error": error.message,
                })),
            }
        }

        Ok(json!({ "results": results }))
    }

    async fn rebuild_database_value(&self, request: ProjectStatusRequest) -> McpResult<Value> {
        let project_id = self.resolve_project_id(&request.project, true).await?;
        let project_root = self.project_root_for_id(&project_id).await?;
        self.clear_project_cache(&project_id);

        let database_paths = vec![
            project_root.join(crate::CACHE_DIR).join(crate::DB_FILE),
            project_root
                .join(crate::CACHE_DIR)
                .join(format!("{}-wal", crate::DB_FILE)),
            project_root
                .join(crate::CACHE_DIR)
                .join(format!("{}-shm", crate::DB_FILE)),
        ];

        Self::spawn_blocking(move || {
            for path in database_paths {
                if path.exists() {
                    std::fs::remove_file(&path).map_err(|error| {
                        format!("failed to remove {}: {}", path.display(), error)
                    })?;
                }
            }
            Ok(())
        })
        .await?;

        self.manager
            .scan(&project_id, true)
            .await
            .map_err(Self::manager_error)?;
        let project = self
            .manager
            .get(&project_id)
            .await
            .map_err(Self::manager_error)?
            .ok_or_else(|| Self::invalid_params(format!("unknown project id: {}", project_id)))?;

        if let ProjectState::Failed { error, .. } = &project.state {
            return Err(Self::internal_error(error.clone()));
        }

        self.refresh_project_cache(&project).await?;
        self.ensure_project_watcher(&project).await?;
        let stats = Self::blocking_storage(project.database_path(), |storage| {
            let stats = storage.get_stats().map_err(Self::storage_error_message)?;
            serde_json::to_value(stats).map_err(|error| error.to_string())
        })
        .await?;

        Ok(json!({
            "project": self.project_record(&project_id).await?,
            "rebuild": true,
            "stats": stats,
        }))
    }

    fn interface_summary_value(symbol: &Symbol) -> Value {
        json!({
            "id": symbol.id,
            "name": symbol.name,
            "qualified_name": symbol.qualified_name,
            "kind": symbol.kind,
            "file_path": symbol.file_path,
            "line": symbol.line,
            "column": symbol.column,
            "visibility": symbol.visibility,
            "source": symbol.source,
        })
    }

    fn build_project_overview(
        storage: &Storage,
        max_symbols: usize,
        max_edges: usize,
    ) -> Result<Value, String> {
        let conn = storage.connection();
        let total_symbols: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM symbols WHERE source = 'local'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let total_edges: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM (
                    SELECT d.from_symbol, d.to_symbol
                    FROM dependencies d
                    JOIN symbols source_symbol ON source_symbol.id = d.from_symbol
                    JOIN symbols target_symbol ON target_symbol.id = d.to_symbol
                    WHERE source_symbol.source = 'local'
                      AND target_symbol.source = 'local'
                    GROUP BY d.from_symbol, d.to_symbol
                )",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;

        let mut symbol_stmt = conn
            .prepare(
                "SELECT
                    s.id,
                    s.name,
                    s.qualified_name,
                    s.kind,
                    s.file_path,
                    s.line,
                    s.column,
                    s.visibility,
                    s.source,
                    COALESCE(incoming.count, 0) AS incoming_count,
                    COALESCE(outgoing.count, 0) AS outgoing_count,
                    COALESCE(incoming.count, 0) + COALESCE(outgoing.count, 0) AS degree
                FROM symbols s
                LEFT JOIN (
                    SELECT to_symbol AS symbol_id, COUNT(*) AS count
                    FROM dependencies
                    GROUP BY to_symbol
                ) incoming ON incoming.symbol_id = s.id
                LEFT JOIN (
                    SELECT from_symbol AS symbol_id, COUNT(*) AS count
                    FROM dependencies
                    GROUP BY from_symbol
                ) outgoing ON outgoing.symbol_id = s.id
                WHERE s.source = 'local'
                ORDER BY degree DESC, s.qualified_name ASC
                LIMIT ?1",
            )
            .map_err(|error| error.to_string())?;

        let nodes = symbol_stmt
            .query_map([max_symbols as i64], |row| {
                let id: String = row.get(0)?;
                Ok(json!({
                    "id": id,
                    "name": row.get::<_, String>(1)?,
                    "qualified_name": row.get::<_, String>(2)?,
                    "kind": row.get::<_, String>(3)?,
                    "file_path": row.get::<_, String>(4)?,
                    "line": row.get::<_, u32>(5)?,
                    "column": row.get::<_, u32>(6)?,
                    "visibility": row.get::<_, String>(7)?,
                    "source": row.get::<_, String>(8)?,
                    "incoming_count": row.get::<_, i64>(9)?,
                    "outgoing_count": row.get::<_, i64>(10)?,
                    "degree": row.get::<_, i64>(11)?,
                }))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;

        let selected_ids = nodes
            .iter()
            .filter_map(|node| node["id"].as_str().map(ToOwned::to_owned))
            .collect::<HashSet<_>>();

        let mut edges = Vec::new();
        if !selected_ids.is_empty() && max_edges > 0 {
            let mut edge_stmt = conn
                .prepare(
                    "SELECT
                        d.from_symbol,
                        d.to_symbol,
                        COUNT(*) AS weight,
                        GROUP_CONCAT(DISTINCT d.kind) AS kinds
                    FROM dependencies d
                    JOIN symbols source_symbol ON source_symbol.id = d.from_symbol
                    JOIN symbols target_symbol ON target_symbol.id = d.to_symbol
                    WHERE source_symbol.source = 'local'
                      AND target_symbol.source = 'local'
                    GROUP BY d.from_symbol, d.to_symbol
                    ORDER BY weight DESC, d.from_symbol ASC, d.to_symbol ASC",
                )
                .map_err(|error| error.to_string())?;

            let edge_rows = edge_stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                })
                .map_err(|error| error.to_string())?;

            for edge in edge_rows {
                let (source, target, weight, kinds) = edge.map_err(|error| error.to_string())?;
                if !selected_ids.contains(&source) || !selected_ids.contains(&target) {
                    continue;
                }

                edges.push(json!({
                    "id": format!("{source}->{target}"),
                    "source": source,
                    "target": target,
                    "weight": weight,
                    "kinds": kinds
                        .unwrap_or_default()
                        .split(',')
                        .filter(|kind| !kind.is_empty())
                        .collect::<Vec<_>>(),
                }));

                if edges.len() >= max_edges {
                    break;
                }
            }
        }

        let displayed_symbols = nodes.len();
        let hidden_symbols = total_symbols.saturating_sub(displayed_symbols as i64);

        Ok(json!({
            "total_symbols": total_symbols,
            "total_edges": total_edges,
            "displayed_symbols": displayed_symbols,
            "displayed_edges": edges.len(),
            "hidden_symbols": hidden_symbols,
            "max_symbols": max_symbols,
            "max_edges": max_edges,
            "nodes": nodes,
            "edges": edges,
        }))
    }

    fn parse_task_context_mode(mode: Option<&str>) -> McpResult<TaskContextMode> {
        match mode.unwrap_or("auto").trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Ok(TaskContextMode::Auto),
            "locate" => Ok(TaskContextMode::Locate),
            "behavior" => Ok(TaskContextMode::Behavior),
            "impact" => Ok(TaskContextMode::Impact),
            "workflow" => Ok(TaskContextMode::Workflow),
            "call_chain" => Ok(TaskContextMode::CallChain),
            "watcher" => Ok(TaskContextMode::Watcher),
            _ => Err(Self::invalid_params(
                "mode must be one of: auto, locate, behavior, impact, workflow, call_chain, watcher",
            )),
        }
    }

    fn parse_task_context_budget(budget: Option<&str>) -> McpResult<TaskContextBudget> {
        match budget
            .unwrap_or("lean")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "" | "lean" => Ok(TaskContextBudget::Lean),
            "normal" => Ok(TaskContextBudget::Normal),
            "wide" => Ok(TaskContextBudget::Wide),
            _ => Err(Self::invalid_params(
                "budget must be one of: lean, normal, wide",
            )),
        }
    }

    fn resolve_task_context_budget(
        requested: TaskContextBudget,
        scene: TaskContextMode,
        allow_source_snippets: bool,
    ) -> (TaskContextBudget, bool) {
        if requested == TaskContextBudget::Lean && scene == TaskContextMode::Workflow {
            return (TaskContextBudget::Normal, true);
        }

        if requested == TaskContextBudget::Lean
            && allow_source_snippets
            && scene == TaskContextMode::Behavior
        {
            return (TaskContextBudget::Normal, true);
        }

        (requested, false)
    }

    fn task_context_limits(
        budget: TaskContextBudget,
        allow_source_snippets: bool,
    ) -> TaskContextLimits {
        match budget {
            TaskContextBudget::Lean => TaskContextLimits {
                primary_symbols: 3,
                primary_files: 3,
                related_files: 3,
                direct_neighbors: 3,
                same_file_symbols: 2,
                source_snippets: 0,
                workflow_symbols: 5,
                workflow_depth: 2,
            },
            TaskContextBudget::Normal => TaskContextLimits {
                primary_symbols: DEFAULT_CONTEXT_SYMBOL_LIMIT,
                primary_files: DEFAULT_CONTEXT_FILE_LIMIT,
                related_files: DEFAULT_CONTEXT_FILE_LIMIT,
                direct_neighbors: DEFAULT_CONTEXT_SYMBOL_LIMIT,
                same_file_symbols: 3,
                source_snippets: if allow_source_snippets { 2 } else { 0 },
                workflow_symbols: 7,
                workflow_depth: 3,
            },
            TaskContextBudget::Wide => TaskContextLimits {
                primary_symbols: 8,
                primary_files: 8,
                related_files: 8,
                direct_neighbors: 8,
                same_file_symbols: 4,
                source_snippets: if allow_source_snippets { 4 } else { 0 },
                workflow_symbols: 10,
                workflow_depth: 4,
            },
        }
    }

    fn resolve_task_context_inputs(
        storage: &Storage,
        symbol_index: &SymbolIndexCache,
        project_root: &Path,
        request: &TaskContextRequest,
        question: Option<&str>,
    ) -> Result<ResolvedTaskContext, String> {
        let mut resolved = ResolvedTaskContext::default();

        for anchor in &request.anchor_symbols {
            Self::add_task_context_symbol_anchor(
                storage,
                symbol_index,
                &mut resolved,
                anchor,
                "anchor_symbols",
            );
        }

        for file in &request.anchor_files {
            Self::add_task_context_file_anchor(&mut resolved, project_root, file, "anchor_files");
        }

        if let Some(workspace) = &request.workspace {
            let mut resolved_workspace_selection = false;
            if let (Some(selection_symbol), Some(active_file)) =
                (&workspace.selection_symbol, &workspace.active_file)
            {
                if let Ok(Some(symbol)) = Self::resolve_named_symbol_from_active_file(
                    storage,
                    project_root,
                    active_file,
                    selection_symbol,
                    workspace.selection_line,
                ) {
                    Self::push_task_context_symbol(
                        &mut resolved,
                        symbol,
                        "workspace.selection_symbol",
                    );
                    resolved_workspace_selection = true;
                }
            }

            if let Some(selection_symbol) = &workspace.selection_symbol {
                if !resolved_workspace_selection {
                    Self::add_task_context_symbol_anchor(
                        storage,
                        symbol_index,
                        &mut resolved,
                        selection_symbol,
                        "workspace.selection_symbol",
                    );
                }
            }
            if let Some(active_file) = &workspace.active_file {
                Self::add_task_context_file_anchor(
                    &mut resolved,
                    project_root,
                    active_file,
                    "workspace.active_file",
                );
            }
            for recent_file in &workspace.recent_files {
                Self::add_task_context_file_anchor(
                    &mut resolved,
                    project_root,
                    recent_file,
                    "workspace.recent_files",
                );
            }
        }

        if let Some(runtime) = &request.runtime {
            for stacktrace_symbol in &runtime.stacktrace_symbols {
                Self::add_task_context_symbol_anchor(
                    storage,
                    symbol_index,
                    &mut resolved,
                    stacktrace_symbol,
                    "runtime.stacktrace_symbols",
                );
            }
        }

        if let Some(conversation) = &request.conversation {
            for previous_target in &conversation.previous_targets {
                Self::add_task_context_symbol_anchor(
                    storage,
                    symbol_index,
                    &mut resolved,
                    previous_target,
                    "conversation.previous_targets",
                );
            }
        }

        if resolved.symbols.is_empty() {
            if let Some(workspace) = &request.workspace {
                if let Some(active_file) = &workspace.active_file {
                    let selection_symbol = Self::resolve_symbol_from_active_file(
                        storage,
                        project_root,
                        active_file,
                        workspace.selection_line,
                    )?;
                    if let Some(symbol) = selection_symbol {
                        Self::push_task_context_symbol(
                            &mut resolved,
                            symbol,
                            "workspace.active_file",
                        );
                    }
                }
            }
        }

        if resolved.symbols.is_empty() {
            if let Some(question) = question {
                Self::resolve_task_context_question_anchor(
                    storage,
                    symbol_index,
                    &mut resolved,
                    question,
                )?;
                if resolved.symbols.is_empty() {
                    resolved.penalties.push(
                        "question text did not provide a usable anchor; add a symbol or file anchor"
                            .to_string(),
                    );
                }
            }
        }

        Ok(resolved)
    }

    fn add_task_context_symbol_anchor(
        storage: &Storage,
        symbol_index: &SymbolIndexCache,
        resolved: &mut ResolvedTaskContext,
        identifier: &str,
        source: &'static str,
    ) {
        let identifier = identifier.trim();
        if identifier.is_empty() {
            return;
        }

        match Self::resolve_symbol_from_storage(storage, symbol_index, identifier) {
            Ok(symbol) => Self::push_task_context_symbol(resolved, symbol, source),
            Err(error) => resolved.penalties.push(format!(
                "failed to resolve {source} '{identifier}': {error}"
            )),
        }
    }

    fn add_task_context_file_anchor(
        resolved: &mut ResolvedTaskContext,
        project_root: &Path,
        file_path: &str,
        source: &'static str,
    ) {
        let file_path = file_path.trim();
        if file_path.is_empty() {
            return;
        }

        match Self::normalize_file_path(project_root, file_path) {
            Ok(file_path) => {
                if !resolved.files.iter().any(|existing| existing == &file_path) {
                    resolved.files.push(file_path);
                }
                Self::push_unique_string(&mut resolved.sources, source.to_string());
            }
            Err(error) => resolved
                .penalties
                .push(format!("failed to resolve {source} '{file_path}': {error}")),
        }
    }

    fn push_task_context_symbol(
        resolved: &mut ResolvedTaskContext,
        symbol: Symbol,
        source: &'static str,
    ) {
        if resolved
            .symbols
            .iter()
            .any(|existing| existing.symbol.id == symbol.id)
        {
            Self::push_unique_string(&mut resolved.sources, source.to_string());
            return;
        }

        resolved
            .symbols
            .push(ResolvedTaskContextAnchor { symbol, source });
        Self::push_unique_string(&mut resolved.sources, source.to_string());
    }

    fn resolve_symbol_from_active_file(
        storage: &Storage,
        project_root: &Path,
        active_file: &str,
        selection_line: Option<u32>,
    ) -> Result<Option<Symbol>, String> {
        let file_path = Self::normalize_file_path(project_root, active_file)?;
        let mut symbols = storage
            .get_symbols_by_file(&file_path)
            .map_err(Self::storage_error_message)?;
        if symbols.is_empty() {
            return Ok(None);
        }

        if let Some(selection_line) = selection_line {
            symbols.sort_by_key(|symbol| (symbol.line.abs_diff(selection_line), symbol.line));
            return Ok(symbols.into_iter().next());
        }

        if symbols.len() == 1 {
            return Ok(symbols.into_iter().next());
        }

        Ok(None)
    }

    fn resolve_named_symbol_from_active_file(
        storage: &Storage,
        project_root: &Path,
        active_file: &str,
        selection_symbol: &str,
        selection_line: Option<u32>,
    ) -> Result<Option<Symbol>, String> {
        let file_path = Self::normalize_file_path(project_root, active_file)?;
        let mut symbols = storage
            .get_symbols_by_file(&file_path)
            .map_err(Self::storage_error_message)?
            .into_iter()
            .filter(|symbol| symbol.name == selection_symbol)
            .collect::<Vec<_>>();

        if symbols.is_empty() {
            return Ok(None);
        }

        if let Some(selection_line) = selection_line {
            symbols.sort_by_key(|symbol| (symbol.line.abs_diff(selection_line), symbol.line));
            return Ok(symbols.into_iter().next());
        }

        if symbols.len() == 1 {
            return Ok(symbols.into_iter().next());
        }

        Ok(None)
    }

    fn resolve_task_context_question_anchor(
        storage: &Storage,
        symbol_index: &SymbolIndexCache,
        resolved: &mut ResolvedTaskContext,
        question: &str,
    ) -> Result<(), String> {
        let candidates = Self::extract_identifier_candidates(question);
        if candidates.is_empty() {
            return Ok(());
        }

        let mut matches = Vec::new();
        for candidate in candidates.iter().take(5) {
            if let Ok(symbol) = Self::resolve_symbol_from_storage(storage, symbol_index, candidate)
            {
                if matches
                    .iter()
                    .all(|existing: &Symbol| existing.id != symbol.id)
                {
                    matches.push(symbol);
                }
            }
        }

        match matches.len() {
            0 => resolved
                .penalties
                .push("question text did not resolve to a unique symbol".to_string()),
            1 => Self::push_task_context_symbol(resolved, matches.remove(0), "question.identifier"),
            _ => resolved.penalties.push(
                "question text resolved to multiple candidate symbols; add an explicit anchor"
                    .to_string(),
            ),
        }

        Ok(())
    }

    fn extract_identifier_candidates(question: &str) -> Vec<String> {
        let mut candidates = Vec::new();
        let mut current = String::new();

        for ch in question.chars() {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':' | '/' | '.') {
                current.push(ch);
            } else if !current.is_empty() {
                Self::push_identifier_candidate(&mut candidates, &current);
                current.clear();
            }
        }

        if !current.is_empty() {
            Self::push_identifier_candidate(&mut candidates, &current);
        }

        candidates.truncate(8);
        candidates
    }

    fn push_identifier_candidate(candidates: &mut Vec<String>, candidate: &str) {
        let candidate = candidate.trim_matches('.');
        if candidate.len() < 3
            || candidate.chars().all(|ch| ch.is_ascii_digit())
            || candidates.iter().any(|existing| existing == candidate)
        {
            return;
        }

        candidates.push(candidate.to_string());
    }

    fn infer_task_context_scene(
        requested_mode: TaskContextMode,
        question: Option<&str>,
        resolved: &ResolvedTaskContext,
        previous_scene: Option<&str>,
    ) -> (TaskContextMode, f64, Vec<String>) {
        if requested_mode != TaskContextMode::Auto {
            return (
                requested_mode,
                0.92,
                vec!["mode overridden by caller".to_string()],
            );
        }

        let mut scores = HashMap::from([
            (TaskContextMode::Locate.as_str(), 1_i32),
            (TaskContextMode::Behavior.as_str(), 0_i32),
            (TaskContextMode::Impact.as_str(), 0_i32),
            (TaskContextMode::Workflow.as_str(), 0_i32),
            (TaskContextMode::CallChain.as_str(), 0_i32),
            (TaskContextMode::Watcher.as_str(), 0_i32),
        ]);
        let mut signals = Vec::new();
        let normalized_question = question.map(|question| question.to_ascii_lowercase());

        if let Some(question) = &normalized_question {
            if Self::question_contains(question, &["why", "failure", "error", "panic", "race"])
                || Self::question_contains(
                    question,
                    &["为什么", "失败", "报错", "panic", "时序", "竞态", "升级"],
                )
            {
                *scores.get_mut(TaskContextMode::Behavior.as_str()).unwrap() += 3;
                signals.push("question looks like behavior analysis".to_string());
            }
            let (workflow_score, workflow_signals) = Self::workflow_question_score(question);
            if workflow_score > 0 {
                *scores.get_mut(TaskContextMode::Workflow.as_str()).unwrap() += workflow_score;
                signals.extend(workflow_signals);
            }
            if Self::question_contains(
                question,
                &["impact", "refactor", "risk", "rename", "change", "modify"],
            ) || Self::question_contains(question, &["影响", "重构", "风险", "改", "修改"])
            {
                *scores.get_mut(TaskContextMode::Impact.as_str()).unwrap() += 3;
                signals.push("question looks like impact analysis".to_string());
            }
            if Self::question_contains(
                question,
                &["call chain", "path", "delegate", "through", "from", "to"],
            ) || Self::question_contains(question, &["调用链", "路径", "链路", "从", "到"])
            {
                *scores.get_mut(TaskContextMode::CallChain.as_str()).unwrap() += 3;
                signals.push("question mentions an explicit chain".to_string());
            }
            if Self::question_contains(
                question,
                &["watcher", "refresh", "updated", "reindex", "rescan"],
            ) || Self::question_contains(question, &["watcher", "刷新", "更新", "重扫", "索引"])
            {
                *scores.get_mut(TaskContextMode::Watcher.as_str()).unwrap() += 3;
                signals.push("question looks like watcher/index validation".to_string());
            }
            if Self::question_contains(
                question,
                &["where", "who calls", "defined", "which file", "which files"],
            ) || Self::question_contains(question, &["哪里", "谁调用", "在哪", "哪些文件"])
            {
                *scores.get_mut(TaskContextMode::Locate.as_str()).unwrap() += 2;
                signals.push("question looks like code location".to_string());
            }
        }

        if resolved.symbols.len() >= 2 {
            *scores.get_mut(TaskContextMode::CallChain.as_str()).unwrap() += 3;
            signals.push("multiple symbol anchors supplied".to_string());
        } else if !resolved.symbols.is_empty() {
            *scores.get_mut(TaskContextMode::Locate.as_str()).unwrap() += 1;
            *scores.get_mut(TaskContextMode::Impact.as_str()).unwrap() += 1;
        }

        if !resolved.files.is_empty() && resolved.symbols.is_empty() {
            *scores.get_mut(TaskContextMode::Locate.as_str()).unwrap() += 2;
            signals.push("file anchors supplied without symbol anchors".to_string());
        }

        if let Some(previous_scene) = previous_scene {
            if let Ok(previous_scene) = Self::parse_task_context_mode(Some(previous_scene)) {
                if previous_scene != TaskContextMode::Auto {
                    *scores.get_mut(previous_scene.as_str()).unwrap() += 1;
                    signals.push("conversation supplied a previous scene".to_string());
                }
            }
        }

        let mut ranked = [
            (
                TaskContextMode::Locate,
                *scores.get(TaskContextMode::Locate.as_str()).unwrap(),
            ),
            (
                TaskContextMode::Behavior,
                *scores.get(TaskContextMode::Behavior.as_str()).unwrap(),
            ),
            (
                TaskContextMode::Impact,
                *scores.get(TaskContextMode::Impact.as_str()).unwrap(),
            ),
            (
                TaskContextMode::Workflow,
                *scores.get(TaskContextMode::Workflow.as_str()).unwrap(),
            ),
            (
                TaskContextMode::CallChain,
                *scores.get(TaskContextMode::CallChain.as_str()).unwrap(),
            ),
            (
                TaskContextMode::Watcher,
                *scores.get(TaskContextMode::Watcher.as_str()).unwrap(),
            ),
        ];
        ranked.sort_by_key(|entry| std::cmp::Reverse(entry.1));

        let scene = ranked[0].0;
        let top_score = ranked[0].1;
        let second_score = ranked.get(1).map(|(_, score)| *score).unwrap_or_default();
        let confidence = (0.56
            + f64::from((top_score - second_score).max(0)) * 0.06
            + if !resolved.symbols.is_empty() {
                0.08
            } else {
                0.0
            }
            + if !resolved.files.is_empty() {
                0.04
            } else {
                0.0
            })
        .clamp(0.4, 0.9);

        (scene, confidence, signals)
    }

    fn question_contains(question: &str, terms: &[&str]) -> bool {
        terms
            .iter()
            .any(|term| question.contains(&term.to_ascii_lowercase()))
    }

    fn workflow_phase_specs() -> &'static [WorkflowPhaseSpec] {
        &WORKFLOW_PHASES
    }

    fn contains_any(text: &str, terms: &[&str]) -> bool {
        terms.iter().any(|term| text.contains(term))
    }

    fn workflow_phase_priority_terms(phase: WorkflowPhaseSpec) -> &'static [&'static str] {
        match phase.key {
            "approval" => WORKFLOW_APPROVAL_PRIORITY_TERMS,
            "resume" => WORKFLOW_RESUME_PRIORITY_TERMS,
            "dispatch" => WORKFLOW_DISPATCH_PRIORITY_TERMS,
            "queue" => WORKFLOW_QUEUE_PRIORITY_TERMS,
            "scheduler" => WORKFLOW_SCHEDULER_PRIORITY_TERMS,
            _ => &[],
        }
    }

    fn workflow_phase_support_limit(phase: WorkflowPhaseSpec) -> usize {
        match phase.key {
            "approval" | "dispatch" | "queue" | "scheduler" => 1,
            "resume" => 0,
            _ => 0,
        }
    }

    fn workflow_primary_symbol_limit(limits: TaskContextLimits, phase_count: usize) -> usize {
        limits
            .primary_symbols
            .max(phase_count)
            .saturating_add(3)
            .min(8)
    }

    fn workflow_question_score(question: &str) -> (i32, Vec<String>) {
        let mut score = 0;
        let mut signals = Vec::new();
        let has_status = Self::question_contains(question, WORKFLOW_STATUS_TERMS);
        let has_transition = Self::question_contains(question, WORKFLOW_TRANSITION_TERMS);
        let has_scheduling = Self::question_contains(question, WORKFLOW_SCHEDULING_TERMS);

        if has_status && has_transition {
            score += 4;
            signals.push("question mentions a status transition workflow".to_string());
        }
        if has_scheduling && has_transition {
            score += 3;
            signals.push("question mentions a scheduling workflow".to_string());
        }
        if Self::question_contains(question, &["approval", "审批"])
            && Self::question_contains(question, &["queue", "queued", "排队", "队列"])
        {
            score += 2;
            signals.push("question connects approval with queueing".to_string());
        }

        (score, signals)
    }

    fn resolved_symbols_lack_workflow_anchor(resolved: &ResolvedTaskContext) -> bool {
        !resolved
            .symbols
            .iter()
            .any(|anchor| Self::is_actionable_workflow_symbol(&anchor.symbol))
    }

    fn is_actionable_workflow_symbol(symbol: &Symbol) -> bool {
        matches!(
            symbol.kind,
            crate::core::SymbolKind::Function | crate::core::SymbolKind::Method
        ) && symbol.source == crate::core::SymbolSource::Local
            && !Self::is_probably_test_file(&symbol.file_path)
    }

    fn is_probably_test_file(file_path: &str) -> bool {
        file_path.contains("/tests/")
            || file_path.contains("test/")
            || file_path.contains("_test.")
            || file_path.ends_with(".spec.ts")
            || file_path.ends_with(".test.ts")
    }

    fn clear_task_context_anchor_penalties(resolved: &mut ResolvedTaskContext) {
        resolved.penalties.retain(|penalty| {
            !penalty.contains("question text did not provide a usable anchor")
                && !penalty.contains("question text did not resolve to a unique symbol")
                && !penalty.contains("question text resolved to multiple candidate symbols")
        });
    }

    fn seed_workflow_symbols_from_question(
        storage: &Storage,
        resolved: &mut ResolvedTaskContext,
        question: &str,
    ) -> Result<usize, String> {
        let normalized_question = question.to_ascii_lowercase();
        let active_phases = Self::workflow_phase_specs()
            .iter()
            .copied()
            .filter(|phase| Self::question_contains(&normalized_question, phase.question_terms))
            .collect::<Vec<_>>();
        if active_phases.is_empty() {
            return Ok(0);
        }

        let mut inserted = 0;
        let mut seen = resolved
            .symbols
            .iter()
            .map(|anchor| anchor.symbol.id.clone())
            .collect::<HashSet<_>>();

        for phase in active_phases {
            let mut phase_candidates = Vec::new();
            let mut phase_seen = HashSet::new();
            for search_term in phase.search_terms {
                for symbol in storage
                    .search_symbols(search_term, 8)
                    .map_err(Self::storage_error_message)?
                {
                    if !Self::is_actionable_workflow_symbol(&symbol)
                        || !phase_seen.insert(symbol.id.clone())
                    {
                        continue;
                    }

                    let score = Self::workflow_phase_score(&symbol, phase);
                    if score > 0 {
                        phase_candidates.push((score, symbol));
                    }
                }
            }

            phase_candidates.sort_by(|left, right| {
                right
                    .0
                    .cmp(&left.0)
                    .then_with(|| left.1.file_path.cmp(&right.1.file_path))
                    .then_with(|| left.1.line.cmp(&right.1.line))
                    .then_with(|| left.1.qualified_name.cmp(&right.1.qualified_name))
            });

            if let Some((_, symbol)) = phase_candidates.into_iter().next() {
                if seen.insert(symbol.id.clone()) {
                    Self::push_task_context_symbol(resolved, symbol, "question.workflow_seed");
                    inserted += 1;
                }
            }
        }

        Ok(inserted)
    }

    fn workflow_phase_score(symbol: &Symbol, phase: WorkflowPhaseSpec) -> i32 {
        let name = symbol.name.to_ascii_lowercase();
        let qualified_name = symbol.qualified_name.to_ascii_lowercase();
        let file_path = symbol.file_path.to_ascii_lowercase();
        let mut score = 0;
        let mut matched_phase_term = false;

        for term in phase.search_terms {
            let term = term.to_ascii_lowercase();
            if name == term {
                score += 8;
                matched_phase_term = true;
            }
            if name.contains(&term) {
                score += 6;
                matched_phase_term = true;
            }
            if qualified_name.contains(&term) {
                score += 3;
                matched_phase_term = true;
            }
            if file_path.contains(&term) {
                score += 1;
            }
        }

        for (index, term) in Self::workflow_phase_priority_terms(phase)
            .iter()
            .enumerate()
        {
            let bonus = match index {
                0 => 12,
                1 => 9,
                _ => 6,
            };
            if name == *term {
                score += bonus;
                matched_phase_term = true;
            } else if qualified_name.ends_with(&format!("::{term}")) {
                score += bonus - 2;
                matched_phase_term = true;
            } else if name.contains(term) {
                score += bonus - 4;
                matched_phase_term = true;
            }
        }

        if matches!(
            symbol.kind,
            crate::core::SymbolKind::Function | crate::core::SymbolKind::Method
        ) {
            score += 3;
        } else {
            score -= 5;
        }

        if matches!(
            symbol.visibility,
            crate::core::Visibility::Public | crate::core::Visibility::Protected
        ) {
            score += 1;
        }

        if symbol.source != crate::core::SymbolSource::Local {
            score -= 6;
        }

        if Self::is_probably_test_file(&symbol.file_path) {
            score -= 6;
        }

        if Self::contains_any(&file_path, WORKFLOW_BOUNDARY_FILE_TERMS)
            || qualified_name.contains("router")
        {
            score -= 6;
        }

        if Self::contains_any(&file_path, WORKFLOW_INTERNAL_FILE_TERMS) {
            score += 3;
        }

        if phase.key == "approval" && (name.contains("approve") || name.contains("approval")) {
            score += 4;
            matched_phase_term = true;
        }
        if phase.key == "approval" && file_path.contains("store") {
            score += 3;
        }
        if phase.key == "approval" && file_path.contains("runtime") {
            score += 2;
        }
        if phase.key == "approval" && name.contains("deny") {
            score -= 3;
        }
        if phase.key == "resume" && name.contains("resume") {
            score += 4;
            matched_phase_term = true;
        }
        if phase.key == "resume" && name.contains("approved_execution") {
            score += 4;
            matched_phase_term = true;
        }
        if phase.key == "dispatch" && name.contains("dispatch_execution") {
            score += 8;
            matched_phase_term = true;
        }
        if phase.key == "dispatch" && name == "dispatch_execution" {
            score += 4;
        }
        if phase.key == "dispatch" && name.contains("prepare_execution_dispatch") {
            score += 5;
            matched_phase_term = true;
        }
        if phase.key == "dispatch" && name == "prepare_execution_dispatch" {
            score -= 2;
        }
        if phase.key == "dispatch" && name.starts_with("prepare_") {
            score -= 1;
        }
        if phase.key == "queue" && name.contains("next_conflict_queue_head") {
            score += 8;
            matched_phase_term = true;
        }
        if phase.key == "queue" && name.contains("queue_for_conflict") {
            score += 6;
            matched_phase_term = true;
        }
        if phase.key == "queue"
            && (name.contains("queue") || name.contains("queued") || name.contains("head"))
        {
            score += 4;
            matched_phase_term = true;
        }
        if phase.key == "scheduler"
            && (name.contains("admit")
                || name.contains("dispatchable")
                || file_path.contains("scheduler"))
        {
            score += 4;
            if name.contains("admit") || name.contains("dispatchable") {
                matched_phase_term = true;
            }
        }
        if phase.key == "scheduler" && name.contains("admit") {
            score += 5;
            matched_phase_term = true;
        }
        if phase.key == "scheduler" && name.contains("dispatchable_head") {
            score += 4;
            matched_phase_term = true;
        }
        if phase.key == "dispatch"
            && (name.contains("dispatchable")
                || name.contains("admit")
                || file_path.contains("scheduler"))
        {
            score -= 3;
        }

        if !matched_phase_term {
            score -= 8;
        }

        score
    }

    fn workflow_phase_support_score(
        candidate: &WorkflowCandidate,
        selection: &WorkflowPhaseSelection,
    ) -> i32 {
        let name = candidate.symbol.name.to_ascii_lowercase();
        let mut score =
            Self::workflow_phase_score(&candidate.symbol, selection.phase) - candidate.depth as i32;

        if candidate.symbol.file_path == selection.symbol.file_path {
            score += 5;
        }

        match selection.phase.key {
            "approval" if name.contains("approval_resolve") => {
                score += 4;
            }
            "dispatch" if name.contains("prepare_execution_dispatch") => {
                score += 4;
            }
            "queue" if name.contains("queue_for_conflict") => {
                score += 4;
            }
            "scheduler" if name.contains("dispatchable_head") => {
                score += 4;
            }
            _ => {}
        }

        score
    }

    fn build_task_context(
        storage: &Storage,
        project_root: &Path,
        request: &TaskContextRequest,
        resolved: &mut ResolvedTaskContext,
        scene: TaskContextMode,
        limits: TaskContextLimits,
    ) -> Result<TaskContextBuild, String> {
        match scene {
            TaskContextMode::CallChain => {
                let Some(from_anchor) = resolved.symbols.first() else {
                    resolved
                        .penalties
                        .push("call_chain requires two resolved symbol anchors".to_string());
                    return Ok(Self::needs_anchor_task_context(
                        "Add two symbol anchors to build a call chain.",
                    ));
                };
                let Some(to_anchor) = resolved.symbols.get(1) else {
                    resolved
                        .penalties
                        .push("call_chain requires two resolved symbol anchors".to_string());
                    return Ok(Self::needs_anchor_task_context(
                        "Add two symbol anchors to build a call chain.",
                    ));
                };

                let path = storage
                    .get_call_chain_path(
                        &from_anchor.symbol.id,
                        &to_anchor.symbol.id,
                        DEFAULT_DEPENDENCY_DEPTH,
                    )
                    .map_err(Self::storage_error_message)?;
                let path_symbols = path
                    .iter()
                    .map(|node| {
                        json!({
                            "id": node.symbol_id,
                            "name": node.name,
                            "qualified_name": node.qualified_name,
                            "file_path": node.file_path,
                            "depth": node.depth,
                            "dep_kind": node.dep_kind.as_ref().map(|kind| kind.as_str()),
                        })
                    })
                    .collect::<Vec<_>>();
                let related_files =
                    Self::call_chain_related_files_value(&path, limits.related_files);
                let key_edges = path
                    .windows(2)
                    .map(|window| {
                        json!({
                            "from": window[0].qualified_name,
                            "to": window[1].qualified_name,
                            "dependency_kind": window[1].dep_kind.as_ref().map(|kind| kind.as_str()),
                            "depth": window[1].depth,
                        })
                    })
                    .collect::<Vec<_>>();
                let source_snippets = if limits.source_snippets == 0 {
                    Vec::new()
                } else {
                    Self::source_snippets_for_symbols(
                        project_root,
                        vec![
                            (&from_anchor.symbol, "call chain source"),
                            (&to_anchor.symbol, "call chain destination"),
                        ],
                        limits.source_snippets,
                    )
                };
                let suggested_reads = if path.is_empty() {
                    vec![
                        json!({
                            "kind": "symbol",
                            "qualified_name": from_anchor.symbol.qualified_name,
                            "reason": "call chain source",
                        }),
                        json!({
                            "kind": "symbol",
                            "qualified_name": to_anchor.symbol.qualified_name,
                            "reason": "call chain destination",
                        }),
                    ]
                } else {
                    path.iter()
                        .take(limits.primary_symbols)
                        .map(|node| {
                            json!({
                                "kind": "symbol",
                                "qualified_name": node.qualified_name,
                                "reason": "call chain step",
                            })
                        })
                        .collect::<Vec<_>>()
                };

                if path.is_empty() {
                    resolved
                        .penalties
                        .push("no static call chain found between the two anchors".to_string());
                    let package = json!({
                        "target": {
                            "from": Self::interface_summary_value(&from_anchor.symbol),
                            "to": Self::interface_summary_value(&to_anchor.symbol),
                        },
                        "primary_symbols": [
                            Self::interface_summary_value(&from_anchor.symbol),
                            Self::interface_summary_value(&to_anchor.symbol),
                        ],
                        "primary_files": related_files.iter().take(limits.primary_files).cloned().collect::<Vec<_>>(),
                        "key_edges": [],
                        "related_files": related_files,
                        "suggested_reads": suggested_reads,
                        "source_snippets": source_snippets,
                        "risk_summary": Value::Null,
                    });
                    return Ok(TaskContextBuild {
                        status: "insufficient_graph",
                        coverage: "minimal",
                        package,
                        graph_signals: vec!["no call chain path found".to_string()],
                        expansion_hint: Some("expand_path_search"),
                        next_tool_calls: vec![Self::batch_query_tool_call(
                            &request.project,
                            vec![
                                json!({
                                    "kind": "get_dependencies",
                                    "interface": from_anchor.symbol.qualified_name,
                                    "direction": "outgoing",
                                    "max_depth": 2,
                                }),
                                json!({
                                    "kind": "get_dependencies",
                                    "interface": to_anchor.symbol.qualified_name,
                                    "direction": "incoming",
                                    "max_depth": 2,
                                }),
                            ],
                            "Expand both anchor neighborhoods to inspect the gap before reading source code.",
                        )],
                        fallback_to_code: false,
                        note: "Static graph could not connect the two anchors directly.".to_string(),
                        confidence_delta: -0.12,
                    });
                }

                let package = json!({
                    "target": {
                        "from": Self::interface_summary_value(&from_anchor.symbol),
                        "to": Self::interface_summary_value(&to_anchor.symbol),
                    },
                    "primary_symbols": path_symbols,
                    "primary_files": related_files.iter().take(limits.primary_files).cloned().collect::<Vec<_>>(),
                    "key_edges": key_edges,
                    "related_files": related_files,
                    "suggested_reads": suggested_reads,
                    "source_snippets": source_snippets,
                    "risk_summary": Value::Null,
                });
                Ok(TaskContextBuild {
                    status: "ready",
                    coverage: "strong",
                    package,
                    graph_signals: vec!["call chain path found".to_string()],
                    expansion_hint: None,
                    next_tool_calls: Vec::new(),
                    fallback_to_code: false,
                    note: "QuickDep found a static call chain between the two anchors.".to_string(),
                    confidence_delta: 0.16,
                })
            }
            TaskContextMode::Impact | TaskContextMode::Behavior | TaskContextMode::Locate => {
                if resolved.symbols.is_empty() {
                    if scene == TaskContextMode::Locate && !resolved.files.is_empty() {
                        return Self::build_file_locate_task_context(
                            storage,
                            project_root,
                            resolved,
                            limits,
                        );
                    }

                    resolved.penalties.push(format!(
                        "{} requires a resolved symbol anchor",
                        scene.as_str()
                    ));
                    return Ok(Self::needs_anchor_task_context(
                        "Add a symbol anchor or selection symbol to focus the task context.",
                    ));
                }

                let target = &resolved.symbols[0].symbol;
                let context = Self::load_symbol_context(storage, target, limits)?;
                let caller_summaries = context
                    .callers
                    .iter()
                    .map(|dependency| Self::interface_summary_value(&dependency.symbol))
                    .collect::<Vec<_>>();
                let callee_summaries = context
                    .callees
                    .iter()
                    .map(|dependency| Self::interface_summary_value(&dependency.symbol))
                    .collect::<Vec<_>>();
                let same_file_summaries = context
                    .same_file_symbols
                    .iter()
                    .map(Self::interface_summary_value)
                    .collect::<Vec<_>>();
                let related_files = Self::related_files_value(
                    target,
                    &caller_summaries,
                    &callee_summaries,
                    &same_file_summaries,
                    limits.related_files,
                );
                let primary_symbols =
                    Self::symbol_neighborhood_summaries(target, &context, limits.primary_symbols);
                let key_edges = Self::key_edges_from_symbol_context(target, &context);
                let suggested_reads = Self::suggested_reads_value(
                    target,
                    &caller_summaries,
                    &callee_summaries,
                    &same_file_summaries,
                );
                let source_snippets = if limits.source_snippets == 0 {
                    Vec::new()
                } else {
                    Self::source_snippets_for_symbols(
                        project_root,
                        Self::snippet_candidates_for_scene(scene, target, &context),
                        limits.source_snippets,
                    )
                };
                let risk_summary = if scene == TaskContextMode::Impact {
                    Self::impact_summary_value(
                        context.callers.len(),
                        context.callees.len(),
                        related_files.len(),
                    )
                } else {
                    Value::Null
                };
                let primary_files = related_files
                    .iter()
                    .take(limits.primary_files)
                    .cloned()
                    .collect::<Vec<_>>();
                let package = json!({
                    "target": Self::interface_summary_value(target),
                    "primary_symbols": primary_symbols,
                    "primary_files": primary_files,
                    "key_edges": key_edges,
                    "related_files": related_files,
                    "suggested_reads": suggested_reads,
                    "source_snippets": source_snippets,
                    "risk_summary": risk_summary,
                });

                let mut graph_signals = Vec::new();
                if !context.callers.is_empty() {
                    graph_signals.push(format!("{} direct callers found", context.callers.len()));
                }
                if !context.callees.is_empty() {
                    graph_signals.push(format!("{} direct callees found", context.callees.len()));
                }
                if !context.same_file_symbols.is_empty() {
                    graph_signals.push("same-file neighbors found".to_string());
                }

                if scene == TaskContextMode::Behavior {
                    return Ok(TaskContextBuild {
                        status: "needs_code_read",
                        coverage: "partial",
                        package,
                        graph_signals,
                        expansion_hint: Some("read_implementation"),
                        next_tool_calls: vec![Self::batch_query_tool_call(
                            &request.project,
                            vec![
                                json!({
                                    "kind": "get_dependencies",
                                    "interface": target.qualified_name,
                                    "direction": "incoming",
                                    "max_depth": 2,
                                }),
                                json!({
                                    "kind": "get_dependencies",
                                    "interface": target.qualified_name,
                                    "direction": "outgoing",
                                    "max_depth": 2,
                                }),
                            ],
                            "Expand one more hop around the anchor if you need more static structure before reading implementations.",
                        )],
                        fallback_to_code: true,
                        note: "Static graph narrowed the likely code region, but behavior confirmation still needs source reading.".to_string(),
                        confidence_delta: 0.04,
                    });
                }

                Ok(TaskContextBuild {
                    status: "ready",
                    coverage: if context.callers.is_empty() && context.callees.is_empty() {
                        "partial"
                    } else {
                        "strong"
                    },
                    package,
                    graph_signals,
                    expansion_hint: None,
                    next_tool_calls: Vec::new(),
                    fallback_to_code: false,
                    note: if scene == TaskContextMode::Impact {
                        "QuickDep produced an impact-focused neighborhood around the anchor."
                            .to_string()
                    } else {
                        "QuickDep narrowed the search area around the anchor symbol.".to_string()
                    },
                    confidence_delta: if scene == TaskContextMode::Impact {
                        0.12
                    } else {
                        0.08
                    },
                })
            }
            TaskContextMode::Workflow => {
                Self::build_workflow_task_context(storage, project_root, request, resolved, limits)
            }
            TaskContextMode::Watcher => Self::build_watcher_task_context(storage, resolved, limits),
            TaskContextMode::Auto => unreachable!("auto mode should be resolved before build"),
        }
    }

    fn needs_anchor_task_context(note: &str) -> TaskContextBuild {
        TaskContextBuild {
            status: "needs_anchor",
            coverage: "minimal",
            package: json!({
                "target": Value::Null,
                "primary_symbols": [],
                "primary_files": [],
                "key_edges": [],
                "related_files": [],
                "suggested_reads": [],
                "source_snippets": [],
                "risk_summary": Value::Null,
            }),
            graph_signals: Vec::new(),
            expansion_hint: Some("needs_anchor"),
            next_tool_calls: Vec::new(),
            fallback_to_code: false,
            note: note.to_string(),
            confidence_delta: -0.18,
        }
    }

    fn build_file_locate_task_context(
        storage: &Storage,
        project_root: &Path,
        resolved: &ResolvedTaskContext,
        limits: TaskContextLimits,
    ) -> Result<TaskContextBuild, String> {
        let Some(target_file) = resolved.files.first() else {
            return Ok(Self::needs_anchor_task_context(
                "Add a file or symbol anchor to locate relevant code.",
            ));
        };

        let mut primary_files = Vec::new();
        let mut primary_symbols = Vec::new();
        let mut seen_symbols = HashSet::new();
        for file_path in resolved.files.iter().take(limits.primary_files) {
            let file_state = storage
                .get_file_state(file_path)
                .map_err(Self::storage_error_message)?;
            let interfaces = storage
                .get_symbols_by_file(file_path)
                .map_err(Self::storage_error_message)?;
            primary_files.push(json!({
                "file_path": file_path,
                "reason": if file_path == target_file { "anchor file" } else { "related file anchor" },
                "symbol_count": interfaces.len(),
                "status": file_state.as_ref().map(|state| state.status.as_str()).unwrap_or("missing"),
                "error_message": file_state.and_then(|state| state.error_message),
            }));

            for symbol in interfaces {
                if primary_symbols.len() >= limits.primary_symbols {
                    break;
                }
                if seen_symbols.insert(symbol.id.clone()) {
                    primary_symbols.push(Self::interface_summary_value(&symbol));
                }
            }
        }

        let source_snippets = if limits.source_snippets == 0 || primary_symbols.is_empty() {
            Vec::new()
        } else {
            let snippets = storage
                .get_symbols_by_file(target_file)
                .map_err(Self::storage_error_message)?
                .into_iter()
                .take(limits.source_snippets)
                .collect::<Vec<_>>();
            let candidates = snippets
                .iter()
                .map(|symbol| (symbol, "anchored file symbol"))
                .collect::<Vec<_>>();
            Self::source_snippets_for_symbols(project_root, candidates, limits.source_snippets)
        };
        let suggested_reads = primary_symbols
            .iter()
            .take(3)
            .map(|summary| {
                json!({
                    "kind": "symbol",
                    "qualified_name": summary["qualified_name"].as_str().unwrap_or_default(),
                    "reason": "declared in anchored file",
                })
            })
            .collect::<Vec<_>>();
        let package = json!({
            "target": {
                "kind": "file",
                "file_path": target_file,
            },
            "primary_symbols": primary_symbols,
            "primary_files": primary_files.clone(),
            "key_edges": [],
            "related_files": primary_files,
            "suggested_reads": suggested_reads,
            "source_snippets": source_snippets,
            "risk_summary": Value::Null,
        });
        Ok(TaskContextBuild {
            status: "ready",
            coverage: "partial",
            package,
            graph_signals: vec!["file anchor resolved".to_string()],
            expansion_hint: None,
            next_tool_calls: Vec::new(),
            fallback_to_code: false,
            note: "QuickDep built a file-centered context because no symbol anchor was available."
                .to_string(),
            confidence_delta: 0.04,
        })
    }

    fn build_watcher_task_context(
        storage: &Storage,
        resolved: &mut ResolvedTaskContext,
        limits: TaskContextLimits,
    ) -> Result<TaskContextBuild, String> {
        if resolved.files.is_empty() {
            resolved
                .penalties
                .push("watcher mode requires at least one file anchor".to_string());
            return Ok(Self::needs_anchor_task_context(
                "Add one or more file anchors to inspect indexed watcher state.",
            ));
        }

        let mut primary_files = Vec::new();
        let mut primary_symbols = Vec::new();
        let mut seen_symbols = HashSet::new();
        for file_path in resolved.files.iter().take(limits.primary_files) {
            let file_state = storage
                .get_file_state(file_path)
                .map_err(Self::storage_error_message)?;
            let interfaces = storage
                .get_symbols_by_file(file_path)
                .map_err(Self::storage_error_message)?;
            primary_files.push(json!({
                "file_path": file_path,
                "status": file_state.as_ref().map(|state| state.status.as_str()).unwrap_or("missing"),
                "error_message": file_state.and_then(|state| state.error_message),
                "symbol_count": interfaces.len(),
            }));
            for symbol in interfaces {
                if primary_symbols.len() >= limits.primary_symbols {
                    break;
                }
                if seen_symbols.insert(symbol.id.clone()) {
                    primary_symbols.push(Self::interface_summary_value(&symbol));
                }
            }
        }

        let package = json!({
            "target": {
                "kind": "watcher",
                "files": resolved.files,
            },
            "primary_symbols": primary_symbols,
            "primary_files": primary_files.clone(),
            "key_edges": [],
            "related_files": primary_files,
            "suggested_reads": [],
            "source_snippets": [],
            "risk_summary": Value::Null,
        });
        Ok(TaskContextBuild {
            status: "ready",
            coverage: "strong",
            package,
            graph_signals: vec!["indexed file states loaded".to_string()],
            expansion_hint: None,
            next_tool_calls: Vec::new(),
            fallback_to_code: false,
            note: "Watcher mode reports the currently indexed state for the anchored files."
                .to_string(),
            confidence_delta: 0.08,
        })
    }

    fn build_workflow_task_context(
        storage: &Storage,
        project_root: &Path,
        request: &TaskContextRequest,
        resolved: &mut ResolvedTaskContext,
        limits: TaskContextLimits,
    ) -> Result<TaskContextBuild, String> {
        let workflow_seeds = resolved
            .symbols
            .iter()
            .map(|anchor| anchor.symbol.clone())
            .filter(Self::is_actionable_workflow_symbol)
            .take(limits.workflow_symbols)
            .collect::<Vec<_>>();

        if workflow_seeds.is_empty() {
            resolved.penalties.push(
                "workflow mode could not resolve an actionable function or method anchor"
                    .to_string(),
            );
            return Ok(Self::needs_anchor_task_context(
                "Add a symbol, file, stack trace symbol, or workspace selection to ground the workflow question.",
            ));
        }

        let candidates =
            Self::collect_workflow_candidates(storage, &workflow_seeds, limits.workflow_depth)?;
        let mut phases = Self::select_workflow_phase_symbols(&candidates);
        if phases.len() < 2 {
            resolved
                .penalties
                .push("workflow inference did not resolve enough distinct phases".to_string());
            return Ok(Self::needs_anchor_task_context(
                "Add a symbol or runtime anchor so QuickDep can connect the workflow stages.",
            ));
        }
        Self::attach_workflow_phase_supports(&candidates, &mut phases);

        let workflow_phases = phases
            .iter()
            .map(|selection| {
                json!({
                    "phase": selection.phase.key,
                    "label": selection.phase.label,
                    "symbol": Self::interface_summary_value(&selection.symbol),
                    "depth": selection.depth,
                    "score": selection.score,
                    "supporting_symbols": selection
                        .supporting
                        .iter()
                        .map(|support| {
                            json!({
                                "symbol": Self::interface_summary_value(&support.symbol),
                                "depth": support.depth,
                                "score": support.score,
                            })
                        })
                        .collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>();
        let primary_symbols = Self::workflow_primary_symbols_value(
            &phases,
            Self::workflow_primary_symbol_limit(limits, phases.len()),
        );
        let (key_edges, bridge_nodes, bridged_paths) =
            Self::workflow_bridge_edges(storage, &phases, limits.workflow_depth)?;
        let related_files =
            Self::workflow_related_files_value(&phases, &bridge_nodes, limits.related_files);
        let primary_files = related_files
            .iter()
            .take(limits.primary_files)
            .cloned()
            .collect::<Vec<_>>();
        let suggested_reads = phases
            .iter()
            .map(|selection| {
                json!({
                    "kind": "symbol",
                    "qualified_name": selection.symbol.qualified_name,
                    "reason": format!("workflow phase: {}", selection.phase.key),
                })
            })
            .collect::<Vec<_>>();
        let source_snippets = if limits.source_snippets == 0 {
            Vec::new()
        } else {
            let snippet_candidates = phases
                .iter()
                .map(|selection| (&selection.symbol, selection.phase.label))
                .collect::<Vec<_>>();
            Self::source_snippets_for_symbols(
                project_root,
                snippet_candidates,
                limits.source_snippets,
            )
        };

        let mut next_queries = Vec::new();
        if let Some(first_phase) = phases.first() {
            next_queries.push(json!({
                "kind": "get_dependencies",
                "interface": first_phase.symbol.qualified_name,
                "direction": "outgoing",
                "max_depth": 2,
            }));
        }
        if let Some(last_phase) = phases.last() {
            let should_add_last = phases
                .first()
                .map(|first_phase| first_phase.symbol.id != last_phase.symbol.id)
                .unwrap_or(true);
            if should_add_last {
                next_queries.push(json!({
                    "kind": "get_dependencies",
                    "interface": last_phase.symbol.qualified_name,
                    "direction": "incoming",
                    "max_depth": 2,
                }));
            }
        }
        let next_tool_calls = if next_queries.is_empty() {
            Vec::new()
        } else {
            vec![Self::batch_query_tool_call(
                &request.project,
                next_queries,
                "Expand the outer workflow stages if you need one more static hop before reading implementations.",
            )]
        };

        let mut graph_signals = vec![
            format!("{} workflow seed symbols resolved", workflow_seeds.len()),
            format!("{} workflow phases selected", phases.len()),
        ];
        let support_count = phases
            .iter()
            .map(|selection| selection.supporting.len())
            .sum::<usize>();
        if support_count > 0 {
            graph_signals.push(format!(
                "{} supporting workflow symbol(s) added",
                support_count
            ));
        }
        if bridged_paths > 0 {
            graph_signals.push(format!(
                "{} bridge path(s) found between adjacent workflow phases",
                bridged_paths
            ));
        } else {
            graph_signals
                .push("workflow phases were inferred from symbol neighborhoods".to_string());
        }

        let package = json!({
            "target": {
                "kind": "workflow",
                "question": request.question,
                "seed_symbols": workflow_seeds
                    .iter()
                    .map(Self::interface_summary_value)
                    .collect::<Vec<_>>(),
            },
            "primary_symbols": primary_symbols,
            "primary_files": primary_files,
            "key_edges": key_edges,
            "related_files": related_files,
            "suggested_reads": suggested_reads,
            "source_snippets": source_snippets,
            "risk_summary": Value::Null,
            "workflow_phases": workflow_phases,
        });

        let strong_coverage = phases.len() >= 4;
        Ok(TaskContextBuild {
            status: "needs_code_read",
            coverage: if strong_coverage { "strong" } else { "partial" },
            package,
            graph_signals,
            expansion_hint: Some("read_stage_implementations"),
            next_tool_calls,
            fallback_to_code: true,
            note: if strong_coverage {
                "QuickDep assembled a multi-stage workflow map; read one implementation per phase to confirm state gating and scheduler behavior.".to_string()
            } else {
                "QuickDep inferred a partial workflow map; read the selected stages to confirm the missing transitions.".to_string()
            },
            confidence_delta: if strong_coverage { 0.14 } else { 0.08 },
        })
    }

    fn collect_workflow_candidates(
        storage: &Storage,
        workflow_seeds: &[Symbol],
        max_depth: u32,
    ) -> Result<Vec<WorkflowCandidate>, String> {
        let mut candidates = HashMap::<String, WorkflowCandidate>::new();

        for seed in workflow_seeds {
            candidates
                .entry(seed.id.clone())
                .or_insert(WorkflowCandidate {
                    symbol: seed.clone(),
                    depth: 0,
                });

            for node in storage
                .get_dependency_chain_forward(&seed.id, max_depth)
                .map_err(Self::storage_error_message)?
                .into_iter()
                .chain(
                    storage
                        .get_dependency_chain_backward(&seed.id, max_depth)
                        .map_err(Self::storage_error_message)?,
                )
            {
                let Some(symbol) = storage
                    .get_symbol(&node.symbol_id)
                    .map_err(Self::storage_error_message)?
                else {
                    continue;
                };
                if !Self::is_actionable_workflow_symbol(&symbol) {
                    continue;
                }

                match candidates.get_mut(&symbol.id) {
                    Some(existing) if node.depth < existing.depth => {
                        existing.depth = node.depth;
                        existing.symbol = symbol;
                    }
                    Some(_) => {}
                    None => {
                        candidates.insert(
                            symbol.id.clone(),
                            WorkflowCandidate {
                                symbol,
                                depth: node.depth,
                            },
                        );
                    }
                }
            }
        }

        let mut collected = candidates.into_values().collect::<Vec<_>>();
        collected.sort_by(|left, right| {
            left.depth
                .cmp(&right.depth)
                .then_with(|| left.symbol.file_path.cmp(&right.symbol.file_path))
                .then_with(|| left.symbol.line.cmp(&right.symbol.line))
                .then_with(|| left.symbol.qualified_name.cmp(&right.symbol.qualified_name))
        });
        Ok(collected)
    }

    fn select_workflow_phase_symbols(
        candidates: &[WorkflowCandidate],
    ) -> Vec<WorkflowPhaseSelection> {
        let mut selections = Vec::new();
        let mut used = HashSet::new();

        for phase in Self::workflow_phase_specs() {
            let mut ranked = candidates
                .iter()
                .filter(|candidate| !used.contains(&candidate.symbol.id))
                .map(|candidate| {
                    (
                        Self::workflow_phase_score(&candidate.symbol, *phase)
                            - candidate.depth as i32,
                        candidate,
                    )
                })
                .filter(|(score, _)| *score > 0)
                .collect::<Vec<_>>();

            ranked.sort_by(|left, right| {
                right
                    .0
                    .cmp(&left.0)
                    .then_with(|| left.1.depth.cmp(&right.1.depth))
                    .then_with(|| left.1.symbol.file_path.cmp(&right.1.symbol.file_path))
                    .then_with(|| left.1.symbol.line.cmp(&right.1.symbol.line))
                    .then_with(|| {
                        left.1
                            .symbol
                            .qualified_name
                            .cmp(&right.1.symbol.qualified_name)
                    })
            });

            if let Some((score, candidate)) = ranked.into_iter().next() {
                used.insert(candidate.symbol.id.clone());
                selections.push(WorkflowPhaseSelection {
                    phase: *phase,
                    symbol: candidate.symbol.clone(),
                    depth: candidate.depth,
                    score,
                    supporting: Vec::new(),
                });
            }
        }

        selections
    }

    fn attach_workflow_phase_supports(
        candidates: &[WorkflowCandidate],
        phases: &mut [WorkflowPhaseSelection],
    ) {
        let mut used = phases
            .iter()
            .map(|selection| selection.symbol.id.clone())
            .collect::<HashSet<_>>();

        for selection in phases.iter_mut() {
            let support_limit = Self::workflow_phase_support_limit(selection.phase);
            if support_limit == 0 {
                continue;
            }

            let mut ranked = candidates
                .iter()
                .filter(|candidate| !used.contains(&candidate.symbol.id))
                .map(|candidate| {
                    (
                        Self::workflow_phase_support_score(candidate, selection),
                        candidate,
                    )
                })
                .filter(|(score, _)| *score > 0)
                .collect::<Vec<_>>();

            ranked.sort_by(|left, right| {
                right
                    .0
                    .cmp(&left.0)
                    .then_with(|| left.1.depth.cmp(&right.1.depth))
                    .then_with(|| left.1.symbol.file_path.cmp(&right.1.symbol.file_path))
                    .then_with(|| left.1.symbol.line.cmp(&right.1.symbol.line))
                    .then_with(|| {
                        left.1
                            .symbol
                            .qualified_name
                            .cmp(&right.1.symbol.qualified_name)
                    })
            });

            for (score, candidate) in ranked.into_iter().take(support_limit) {
                if !used.insert(candidate.symbol.id.clone()) {
                    continue;
                }
                selection.supporting.push(WorkflowPhaseSupport {
                    symbol: candidate.symbol.clone(),
                    depth: candidate.depth,
                    score,
                });
            }
        }
    }

    fn workflow_primary_symbols_value(
        phases: &[WorkflowPhaseSelection],
        limit: usize,
    ) -> Vec<Value> {
        let mut summaries = Vec::new();
        let mut seen = HashSet::new();

        for selection in phases {
            if summaries.len() >= limit {
                break;
            }
            if seen.insert(selection.symbol.id.clone()) {
                summaries.push(Self::interface_summary_value(&selection.symbol));
            }

            for support in &selection.supporting {
                if summaries.len() >= limit {
                    break;
                }
                if seen.insert(support.symbol.id.clone()) {
                    summaries.push(Self::interface_summary_value(&support.symbol));
                }
            }
        }

        summaries
    }

    fn workflow_bridge_edges(
        storage: &Storage,
        phases: &[WorkflowPhaseSelection],
        max_depth: u32,
    ) -> Result<(Vec<Value>, Vec<crate::storage::DependencyNode>, usize), String> {
        let mut edges = Vec::new();
        let mut bridge_nodes = Vec::new();
        let mut seen_edges = HashSet::new();
        let mut bridged_paths = 0;

        for window in phases.windows(2) {
            let path = storage
                .get_call_chain_path(&window[0].symbol.id, &window[1].symbol.id, max_depth)
                .map_err(Self::storage_error_message)?;
            if path.len() < 2 {
                continue;
            }

            bridged_paths += 1;
            bridge_nodes.extend(path.iter().cloned());
            for edge_window in path.windows(2) {
                let edge_key =
                    format!("{}->{}", edge_window[0].symbol_id, edge_window[1].symbol_id);
                if !seen_edges.insert(edge_key) {
                    continue;
                }

                edges.push(json!({
                    "from": edge_window[0].qualified_name,
                    "to": edge_window[1].qualified_name,
                    "phase_from": window[0].phase.key,
                    "phase_to": window[1].phase.key,
                    "dependency_kind": edge_window[1].dep_kind.as_ref().map(|kind| kind.as_str()),
                    "depth": edge_window[1].depth,
                }));
            }
        }

        Ok((edges, bridge_nodes, bridged_paths))
    }

    fn workflow_related_files_value(
        phases: &[WorkflowPhaseSelection],
        bridge_nodes: &[crate::storage::DependencyNode],
        limit: usize,
    ) -> Vec<Value> {
        let mut file_stats = HashMap::<String, (HashSet<String>, HashSet<String>)>::new();

        for phase in phases {
            let entry = file_stats
                .entry(phase.symbol.file_path.clone())
                .or_default();
            entry.0.insert(phase.symbol.qualified_name.clone());
            entry
                .1
                .insert(format!("workflow phase: {}", phase.phase.key));

            for support in &phase.supporting {
                let entry = file_stats
                    .entry(support.symbol.file_path.clone())
                    .or_default();
                entry.0.insert(support.symbol.qualified_name.clone());
                entry
                    .1
                    .insert(format!("workflow support: {}", phase.phase.key));
            }
        }

        for node in bridge_nodes {
            let entry = file_stats.entry(node.file_path.clone()).or_default();
            entry.0.insert(node.qualified_name.clone());
            entry.1.insert("workflow bridge path".to_string());
        }

        let mut files = file_stats
            .into_iter()
            .map(|(file_path, (symbols, reasons))| {
                let mut reason_list = reasons.into_iter().collect::<Vec<_>>();
                reason_list.sort_unstable();
                json!({
                    "file_path": file_path,
                    "reason": reason_list.join(", "),
                    "symbol_count": symbols.len(),
                })
            })
            .collect::<Vec<_>>();

        files.sort_by(|left, right| {
            let left_count = left["symbol_count"].as_u64().unwrap_or_default();
            let right_count = right["symbol_count"].as_u64().unwrap_or_default();
            right_count.cmp(&left_count).then_with(|| {
                left["file_path"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["file_path"].as_str().unwrap_or_default())
            })
        });
        files.truncate(limit);
        files
    }

    fn load_symbol_context(
        storage: &Storage,
        target: &Symbol,
        limits: TaskContextLimits,
    ) -> Result<SymbolContext, String> {
        let callers = Self::direct_dependency_entries(
            storage,
            storage
                .get_dependency_chain_backward(&target.id, 1)
                .map_err(Self::storage_error_message)?,
            &target.id,
            limits.direct_neighbors,
            "incoming",
        )?;
        let callees = Self::direct_dependency_entries(
            storage,
            storage
                .get_dependency_chain_forward(&target.id, 1)
                .map_err(Self::storage_error_message)?,
            &target.id,
            limits.direct_neighbors,
            "outgoing",
        )?;
        let same_file_symbols = Self::same_file_symbols(storage, target, limits.same_file_symbols)?;
        Ok(SymbolContext {
            callers,
            callees,
            same_file_symbols,
        })
    }

    fn direct_dependency_entries(
        storage: &Storage,
        chain: Vec<crate::storage::DependencyNode>,
        symbol_id: &str,
        limit: usize,
        direction: &'static str,
    ) -> Result<Vec<ContextDependency>, String> {
        let mut seen = HashSet::new();
        let mut results = Vec::new();

        for node in chain {
            if node.depth != 1
                || node.symbol_id == symbol_id
                || !seen.insert(node.symbol_id.clone())
            {
                continue;
            }

            let Some(symbol) = storage
                .get_symbol(&node.symbol_id)
                .map_err(Self::storage_error_message)?
            else {
                continue;
            };

            results.push(ContextDependency {
                symbol,
                direction,
                dep_kind: node.dep_kind.map(|kind| kind.as_str().to_string()),
            });
            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
    }

    fn same_file_symbols(
        storage: &Storage,
        target: &Symbol,
        limit: usize,
    ) -> Result<Vec<Symbol>, String> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut symbols = storage
            .get_symbols_by_file(&target.file_path)
            .map_err(Self::storage_error_message)?
            .into_iter()
            .filter(|symbol| symbol.id != target.id)
            .collect::<Vec<_>>();
        symbols.sort_by(|left, right| {
            let left_public = matches!(left.visibility, crate::core::Visibility::Public);
            let right_public = matches!(right.visibility, crate::core::Visibility::Public);
            right_public
                .cmp(&left_public)
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.name.cmp(&right.name))
        });
        symbols.truncate(limit);
        Ok(symbols)
    }

    fn symbol_neighborhood_summaries(
        target: &Symbol,
        context: &SymbolContext,
        limit: usize,
    ) -> Vec<Value> {
        let mut summaries = Vec::new();
        let mut seen = HashSet::new();
        Self::push_unique_symbol_summary(&mut summaries, &mut seen, target);
        for dependency in &context.callers {
            if summaries.len() >= limit {
                break;
            }
            Self::push_unique_symbol_summary(&mut summaries, &mut seen, &dependency.symbol);
        }
        for dependency in &context.callees {
            if summaries.len() >= limit {
                break;
            }
            Self::push_unique_symbol_summary(&mut summaries, &mut seen, &dependency.symbol);
        }
        for symbol in &context.same_file_symbols {
            if summaries.len() >= limit {
                break;
            }
            Self::push_unique_symbol_summary(&mut summaries, &mut seen, symbol);
        }
        summaries
    }

    fn push_unique_symbol_summary(
        summaries: &mut Vec<Value>,
        seen: &mut HashSet<String>,
        symbol: &Symbol,
    ) {
        if seen.insert(symbol.id.clone()) {
            summaries.push(Self::interface_summary_value(symbol));
        }
    }

    fn key_edges_from_symbol_context(target: &Symbol, context: &SymbolContext) -> Vec<Value> {
        let mut edges = Vec::new();
        for dependency in &context.callers {
            edges.push(json!({
                "from": dependency.symbol.qualified_name,
                "to": target.qualified_name,
                "direction": dependency.direction,
                "dependency_kind": dependency.dep_kind,
            }));
        }
        for dependency in &context.callees {
            edges.push(json!({
                "from": target.qualified_name,
                "to": dependency.symbol.qualified_name,
                "direction": dependency.direction,
                "dependency_kind": dependency.dep_kind,
            }));
        }
        edges
    }

    fn snippet_candidates_for_scene<'a>(
        scene: TaskContextMode,
        target: &'a Symbol,
        context: &'a SymbolContext,
    ) -> Vec<(&'a Symbol, &'static str)> {
        let mut candidates = vec![(target, "target symbol")];
        match scene {
            TaskContextMode::Behavior => {
                for dependency in context.callers.iter().take(1) {
                    candidates.push((&dependency.symbol, "direct caller"));
                }
                for dependency in context.callees.iter().take(1) {
                    candidates.push((&dependency.symbol, "direct callee"));
                }
            }
            TaskContextMode::Impact => {
                for dependency in context.callers.iter().take(1) {
                    candidates.push((&dependency.symbol, "direct caller"));
                }
            }
            TaskContextMode::Locate => {
                if let Some(symbol) = context.same_file_symbols.first() {
                    candidates.push((symbol, "same-file neighbor"));
                }
            }
            _ => {}
        }
        candidates
    }

    fn related_files_value(
        symbol: &Symbol,
        callers: &[Value],
        callees: &[Value],
        same_file_interfaces: &[Value],
        max_related_files: usize,
    ) -> Vec<Value> {
        let mut file_stats: HashMap<String, (HashSet<String>, HashSet<&'static str>)> =
            HashMap::new();
        let target_entry = file_stats.entry(symbol.file_path.clone()).or_default();
        target_entry.0.insert(symbol.qualified_name.clone());
        target_entry.1.insert("contains target symbol");

        for same_file in same_file_interfaces {
            if let Some(qualified_name) = same_file.get("qualified_name").and_then(Value::as_str) {
                target_entry.0.insert(qualified_name.to_string());
            }
        }

        for caller in callers {
            Self::apply_related_file_summary(&mut file_stats, caller, "contains direct caller");
        }

        for callee in callees {
            Self::apply_related_file_summary(&mut file_stats, callee, "contains direct callee");
        }

        let mut files = file_stats
            .into_iter()
            .map(|(file_path, (symbols, reasons))| {
                let mut reason_list = reasons.into_iter().collect::<Vec<_>>();
                reason_list.sort_unstable();
                json!({
                    "file_path": file_path,
                    "reason": reason_list.join(", "),
                    "symbol_count": symbols.len(),
                })
            })
            .collect::<Vec<_>>();

        files.sort_by(|left, right| {
            let left_path = left["file_path"].as_str().unwrap_or_default();
            let right_path = right["file_path"].as_str().unwrap_or_default();
            let left_is_target = left_path == symbol.file_path;
            let right_is_target = right_path == symbol.file_path;
            right_is_target
                .cmp(&left_is_target)
                .then_with(|| {
                    let left_count = left["symbol_count"].as_u64().unwrap_or_default();
                    let right_count = right["symbol_count"].as_u64().unwrap_or_default();
                    right_count.cmp(&left_count)
                })
                .then_with(|| left_path.cmp(right_path))
        });
        files.truncate(max_related_files);
        files
    }

    fn apply_related_file_summary(
        file_stats: &mut HashMap<String, (HashSet<String>, HashSet<&'static str>)>,
        summary: &Value,
        reason: &'static str,
    ) {
        let Some(file_path) = summary.get("file_path").and_then(Value::as_str) else {
            return;
        };
        let Some(qualified_name) = summary.get("qualified_name").and_then(Value::as_str) else {
            return;
        };

        let entry = file_stats.entry(file_path.to_string()).or_default();
        entry.0.insert(qualified_name.to_string());
        entry.1.insert(reason);
    }

    fn suggested_reads_value(
        symbol: &Symbol,
        callers: &[Value],
        callees: &[Value],
        same_file_interfaces: &[Value],
    ) -> Vec<Value> {
        let mut reads = vec![json!({
            "kind": "symbol",
            "qualified_name": symbol.qualified_name,
            "reason": "target symbol",
        })];

        for caller in callers {
            reads.push(json!({
                "kind": "symbol",
                "qualified_name": caller["qualified_name"].as_str().unwrap_or_default(),
                "reason": "direct caller",
            }));
        }

        for callee in callees {
            reads.push(json!({
                "kind": "symbol",
                "qualified_name": callee["qualified_name"].as_str().unwrap_or_default(),
                "reason": "direct callee",
            }));
        }

        if !same_file_interfaces.is_empty() {
            reads.push(json!({
                "kind": "file",
                "file_path": symbol.file_path,
                "reason": "same file as target",
            }));
        }

        reads
    }

    fn impact_summary_value(
        caller_count: usize,
        callee_count: usize,
        related_file_count: usize,
    ) -> Value {
        let mut reasons = Vec::new();
        if caller_count > 10 {
            reasons.push("direct callers exceed 10");
        } else if caller_count > 3 {
            reasons.push("direct callers exceed 3");
        }

        if callee_count > 15 {
            reasons.push("direct callees exceed 15");
        } else if callee_count > 5 {
            reasons.push("direct callees exceed 5");
        }

        if related_file_count > 8 {
            reasons.push("touches more than 8 files");
        } else if related_file_count > 3 {
            reasons.push("touches more than 3 files");
        }

        let risk = if caller_count > 10 || callee_count > 15 || related_file_count > 8 {
            "high"
        } else if caller_count <= 3 && callee_count <= 5 && related_file_count <= 3 {
            "low"
        } else {
            "medium"
        };

        if reasons.is_empty() {
            reasons.push("direct impact is limited");
        }

        json!({
            "risk": risk,
            "reasons": reasons,
        })
    }

    fn call_chain_related_files_value(
        path: &[crate::storage::DependencyNode],
        limit: usize,
    ) -> Vec<Value> {
        let mut file_stats: HashMap<String, usize> = HashMap::new();
        for node in path {
            *file_stats.entry(node.file_path.clone()).or_insert(0) += 1;
        }

        let mut files = file_stats
            .into_iter()
            .map(|(file_path, symbol_count)| {
                json!({
                    "file_path": file_path,
                    "reason": "call chain path",
                    "symbol_count": symbol_count,
                })
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| {
            right["symbol_count"]
                .as_u64()
                .unwrap_or_default()
                .cmp(&left["symbol_count"].as_u64().unwrap_or_default())
                .then_with(|| {
                    left["file_path"]
                        .as_str()
                        .unwrap_or_default()
                        .cmp(right["file_path"].as_str().unwrap_or_default())
                })
        });
        files.truncate(limit);
        files
    }

    fn batch_query_tool_call(project: &ProjectTarget, queries: Vec<Value>, reason: &str) -> Value {
        json!({
            "tool": "batch_query",
            "arguments": {
                "project": project,
                "queries": queries,
            },
            "reason": reason,
        })
    }

    fn source_snippets_for_symbols(
        project_root: &Path,
        candidates: Vec<(&Symbol, &'static str)>,
        limit: usize,
    ) -> Vec<Value> {
        let mut snippets = Vec::new();
        let mut seen = HashSet::new();
        for (symbol, reason) in candidates {
            if snippets.len() >= limit || !seen.insert(symbol.id.clone()) {
                continue;
            }
            if let Some(snippet) = Self::source_snippet_value(project_root, symbol, reason) {
                snippets.push(snippet);
            }
        }
        snippets
    }

    fn source_snippet_value(project_root: &Path, symbol: &Symbol, reason: &str) -> Option<Value> {
        let canonical = crate::security::validate_path(project_root, &symbol.file_path).ok()?;
        let content = std::fs::read_to_string(canonical).ok()?;
        let lines = content.lines().collect::<Vec<_>>();
        if lines.is_empty() {
            return None;
        }

        let highlight_line = usize::try_from(symbol.line).ok()?.clamp(1, lines.len());
        let start_line = highlight_line
            .saturating_sub(SOURCE_SNIPPET_CONTEXT_BEFORE)
            .max(1);
        let end_line = (highlight_line + SOURCE_SNIPPET_CONTEXT_AFTER).min(lines.len());
        let snippet = lines
            .get(start_line - 1..end_line)?
            .join("\n")
            .trim_end()
            .to_string();

        if snippet.is_empty() {
            return None;
        }

        Some(json!({
            "kind": "symbol",
            "id": symbol.id,
            "qualified_name": symbol.qualified_name,
            "file_path": symbol.file_path,
            "reason": reason,
            "start_line": start_line,
            "end_line": end_line,
            "highlight_line": symbol.line,
            "snippet": snippet,
            "estimated_tokens": Self::estimate_token_count(&snippet),
        }))
    }

    fn estimate_token_count(text: &str) -> usize {
        text.chars().count().div_ceil(4)
    }

    fn estimate_value_tokens(value: &Value) -> usize {
        serde_json::to_string(value)
            .map(|text| Self::estimate_token_count(&text))
            .unwrap_or_default()
    }

    fn push_unique_string(values: &mut Vec<String>, value: String) {
        if values.iter().all(|existing| existing != &value) {
            values.push(value);
        }
    }

    fn parse_resource_uri(uri: &str) -> Option<ResourceUri> {
        if uri == "quickdep://projects" {
            return Some(ResourceUri::Projects);
        }

        let suffix = uri.strip_prefix("quickdep://project/")?;
        let parts = suffix.split('/').collect::<Vec<_>>();
        match parts.as_slice() {
            [project_id, "status"] => Some(ResourceUri::ProjectStatus((*project_id).to_string())),
            [project_id, "interfaces"] => {
                Some(ResourceUri::ProjectInterfaces((*project_id).to_string()))
            }
            [project_id, "interface", symbol_id] => Some(ResourceUri::Interface(
                (*project_id).to_string(),
                (*symbol_id).to_string(),
            )),
            [project_id, "interface", symbol_id, "deps"] => Some(ResourceUri::InterfaceDeps(
                (*project_id).to_string(),
                (*symbol_id).to_string(),
            )),
            _ => None,
        }
    }

    fn project_id_error(error: impl ToString) -> McpError {
        Self::invalid_params(error.to_string())
    }

    fn manager_error(error: ManagerError) -> McpError {
        match error {
            ManagerError::NotFound(id) => {
                Self::invalid_params(format!("unknown project id: {}", id))
            }
            ManagerError::AlreadyRegistered(id) => {
                Self::invalid_params(format!("project already registered: {}", id))
            }
            other => Self::internal_error(other.to_string()),
        }
    }

    fn storage_error_message(error: StorageError) -> String {
        match error {
            StorageError::SchemaMismatch { expected, found } => format!(
                "database schema mismatch (expected {}, found {}); call rebuild_database",
                expected, found
            ),
            other => other.to_string(),
        }
    }

    fn serialization_error(error: serde_json::Error) -> McpError {
        Self::internal_error(format!("failed to serialize MCP response: {}", error))
    }

    fn invalid_params(message: impl Into<String>) -> McpError {
        McpError::invalid_params(message.into(), None)
    }

    fn internal_error(message: impl Into<String>) -> McpError {
        McpError::internal_error(message.into(), None)
    }
}

#[tool_router(router = tool_router)]
impl QuickDepServer {
    /// Create an MCP server instance from a prepared runtime.
    pub fn new(
        workspace_root: PathBuf,
        default_project_id: ProjectId,
        manager: Arc<ProjectManager>,
    ) -> Self {
        let (tool_router, enabled_tools) =
            Self::build_tool_router(None).expect("default tool router should be valid");
        Self {
            workspace_root,
            default_project_id,
            manager,
            caches: Arc::new(RwLock::new(HashMap::new())),
            watchers: Arc::new(RwLock::new(HashMap::new())),
            idle_checker_started: Arc::new(AtomicBool::new(false)),
            enabled_tools: Arc::new(enabled_tools),
            tool_router,
        }
    }

    fn force_task_context_mode(mut request: TaskContextRequest, mode: &str) -> TaskContextRequest {
        request.mode = Some(mode.to_string());
        request
    }

    /// List every known project.
    #[tool(description = "List registered QuickDep projects")]
    pub async fn list_projects(&self) -> McpResult<Json<Value>> {
        Ok(Json(self.list_projects_value().await?))
    }

    /// Scan a project and update its SQLite graph.
    #[tool(description = "Scan a project and build or update its dependency database")]
    pub async fn scan_project(
        &self,
        Parameters(request): Parameters<ScanProjectRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(self.scan_project_value(request).await?))
    }

    /// Read the current scan state of a project.
    #[tool(description = "Get the current scan status for a project")]
    pub async fn get_scan_status(
        &self,
        Parameters(request): Parameters<ProjectStatusRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(self.get_scan_status_value(request).await?))
    }

    /// Summarize the project-level dependency graph for visualization.
    #[tool(
        description = "Summarize the highest-degree local interfaces and dependencies in a project"
    )]
    pub async fn get_project_overview(
        &self,
        Parameters(request): Parameters<ProjectOverviewRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(self.get_project_overview_value(request).await?))
    }

    /// Request cancellation of an ongoing scan.
    #[tool(description = "Cancel the current scan for a project")]
    pub async fn cancel_scan(
        &self,
        Parameters(request): Parameters<ProjectStatusRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(self.cancel_scan_value(request).await?))
    }

    /// Fuzzy-search interfaces by name.
    #[tool(
        description = "Low-level symbol-name search. Prefer get_task_context or the scene-specific context tools first for natural-language why/risk/impact/workflow questions."
    )]
    pub async fn find_interfaces(
        &self,
        Parameters(request): Parameters<FindInterfacesRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(self.find_interfaces_value(request).await?))
    }

    /// Return the full details of one interface.
    #[tool(
        description = "Low-level symbol detail lookup when you already know the interface you want to inspect."
    )]
    pub async fn get_interface(
        &self,
        Parameters(request): Parameters<InterfaceLookupRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(self.get_interface_value(request).await?))
    }

    /// Traverse interface dependencies.
    #[tool(
        description = "Low-level dependency graph lookup for one known interface. Prefer high-level context tools first for natural-language analysis questions."
    )]
    pub async fn get_dependencies(
        &self,
        Parameters(request): Parameters<DependenciesRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(self.get_dependencies_value(request).await?))
    }

    /// Find a call path between two interfaces.
    #[tool(description = "Find a call chain path between two interfaces")]
    pub async fn get_call_chain(
        &self,
        Parameters(request): Parameters<CallChainRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(self.get_call_chain_value(request).await?))
    }

    /// List interfaces declared in one file.
    #[tool(description = "List interfaces declared in a file")]
    pub async fn get_file_interfaces(
        &self,
        Parameters(request): Parameters<FileInterfacesRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(self.get_file_interfaces_value(request).await?))
    }

    /// Build an agent-oriented task context package from anchors and workspace hints.
    #[tool(
        description = "First-choice tool for natural-language code questions. Builds a task-oriented context package for why/impact/workflow/locate analysis from anchors, workspace hints, and a question."
    )]
    pub async fn get_task_context(
        &self,
        Parameters(request): Parameters<TaskContextRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(self.get_task_context_value(request).await?))
    }

    /// Build a workflow-focused task context package from a natural-language question.
    #[tool(
        description = "First-choice tool for workflow, state transition, scheduling, queue, approval, or 'why is it still queued' questions. Forces workflow scene routing."
    )]
    pub async fn analyze_workflow_context(
        &self,
        Parameters(request): Parameters<TaskContextRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(
            self.get_task_context_value(Self::force_task_context_mode(request, "workflow"))
                .await?,
        ))
    }

    /// Build an impact-focused task context package from a natural-language question.
    #[tool(
        description = "First-choice tool for refactors, change risk, rename impact, or 'what will this affect' questions. Forces impact scene routing."
    )]
    pub async fn analyze_change_impact(
        &self,
        Parameters(request): Parameters<TaskContextRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(
            self.get_task_context_value(Self::force_task_context_mode(request, "impact"))
                .await?,
        ))
    }

    /// Build a behavior-focused task context package from a natural-language question.
    #[tool(
        description = "First-choice tool for why/how behavior, failures, stack traces, or runtime debugging questions. Forces behavior scene routing."
    )]
    pub async fn analyze_behavior_context(
        &self,
        Parameters(request): Parameters<TaskContextRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(
            self.get_task_context_value(Self::force_task_context_mode(request, "behavior"))
                .await?,
        ))
    }

    /// Build a locate-focused task context package from file or workspace anchors.
    #[tool(
        description = "First-choice tool when the goal is to find the most relevant files or symbols to read next for a feature or question. Forces locate scene routing."
    )]
    pub async fn locate_relevant_code(
        &self,
        Parameters(request): Parameters<TaskContextRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(
            self.get_task_context_value(Self::force_task_context_mode(request, "locate"))
                .await?,
        ))
    }

    /// Execute multiple read-only queries in one request.
    #[tool(description = "Execute a batch of interface and dependency queries")]
    pub async fn batch_query(
        &self,
        Parameters(request): Parameters<BatchQueryRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(self.batch_query_value(request).await?))
    }

    /// Force-delete and rebuild the database for a project.
    #[tool(description = "Delete and rebuild a project's dependency database")]
    pub async fn rebuild_database(
        &self,
        Parameters(request): Parameters<ProjectStatusRequest>,
    ) -> McpResult<Json<Value>> {
        Ok(Json(self.rebuild_database_value(request).await?))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for QuickDepServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_instructions(
            "QuickDep indexes project interfaces and dependency graphs. For natural-language engineering questions, start with get_task_context or the scene-specific tools analyze_workflow_context, analyze_change_impact, analyze_behavior_context, and locate_relevant_code. Use find_interfaces, get_interface, and get_dependencies as low-level follow-up queries once you already know the symbol or file you need.",
        )
    }

    async fn on_initialized(
        &self,
        _context: rmcp::service::NotificationContext<rmcp::service::RoleServer>,
    ) {
        if !self.idle_checker_started.swap(true, Ordering::SeqCst) {
            let manager = (*self.manager).clone();
            let watchers = self.watchers.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(IDLE_CHECK_INTERVAL).await;
                    let paused = manager.check_idle().await;
                    if paused.is_empty() {
                        continue;
                    }

                    let watcher_handles = watchers
                        .read()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    for project_id in &paused {
                        if let Some(handle) = watcher_handles.get(project_id) {
                            if let Err(error) = handle.command_tx.send(WatchCommand::Pause) {
                                warn!(
                                    "Failed to pause watcher for project {}: {}",
                                    project_id, error
                                );
                            }
                        }
                    }

                    info!("Paused {} idle projects", paused.len());
                }
            });
            info!("Started QuickDep watcher coordinator");
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> McpResult<ListResourcesResult> {
        let manifest = self.manager.get_manifest().await;
        let mut resources = Vec::new();
        resources.push(
            RawResource::new("quickdep://projects", "projects")
                .with_description("Registered QuickDep projects")
                .with_mime_type("application/json")
                .no_annotation(),
        );

        for entry in manifest.projects {
            resources.push(
                RawResource::new(
                    format!("quickdep://project/{}/status", entry.id),
                    format!("project-{}-status", entry.id),
                )
                .with_description("Project scan status")
                .with_mime_type("application/json")
                .no_annotation(),
            );
            resources.push(
                RawResource::new(
                    format!("quickdep://project/{}/interfaces", entry.id),
                    format!("project-{}-interfaces", entry.id),
                )
                .with_description("Project interface summaries")
                .with_mime_type("application/json")
                .no_annotation(),
            );
        }

        Ok(ListResourcesResult {
            meta: None,
            resources,
            next_cursor: None,
        })
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> McpResult<ListResourceTemplatesResult> {
        Ok(ListResourceTemplatesResult {
            meta: None,
            resource_templates: vec![
                RawResourceTemplate::new(
                    "quickdep://project/{project_id}/status",
                    "project-status",
                )
                .with_description("Project scan status")
                .with_mime_type("application/json")
                .no_annotation(),
                RawResourceTemplate::new(
                    "quickdep://project/{project_id}/interfaces",
                    "project-interfaces",
                )
                .with_description("Project interface summaries")
                .with_mime_type("application/json")
                .no_annotation(),
                RawResourceTemplate::new(
                    "quickdep://project/{project_id}/interface/{symbol_id}",
                    "interface-detail",
                )
                .with_description("Interface detail resource")
                .with_mime_type("application/json")
                .no_annotation(),
                RawResourceTemplate::new(
                    "quickdep://project/{project_id}/interface/{symbol_id}/deps",
                    "interface-dependencies",
                )
                .with_description("Interface dependency graph")
                .with_mime_type("application/json")
                .no_annotation(),
            ],
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> McpResult<ReadResourceResult> {
        let value = match Self::parse_resource_uri(&request.uri) {
            Some(ResourceUri::Projects) => self.list_projects_value().await?,
            Some(ResourceUri::ProjectStatus(project_id)) => {
                self.get_scan_status_value(ProjectStatusRequest {
                    project: ProjectTarget {
                        project_id: Some(project_id),
                        path: None,
                    },
                })
                .await?
            }
            Some(ResourceUri::ProjectInterfaces(project_id)) => {
                self.list_project_interfaces_value(ProjectTarget {
                    project_id: Some(project_id),
                    path: None,
                })
                .await?
            }
            Some(ResourceUri::Interface(project_id, symbol_id)) => {
                self.get_interface_value(InterfaceLookupRequest {
                    project: ProjectTarget {
                        project_id: Some(project_id),
                        path: None,
                    },
                    interface: symbol_id,
                })
                .await?
            }
            Some(ResourceUri::InterfaceDeps(project_id, symbol_id)) => {
                self.get_dependencies_value(DependenciesRequest {
                    project: ProjectTarget {
                        project_id: Some(project_id),
                        path: None,
                    },
                    interface: symbol_id,
                    direction: Some("outgoing".to_string()),
                    max_depth: Some(DEFAULT_DEPENDENCY_DEPTH),
                })
                .await?
            }
            None => {
                return Err(McpError::resource_not_found(
                    format!("unknown QuickDep resource: {}", request.uri),
                    None,
                ))
            }
        };

        let text = serde_json::to_string_pretty(&value).map_err(Self::serialization_error)?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::TextResourceContents {
                uri: request.uri,
                mime_type: Some("application/json".to_string()),
                text,
                meta: None,
            },
        ]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResourceUri {
    Projects,
    ProjectStatus(String),
    ProjectInterfaces(String),
    Interface(String, String),
    InterfaceDeps(String, String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::{model::CallToolRequestParams, ClientHandler, ServiceExt};
    use tempfile::TempDir;

    #[derive(Debug, Clone, Default)]
    struct TestClient;

    impl ClientHandler for TestClient {
        fn get_info(&self) -> rmcp::model::ClientInfo {
            rmcp::model::ClientInfo::default()
        }
    }

    fn write_sample_project(root: &Path) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            r#"
pub fn entry() {
    helper();
}

pub fn helper() {}
"#,
        )
        .unwrap();
    }

    fn write_large_sample_project(root: &Path, file_count: usize) {
        std::fs::create_dir_all(root.join("src")).unwrap();

        for index in 0..file_count {
            std::fs::write(
                root.join("src").join(format!("module_{index}.rs")),
                format!(
                    "pub fn helper_{index}() -> usize {{ {index} }}\n\npub fn caller_{index}() -> usize {{\n    helper_{index}()\n}}\n"
                ),
            )
            .unwrap();
        }
    }

    fn write_task_context_project(root: &Path) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            r#"
mod callers;
mod helpers;

pub fn entry() {
    helper();
}

pub fn helper() {
    format_value();
    store();
}

pub fn format_value() {}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/callers.rs"),
            r#"
use crate::helper;

pub fn run() {
    helper();
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/helpers.rs"),
            r#"
pub fn store() {}
"#,
        )
        .unwrap();
    }

    fn write_ambiguous_task_context_project(root: &Path) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            r#"
mod helpers;

pub fn helper() {}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/helpers.rs"),
            r#"
pub fn helper() {}
"#,
        )
        .unwrap();
    }

    fn write_workflow_task_context_project(root: &Path) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            r#"
mod core_flow_service;
mod execution;
mod flow;
mod runtime;
mod scheduler;
mod store;
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/store.rs"),
            r#"
use crate::runtime::approval_resolve;

pub fn approve_pending_approval() {
    approval_resolve();
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/runtime.rs"),
            r#"
use crate::core_flow_service::resume_approved_execution;

pub fn approval_resolve() {
    resume_approved_execution();
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/core_flow_service.rs"),
            r#"
use crate::flow::dispatch_execution;

pub fn resume_approved_execution() {
    dispatch_execution();
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/flow.rs"),
            r#"
use crate::execution::next_conflict_queue_head;

pub fn dispatch_execution() {
    prepare_execution_dispatch();
}

pub fn prepare_execution_dispatch() {
    next_conflict_queue_head();
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/execution.rs"),
            r#"
use crate::scheduler::admit;

pub fn next_conflict_queue_head() {
    admit();
}
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/scheduler.rs"),
            r#"
pub fn admit() {
    dispatchable_head();
}

pub fn dispatchable_head() {}
"#,
        )
        .unwrap();
    }

    fn workflow_candidate(qualified_name: &str, file_path: &str, depth: u32) -> WorkflowCandidate {
        let name = qualified_name
            .rsplit("::")
            .next()
            .unwrap_or(qualified_name)
            .to_string();
        WorkflowCandidate {
            symbol: Symbol::new(
                name,
                qualified_name.to_string(),
                crate::core::SymbolKind::Method,
                file_path.to_string(),
                depth + 1,
                1,
            )
            .with_visibility(crate::core::Visibility::Public),
            depth,
        }
    }

    async fn start_test_server(
        project_root: &Path,
    ) -> anyhow::Result<rmcp::service::RunningService<rmcp::service::RoleClient, TestClient>> {
        start_test_server_with_tools(project_root, None).await
    }

    async fn start_test_server_with_tools(
        project_root: &Path,
        allowed_tools: Option<Vec<String>>,
    ) -> anyhow::Result<rmcp::service::RunningService<rmcp::service::RoleClient, TestClient>> {
        let (server_transport, client_transport) = tokio::io::duplex(4096);
        let server = match allowed_tools {
            Some(allowed_tools) => {
                QuickDepServer::from_workspace_with_tools(project_root, allowed_tools).await?
            }
            None => QuickDepServer::from_workspace(project_root).await?,
        };
        tokio::spawn(async move {
            server.serve(server_transport).await?.waiting().await?;
            anyhow::Ok(())
        });

        Ok(TestClient.serve(client_transport).await?)
    }

    async fn wait_for_symbol(
        server: &QuickDepServer,
        project: &Project,
        query: &str,
    ) -> anyhow::Result<bool> {
        for _ in 0..50 {
            let value = server
                .find_interfaces_value(FindInterfacesRequest {
                    project: ProjectTarget::default(),
                    query: query.to_string(),
                    limit: Some(5),
                })
                .await
                .map_err(|error| anyhow!(error.message.clone()))?;
            if value["interfaces"]
                .as_array()
                .is_some_and(|interfaces| !interfaces.is_empty())
            {
                return Ok(true);
            }

            let exists = QuickDepServer::blocking_storage(project.database_path(), {
                let qualified_name = format!("src/lib.rs::{query}");
                move |storage| {
                    storage
                        .get_symbol_by_qualified_name(&qualified_name)
                        .map(|symbol| symbol.is_some())
                        .map_err(|error| error.to_string())
                }
            })
            .await
            .map_err(|error| anyhow!(error.message.clone()))?;
            if exists {
                return Ok(true);
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Ok(false)
    }

    #[tokio::test]
    async fn test_mcp_tools_scan_and_query_interfaces() -> anyhow::Result<()> {
        let project_dir = TempDir::new()?;
        write_sample_project(project_dir.path());
        let client = start_test_server(project_dir.path()).await?;

        let tools = client.list_all_tools().await?;
        assert!(tools.iter().any(|tool| tool.name == "scan_project"));
        assert!(tools.iter().any(|tool| tool.name == "find_interfaces"));
        assert!(tools
            .iter()
            .any(|tool| tool.name == "analyze_workflow_context"));
        assert!(tools
            .iter()
            .any(|tool| tool.name == "analyze_change_impact"));
        assert!(tools
            .iter()
            .any(|tool| tool.name == "analyze_behavior_context"));
        assert!(tools.iter().any(|tool| tool.name == "locate_relevant_code"));

        let scan_result = client
            .call_tool(
                CallToolRequestParams::new("scan_project").with_arguments(
                    json!({
                        "project": {
                            "path": project_dir.path().display().to_string()
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;
        assert_eq!(scan_result.is_error, Some(false));

        let find_result = client
            .call_tool(
                CallToolRequestParams::new("find_interfaces").with_arguments(
                    json!({
                        "project": {
                            "path": project_dir.path().display().to_string()
                        },
                        "query": "helper"
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;

        let structured = find_result.structured_content.unwrap();
        let interfaces = structured["interfaces"].as_array().unwrap();
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0]["name"], "helper");

        Ok(())
    }

    #[tokio::test]
    async fn test_scene_context_alias_tools_force_modes() -> anyhow::Result<()> {
        let project_dir = TempDir::new()?;
        write_task_context_project(project_dir.path());
        let client = start_test_server(project_dir.path()).await?;

        client
            .call_tool(
                CallToolRequestParams::new("scan_project").with_arguments(
                    json!({
                        "project": {
                            "path": project_dir.path().display().to_string()
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;

        let impact = client
            .call_tool(
                CallToolRequestParams::new("analyze_change_impact").with_arguments(
                    json!({
                        "project": {
                            "path": project_dir.path().display().to_string()
                        },
                        "question": "改 helper 会影响谁？",
                        "anchor_symbols": ["helper"],
                        "allow_source_snippets": false
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;
        assert_eq!(
            impact.structured_content.as_ref().unwrap()["scene"],
            "impact"
        );

        let behavior = client
            .call_tool(
                CallToolRequestParams::new("analyze_behavior_context").with_arguments(
                    json!({
                        "project": {
                            "path": project_dir.path().display().to_string()
                        },
                        "question": "为什么这里失败会升级？",
                        "runtime": {
                            "stacktrace_symbols": ["helper"],
                            "failing_test": "tests::failure_path"
                        },
                        "allow_source_snippets": false
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;
        assert_eq!(
            behavior.structured_content.as_ref().unwrap()["scene"],
            "behavior"
        );

        let locate = client
            .call_tool(
                CallToolRequestParams::new("locate_relevant_code").with_arguments(
                    json!({
                        "project": {
                            "path": project_dir.path().display().to_string()
                        },
                        "question": "先看哪里最相关？",
                        "anchor_files": ["src/lib.rs"],
                        "allow_source_snippets": false
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;
        assert_eq!(
            locate.structured_content.as_ref().unwrap()["scene"],
            "locate"
        );

        let workflow_dir = TempDir::new()?;
        write_workflow_task_context_project(workflow_dir.path());
        let workflow_client = start_test_server(workflow_dir.path()).await?;

        workflow_client
            .call_tool(
                CallToolRequestParams::new("scan_project").with_arguments(
                    json!({
                        "project": {
                            "path": workflow_dir.path().display().to_string()
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;

        let workflow = workflow_client
            .call_tool(
                CallToolRequestParams::new("analyze_workflow_context").with_arguments(
                    json!({
                        "project": {
                            "path": workflow_dir.path().display().to_string()
                        },
                        "question": "一个 execution 在审批通过后，为什么仍然可能继续停留在 `Queued`，而不是直接进入 `Running`？请解释真正的状态流转和调度原因。",
                        "allow_source_snippets": false
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;
        assert_eq!(
            workflow.structured_content.as_ref().unwrap()["scene"],
            "workflow"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_mcp_resources_expose_project_status_and_interfaces() -> anyhow::Result<()> {
        let project_dir = TempDir::new()?;
        write_sample_project(project_dir.path());
        let client = start_test_server(project_dir.path()).await?;

        client
            .call_tool(
                CallToolRequestParams::new("scan_project").with_arguments(
                    json!({
                        "project": {
                            "path": project_dir.path().display().to_string()
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;

        let resources = client.list_all_resources().await?;
        assert!(resources
            .iter()
            .any(|resource| resource.uri == "quickdep://projects"));

        let projects = client
            .read_resource(ReadResourceRequestParams::new("quickdep://projects"))
            .await?;
        let projects_text = match &projects.contents[0] {
            ResourceContents::TextResourceContents { text, .. } => text,
            _ => panic!("expected text resource"),
        };
        let projects_json: Value = serde_json::from_str(projects_text)?;
        let project_id = projects_json["projects"][0]["id"]
            .as_str()
            .unwrap()
            .to_string();

        let status_uri = format!("quickdep://project/{}/status", project_id);
        let status = client
            .read_resource(ReadResourceRequestParams::new(status_uri))
            .await?;
        let status_text = match &status.contents[0] {
            ResourceContents::TextResourceContents { text, .. } => text,
            _ => panic!("expected text resource"),
        };
        let status_json: Value = serde_json::from_str(status_text)?;
        assert_eq!(status_json["project"]["state"]["Loaded"]["watching"], true);

        let interfaces_uri = format!("quickdep://project/{}/interfaces", project_id);
        let interfaces = client
            .read_resource(ReadResourceRequestParams::new(interfaces_uri))
            .await?;
        let interfaces_text = match &interfaces.contents[0] {
            ResourceContents::TextResourceContents { text, .. } => text,
            _ => panic!("expected text resource"),
        };
        let interfaces_json: Value = serde_json::from_str(interfaces_text)?;
        assert_eq!(interfaces_json["count"], 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_get_task_context_returns_impact_package() -> anyhow::Result<()> {
        let project_dir = TempDir::new()?;
        write_task_context_project(project_dir.path());
        let server = QuickDepServer::from_workspace(project_dir.path()).await?;

        let value = server
            .get_task_context_value(TaskContextRequest {
                project: ProjectTarget::default(),
                question: Some("改 helper 会影响谁？".to_string()),
                anchor_symbols: vec!["helper".to_string()],
                anchor_files: Vec::new(),
                mode: None,
                budget: Some("normal".to_string()),
                allow_source_snippets: Some(false),
                max_expansions: None,
                workspace: None,
                runtime: None,
                conversation: None,
            })
            .await
            .map_err(|error| anyhow!(error.message.clone()))?;

        assert_eq!(value["scene"], "impact");
        assert_eq!(value["status"], "ready");
        assert_eq!(
            value["package"]["target"]["qualified_name"],
            "src/lib.rs::helper"
        );
        assert_eq!(value["package"]["risk_summary"]["risk"], "low");

        let primary_symbols = value["package"]["primary_symbols"].as_array().unwrap();
        let primary_names = primary_symbols
            .iter()
            .map(|item| item["qualified_name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(primary_names.contains(&"src/lib.rs::helper"));
        assert!(primary_names.contains(&"src/lib.rs::entry"));
        assert!(primary_names.contains(&"src/helpers.rs::store"));

        let related_files = value["package"]["related_files"].as_array().unwrap();
        assert_eq!(related_files[0]["file_path"], "src/lib.rs");
        assert!(related_files
            .iter()
            .any(|item| item["file_path"] == "src/callers.rs"));
        assert!(related_files
            .iter()
            .any(|item| item["file_path"] == "src/helpers.rs"));

        Ok(())
    }

    #[tokio::test]
    async fn test_get_task_context_uses_workspace_selection_for_auto_impact() -> anyhow::Result<()>
    {
        let project_dir = TempDir::new()?;
        write_task_context_project(project_dir.path());
        let server = QuickDepServer::from_workspace(project_dir.path()).await?;

        let value = server
            .get_task_context_value(TaskContextRequest {
                project: ProjectTarget::default(),
                question: Some("这个改起来风险大吗？".to_string()),
                anchor_symbols: Vec::new(),
                anchor_files: Vec::new(),
                mode: None,
                budget: None,
                allow_source_snippets: Some(false),
                max_expansions: None,
                workspace: Some(TaskContextWorkspace {
                    active_file: Some("src/lib.rs".to_string()),
                    selection_symbol: Some("helper".to_string()),
                    selection_line: Some(8),
                    recent_files: Vec::new(),
                }),
                runtime: None,
                conversation: None,
            })
            .await
            .map_err(|error| anyhow!(error.message.clone()))?;

        assert_eq!(value["scene"], "impact");
        assert_eq!(value["status"], "ready");
        assert_eq!(
            value["resolved_anchors"]["symbols"][0]["qualified_name"],
            "src/lib.rs::helper"
        );
        assert!(value["evidence"]["anchor_sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source == "workspace.selection_symbol"));

        Ok(())
    }

    #[tokio::test]
    async fn test_get_task_context_marks_behavior_as_needing_code_read() -> anyhow::Result<()> {
        let project_dir = TempDir::new()?;
        write_task_context_project(project_dir.path());
        let server = QuickDepServer::from_workspace(project_dir.path()).await?;

        let value = server
            .get_task_context_value(TaskContextRequest {
                project: ProjectTarget::default(),
                question: Some("为什么这里失败会升级？".to_string()),
                anchor_symbols: Vec::new(),
                anchor_files: Vec::new(),
                mode: None,
                budget: Some("lean".to_string()),
                allow_source_snippets: Some(true),
                max_expansions: None,
                workspace: None,
                runtime: Some(TaskContextRuntime {
                    stacktrace_symbols: vec!["helper".to_string()],
                    failing_test: Some("tests::failure_path".to_string()),
                }),
                conversation: None,
            })
            .await
            .map_err(|error| anyhow!(error.message.clone()))?;

        assert_eq!(value["scene"], "behavior");
        assert_eq!(value["status"], "needs_code_read");
        assert_eq!(value["fallback_to_code"], true);
        assert_eq!(value["budget"]["requested"], "lean");
        assert_eq!(value["budget"]["applied"], "normal");
        assert!(value["package"]["source_snippets"]
            .as_array()
            .is_some_and(|items| !items.is_empty()));

        Ok(())
    }

    #[test]
    fn test_select_workflow_phase_symbols_prefers_internal_runtime_chain() {
        let candidates = vec![
            workflow_candidate(
                "crates/ark-control-plane/src/lib.rs::ControlPlane::approval_resolve",
                "crates/ark-control-plane/src/lib.rs",
                0,
            ),
            workflow_candidate(
                "crates/ark-cli/src/lib.rs::Cli::approval_resolve",
                "crates/ark-cli/src/lib.rs",
                0,
            ),
            workflow_candidate(
                "crates/ark-store/src/write.rs::Store::approve_pending_approval",
                "crates/ark-store/src/write.rs",
                0,
            ),
            workflow_candidate(
                "crates/ark-runtime/src/lib.rs::Runtime::approval_resolve",
                "crates/ark-runtime/src/lib.rs",
                1,
            ),
            workflow_candidate(
                "crates/ark-runtime/src/core_flow_service.rs::RuntimeFlowService::resume_approved_execution",
                "crates/ark-runtime/src/core_flow_service.rs",
                1,
            ),
            workflow_candidate(
                "crates/ark-runtime/src/flow.rs::RuntimeCore::dispatch_execution",
                "crates/ark-runtime/src/flow.rs",
                1,
            ),
            workflow_candidate(
                "crates/ark-runtime/src/flow.rs::RuntimeCore::prepare_execution_dispatch",
                "crates/ark-runtime/src/flow.rs",
                0,
            ),
            workflow_candidate(
                "crates/ark-execution/src/lib.rs::ExecutionService::queue_for_conflict",
                "crates/ark-execution/src/lib.rs",
                1,
            ),
            workflow_candidate(
                "crates/ark-execution/src/lib.rs::ExecutionService::next_conflict_queue_head",
                "crates/ark-execution/src/lib.rs",
                2,
            ),
            workflow_candidate(
                "crates/ark-scheduler/src/lib.rs::Scheduler::admit",
                "crates/ark-scheduler/src/lib.rs",
                1,
            ),
            workflow_candidate(
                "crates/ark-scheduler/src/lib.rs::Scheduler::dispatchable_head",
                "crates/ark-scheduler/src/lib.rs",
                2,
            ),
        ];

        let mut phases = QuickDepServer::select_workflow_phase_symbols(&candidates);
        QuickDepServer::attach_workflow_phase_supports(&candidates, &mut phases);

        let selected = phases
            .iter()
            .map(|selection| {
                (
                    selection.phase.key,
                    std::iter::once(selection.symbol.qualified_name.as_str())
                        .chain(
                            selection
                                .supporting
                                .iter()
                                .map(|support| support.symbol.qualified_name.as_str()),
                        )
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        let selected_symbols = selected
            .iter()
            .flat_map(|(_, symbols)| symbols.iter().copied())
            .collect::<Vec<_>>();

        assert!(!selected_symbols
            .iter()
            .any(|symbol| symbol.contains("ark-cli") || symbol.contains("ark-control-plane")));
        assert!(selected.iter().any(|(phase, symbols)| {
            *phase == "approval"
                && symbols
                    .contains(&"crates/ark-store/src/write.rs::Store::approve_pending_approval")
        }));
        assert!(selected.iter().any(|(phase, symbols)| {
            *phase == "resume"
                && symbols.contains(
                    &"crates/ark-runtime/src/core_flow_service.rs::RuntimeFlowService::resume_approved_execution",
                )
        }));
        assert!(selected.iter().any(|(phase, symbols)| {
            *phase == "dispatch"
                && symbols
                    .contains(&"crates/ark-runtime/src/flow.rs::RuntimeCore::dispatch_execution")
                && symbols.contains(
                    &"crates/ark-runtime/src/flow.rs::RuntimeCore::prepare_execution_dispatch",
                )
        }));
        assert!(selected.iter().any(|(phase, symbols)| {
            *phase == "queue"
                && symbols.contains(
                    &"crates/ark-execution/src/lib.rs::ExecutionService::next_conflict_queue_head",
                )
                && symbols.contains(
                    &"crates/ark-execution/src/lib.rs::ExecutionService::queue_for_conflict",
                )
        }));
        assert!(selected.iter().any(|(phase, symbols)| {
            *phase == "scheduler"
                && symbols.contains(&"crates/ark-scheduler/src/lib.rs::Scheduler::admit")
                && symbols
                    .contains(&"crates/ark-scheduler/src/lib.rs::Scheduler::dispatchable_head")
        }));
    }

    #[tokio::test]
    async fn test_get_task_context_builds_workflow_package_without_explicit_anchor(
    ) -> anyhow::Result<()> {
        let project_dir = TempDir::new()?;
        write_workflow_task_context_project(project_dir.path());
        let server = QuickDepServer::from_workspace(project_dir.path()).await?;

        let value = server
            .get_task_context_value(TaskContextRequest {
                project: ProjectTarget::default(),
                question: Some(
                    "一个 execution 在审批通过后，为什么仍然可能继续停留在 `Queued`，而不是直接进入 `Running`？请解释真正的状态流转和调度原因。"
                        .to_string(),
                ),
                anchor_symbols: Vec::new(),
                anchor_files: Vec::new(),
                mode: None,
                budget: Some("lean".to_string()),
                allow_source_snippets: Some(false),
                max_expansions: None,
                workspace: None,
                runtime: None,
                conversation: None,
            })
            .await
            .map_err(|error| anyhow!(error.message.clone()))?;

        assert_eq!(value["scene"], "workflow");
        assert_eq!(value["status"], "needs_code_read");
        assert_eq!(value["budget"]["requested"], "lean");
        assert_eq!(value["budget"]["applied"], "normal");
        assert_eq!(value["fallback_to_code"], true);
        assert!(value["evidence"]["anchor_sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source == "question.workflow_seed"));

        let phases = value["package"]["workflow_phases"].as_array().unwrap();
        assert!(phases.len() >= 4);
        let phase_names = phases
            .iter()
            .map(|item| item["phase"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(phase_names.contains(&"approval"));
        assert!(phase_names.contains(&"dispatch"));
        assert!(phase_names.contains(&"queue"));
        assert!(phase_names.contains(&"scheduler"));

        let primary_symbols = value["package"]["primary_symbols"].as_array().unwrap();
        let primary_names = primary_symbols
            .iter()
            .map(|item| item["qualified_name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(
            primary_names.contains(&"src/store.rs::approve_pending_approval")
                || primary_names.contains(&"src/runtime.rs::approval_resolve")
        );
        assert!(primary_names.contains(&"src/core_flow_service.rs::resume_approved_execution"));
        assert!(primary_names.contains(&"src/flow.rs::dispatch_execution"));
        assert!(primary_names.contains(&"src/flow.rs::prepare_execution_dispatch"));
        assert!(primary_names.contains(&"src/execution.rs::next_conflict_queue_head"));
        assert!(primary_names.contains(&"src/scheduler.rs::admit"));
        assert!(primary_names.contains(&"src/scheduler.rs::dispatchable_head"));

        let dispatch_phase = phases
            .iter()
            .find(|item| item["phase"] == "dispatch")
            .unwrap();
        let dispatch_bucket =
            std::iter::once(dispatch_phase["symbol"]["qualified_name"].as_str().unwrap())
                .chain(
                    dispatch_phase["supporting_symbols"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|item| item["symbol"]["qualified_name"].as_str()),
                )
                .collect::<Vec<_>>();
        assert!(dispatch_bucket.contains(&"src/flow.rs::dispatch_execution"));
        assert!(dispatch_bucket.contains(&"src/flow.rs::prepare_execution_dispatch"));
        let scheduler_phase = phases
            .iter()
            .find(|item| item["phase"] == "scheduler")
            .unwrap();
        let scheduler_bucket = std::iter::once(
            scheduler_phase["symbol"]["qualified_name"]
                .as_str()
                .unwrap(),
        )
        .chain(
            scheduler_phase["supporting_symbols"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|item| item["symbol"]["qualified_name"].as_str()),
        )
        .collect::<Vec<_>>();
        assert!(scheduler_bucket.contains(&"src/scheduler.rs::admit"));
        assert!(scheduler_bucket.contains(&"src/scheduler.rs::dispatchable_head"));

        let related_files = value["package"]["related_files"].as_array().unwrap();
        assert!(
            related_files
                .iter()
                .any(|item| item["file_path"] == "src/store.rs")
                || related_files
                    .iter()
                    .any(|item| item["file_path"] == "src/runtime.rs")
        );
        assert!(related_files
            .iter()
            .any(|item| item["file_path"] == "src/flow.rs"));
        assert!(related_files
            .iter()
            .any(|item| item["file_path"] == "src/scheduler.rs"));

        Ok(())
    }

    #[tokio::test]
    async fn test_get_task_context_prefers_active_file_for_workspace_selection_symbol(
    ) -> anyhow::Result<()> {
        let project_dir = TempDir::new()?;
        write_ambiguous_task_context_project(project_dir.path());
        let server = QuickDepServer::from_workspace(project_dir.path()).await?;

        let value = server
            .get_task_context_value(TaskContextRequest {
                project: ProjectTarget::default(),
                question: Some("这个 helper 改动风险大吗？".to_string()),
                anchor_symbols: Vec::new(),
                anchor_files: Vec::new(),
                mode: None,
                budget: None,
                allow_source_snippets: Some(false),
                max_expansions: None,
                workspace: Some(TaskContextWorkspace {
                    active_file: Some("src/helpers.rs".to_string()),
                    selection_symbol: Some("helper".to_string()),
                    selection_line: Some(2),
                    recent_files: Vec::new(),
                }),
                runtime: None,
                conversation: None,
            })
            .await
            .map_err(|error| anyhow!(error.message.clone()))?;

        assert_eq!(
            value["resolved_anchors"]["symbols"][0]["qualified_name"],
            "src/helpers.rs::helper"
        );
        assert!(value["evidence"]["anchor_sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source == "workspace.selection_symbol"));

        Ok(())
    }

    #[tokio::test]
    async fn test_get_task_context_returns_call_chain_when_two_anchors_are_supplied(
    ) -> anyhow::Result<()> {
        let project_dir = TempDir::new()?;
        write_task_context_project(project_dir.path());
        let server = QuickDepServer::from_workspace(project_dir.path()).await?;

        let value = server
            .get_task_context_value(TaskContextRequest {
                project: ProjectTarget::default(),
                question: Some("从 entry 到 store 的调用链是什么？".to_string()),
                anchor_symbols: vec!["entry".to_string(), "store".to_string()],
                anchor_files: Vec::new(),
                mode: Some("call_chain".to_string()),
                budget: Some("normal".to_string()),
                allow_source_snippets: Some(true),
                max_expansions: None,
                workspace: None,
                runtime: None,
                conversation: None,
            })
            .await
            .map_err(|error| anyhow!(error.message.clone()))?;

        assert_eq!(value["scene"], "call_chain");
        assert_eq!(value["status"], "ready");
        let symbols = value["package"]["primary_symbols"].as_array().unwrap();
        assert_eq!(symbols.len(), 3);
        assert_eq!(
            symbols.first().unwrap()["qualified_name"],
            "src/lib.rs::entry"
        );
        assert_eq!(
            symbols.last().unwrap()["qualified_name"],
            "src/helpers.rs::store"
        );
        assert_eq!(value["package"]["key_edges"].as_array().unwrap().len(), 2);

        Ok(())
    }

    #[tokio::test]
    async fn test_get_task_context_needs_anchor_for_ambiguous_question() -> anyhow::Result<()> {
        let project_dir = TempDir::new()?;
        write_task_context_project(project_dir.path());
        let server = QuickDepServer::from_workspace(project_dir.path()).await?;

        let value = server
            .get_task_context_value(TaskContextRequest {
                project: ProjectTarget::default(),
                question: Some("这个为什么会失败？".to_string()),
                anchor_symbols: Vec::new(),
                anchor_files: Vec::new(),
                mode: None,
                budget: None,
                allow_source_snippets: Some(false),
                max_expansions: None,
                workspace: None,
                runtime: None,
                conversation: None,
            })
            .await
            .map_err(|error| anyhow!(error.message.clone()))?;

        assert_eq!(value["status"], "needs_anchor");
        assert_eq!(value["coverage"], "minimal");
        assert!(value["evidence"]["penalties"]
            .as_array()
            .unwrap()
            .iter()
            .any(|penalty| penalty
                .as_str()
                .unwrap_or_default()
                .contains("question text")));

        Ok(())
    }

    #[tokio::test]
    async fn test_get_task_context_can_build_file_centered_locate_context() -> anyhow::Result<()> {
        let project_dir = TempDir::new()?;
        write_task_context_project(project_dir.path());
        let server = QuickDepServer::from_workspace(project_dir.path()).await?;

        let value = server
            .get_task_context_value(TaskContextRequest {
                project: ProjectTarget::default(),
                question: Some("先看这个文件里有哪些接口".to_string()),
                anchor_symbols: Vec::new(),
                anchor_files: vec!["src/helpers.rs".to_string()],
                mode: Some("locate".to_string()),
                budget: None,
                allow_source_snippets: Some(false),
                max_expansions: None,
                workspace: None,
                runtime: None,
                conversation: None,
            })
            .await
            .map_err(|error| anyhow!(error.message.clone()))?;

        assert_eq!(value["scene"], "locate");
        assert_eq!(value["status"], "ready");
        assert_eq!(value["package"]["target"]["kind"], "file");
        assert_eq!(value["package"]["target"]["file_path"], "src/helpers.rs");
        assert_eq!(
            value["package"]["primary_symbols"][0]["qualified_name"],
            "src/helpers.rs::store"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_file_watcher_refreshes_cached_queries_after_source_change() -> anyhow::Result<()>
    {
        let project_dir = TempDir::new()?;
        write_sample_project(project_dir.path());
        let server = QuickDepServer::from_workspace(project_dir.path()).await?;
        let project = server
            .ensure_project_loaded(&ProjectTarget::default())
            .await
            .map_err(|error| anyhow!(error.message.clone()))?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        let initial = server
            .find_interfaces_value(FindInterfacesRequest {
                project: ProjectTarget::default(),
                query: "added".to_string(),
                limit: Some(5),
            })
            .await
            .map_err(|error| anyhow!(error.message.clone()))?;
        assert!(initial["interfaces"]
            .as_array()
            .is_some_and(|interfaces| interfaces.is_empty()));

        std::fs::write(
            project_dir.path().join("src/lib.rs"),
            r#"
pub fn entry() {
    helper();
    added();
}

pub fn helper() {}

pub fn added() {}
"#,
        )?;

        assert!(wait_for_symbol(&server, &project, "added").await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_paused_watcher_reconciles_missed_events_on_resume() -> anyhow::Result<()> {
        let project_dir = TempDir::new()?;
        write_sample_project(project_dir.path());
        let server = QuickDepServer::from_workspace(project_dir.path()).await?;
        let project = server
            .ensure_project_loaded(&ProjectTarget::default())
            .await
            .map_err(|error| anyhow!(error.message.clone()))?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        server
            .manager
            .pause_watch(&project.id, "Idle timeout")
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        let mut paused_project = project.clone();
        paused_project.pause_watching("Idle timeout");
        server
            .ensure_project_watcher(&paused_project)
            .await
            .map_err(|error| anyhow!(error.message.clone()))?;

        std::fs::write(
            project_dir.path().join("src/lib.rs"),
            r#"
pub fn entry() {
    helper();
    resumed();
}

pub fn helper() {}

pub fn resumed() {}
"#,
        )?;
        tokio::time::sleep(Duration::from_millis(800)).await;

        let exists_while_paused = QuickDepServer::blocking_storage(project.database_path(), {
            let qualified_name = "src/lib.rs::resumed".to_string();
            move |storage| {
                storage
                    .get_symbol_by_qualified_name(&qualified_name)
                    .map(|symbol| symbol.is_some())
                    .map_err(|error| error.to_string())
            }
        })
        .await
        .map_err(|error| anyhow!(error.message.clone()))?;
        assert!(!exists_while_paused);

        let resumed = server
            .ensure_project_loaded(&ProjectTarget::default())
            .await
            .map_err(|error| anyhow!(error.message.clone()))?;
        assert!(wait_for_symbol(&server, &resumed, "resumed").await?);
        Ok(())
    }

    #[tokio::test]
    async fn test_concurrent_initial_project_loads_wait_for_inflight_scan() -> anyhow::Result<()> {
        let project_dir = TempDir::new()?;
        write_large_sample_project(project_dir.path(), 1200);
        let server = Arc::new(QuickDepServer::from_workspace(project_dir.path()).await?);
        let mut tasks = Vec::new();

        for _ in 0..12 {
            let server = server.clone();
            tasks.push(tokio::spawn(async move {
                server
                    .ensure_project_loaded(&ProjectTarget::default())
                    .await
            }));
        }

        for task in tasks {
            let project = task
                .await
                .map_err(|error| anyhow!(error.to_string()))?
                .map_err(|error| anyhow!(error.message.clone()))?;
            assert!(project.is_loaded());
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_dirty_watcher_refresh_skips_invalid_operation_without_clearing_dirty(
    ) -> anyhow::Result<()> {
        let dirty = AtomicBool::new(true);
        let cache = ProjectCacheState::default();
        cache.symbol_index_ready.store(true, Ordering::SeqCst);

        QuickDepServer::handle_dirty_watcher_refresh_result(
            &ProjectId::from_string("test-project"),
            &dirty,
            &cache,
            Err(ManagerError::InvalidOperation(
                "project test-project is already loading".to_string(),
            )),
        )
        .map_err(|error| anyhow!(error.message.clone()))?;

        assert!(dirty.load(Ordering::SeqCst));
        assert!(cache.symbol_index_ready.load(Ordering::SeqCst));

        Ok(())
    }

    #[tokio::test]
    async fn test_dirty_watcher_refresh_success_clears_dirty_and_cache() -> anyhow::Result<()> {
        let dirty = AtomicBool::new(true);
        let cache = ProjectCacheState::default();
        cache.symbol_index_ready.store(true, Ordering::SeqCst);

        QuickDepServer::handle_dirty_watcher_refresh_result(
            &ProjectId::from_string("test-project"),
            &dirty,
            &cache,
            Ok(()),
        )
        .map_err(|error| anyhow!(error.message.clone()))?;

        assert!(!dirty.load(Ordering::SeqCst));
        assert!(!cache.symbol_index_ready.load(Ordering::SeqCst));

        Ok(())
    }

    #[tokio::test]
    async fn test_mcp_tool_filter_limits_listed_and_batched_tools() -> anyhow::Result<()> {
        let project_dir = TempDir::new()?;
        write_sample_project(project_dir.path());
        let client = start_test_server_with_tools(
            project_dir.path(),
            Some(vec![
                "scan_project".to_string(),
                "find_interfaces".to_string(),
                "batch_query".to_string(),
            ]),
        )
        .await?;

        let tools = client.list_all_tools().await?;
        let tool_names = tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(
            tool_names,
            vec!["batch_query", "find_interfaces", "scan_project"]
        );

        client
            .call_tool(
                CallToolRequestParams::new("scan_project").with_arguments(
                    json!({
                        "project": {
                            "path": project_dir.path().display().to_string()
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;

        assert!(client
            .call_tool(
                CallToolRequestParams::new("get_dependencies").with_arguments(
                    json!({
                        "project": {
                            "path": project_dir.path().display().to_string()
                        },
                        "interface": "helper"
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await
            .is_err());

        let batch_result = client
            .call_tool(
                CallToolRequestParams::new("batch_query").with_arguments(
                    json!({
                        "project": {
                            "path": project_dir.path().display().to_string()
                        },
                        "queries": [
                            {
                                "kind": "find_interfaces",
                                "query": "helper"
                            },
                            {
                                "kind": "get_dependencies",
                                "interface": "helper"
                            }
                        ]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
            )
            .await?;

        let structured = batch_result.structured_content.unwrap();
        let results = structured["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["ok"], true);
        assert_eq!(results[1]["ok"], false);
        assert!(results[1]["error"]
            .as_str()
            .unwrap()
            .contains("disabled by server configuration"));

        Ok(())
    }

    #[tokio::test]
    async fn test_unknown_tool_filter_is_rejected() {
        let project_dir = TempDir::new().unwrap();
        write_sample_project(project_dir.path());

        let error =
            QuickDepServer::from_workspace_with_tools(project_dir.path(), vec!["nope".into()])
                .await
                .unwrap_err();
        assert!(error.to_string().contains("unknown tools: nope"));
    }
}
