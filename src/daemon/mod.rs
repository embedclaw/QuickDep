//! Local daemon runtime for sharing QuickDep state across frontends.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{watch, Mutex};
use tracing::{info, warn};

use crate::{
    http,
    mcp::{
        BatchQueryRequest, CallChainRequest, DependenciesRequest, FileInterfacesRequest,
        FindInterfacesRequest, InterfaceLookupRequest, ProjectOverviewRequest,
        ProjectStatusRequest, QuickDepServer, ScanProjectRequest, TaskContextRequest,
    },
    project::ProjectManager,
    runtime::QuickDepRuntime,
};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DaemonMetadata {
    pid: u32,
    endpoint: String,
    started_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DaemonRequest {
    Ping,
    Stop,
    Status,
    CliScan {
        path: String,
        rebuild: bool,
    },
    CliStatus {
        path: String,
    },
    CliDebug {
        path: String,
        stats: bool,
        deps: Option<String>,
        file: Option<String>,
    },
    ToolCall {
        workspace_root: String,
        tool: String,
        arguments: Value,
        allowed_tools: Option<Vec<String>>,
    },
    EnsureHttp {
        workspace_root: String,
        port: u16,
        allowed_tools: Option<Vec<String>>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct DaemonEnvelope {
    ok: bool,
    value: Option<Value>,
    error: Option<String>,
}

impl DaemonEnvelope {
    fn ok(value: Value) -> Self {
        Self {
            ok: true,
            value: Some(value),
            error: None,
        }
    }

    fn err(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            value: None,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WorkspaceKey {
    root: PathBuf,
    tools_signature: Option<String>,
}

impl WorkspaceKey {
    fn new(root: PathBuf, allowed_tools: Option<&[String]>) -> Self {
        Self {
            root,
            tools_signature: allowed_tools.map(sorted_tools_signature),
        }
    }
}

#[derive(Debug)]
struct HttpHandle {
    port: u16,
    _task: tokio::task::JoinHandle<anyhow::Result<()>>,
}

#[derive(Debug, Clone)]
pub struct DaemonClient {
    endpoint: String,
}

impl DaemonClient {
    pub async fn connect() -> anyhow::Result<Self> {
        let metadata = read_metadata().await?;
        let client = Self {
            endpoint: metadata.endpoint,
        };
        client.send_once(&DaemonRequest::Ping).await?;
        Ok(client)
    }

    pub async fn connect_or_start() -> anyhow::Result<Self> {
        if let Ok(client) = Self::connect().await {
            return Ok(client);
        }

        start_background_process()?;
        let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
        loop {
            match Self::connect().await {
                Ok(client) => return Ok(client),
                Err(error) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(CONNECT_RETRY_DELAY).await;
                    if error.to_string().contains("No such file") {
                        continue;
                    }
                }
                Err(error) => {
                    return Err(error.context("daemon did not become ready in time"));
                }
            }
        }
    }

    pub async fn ping(&self) -> anyhow::Result<Value> {
        self.send(DaemonRequest::Ping).await
    }

    pub async fn stop(&self) -> anyhow::Result<Value> {
        self.send(DaemonRequest::Stop).await
    }

    pub async fn status(&self) -> anyhow::Result<Value> {
        self.send(DaemonRequest::Status).await
    }

    pub async fn cli_scan(&self, path: &Path, rebuild: bool) -> anyhow::Result<Value> {
        self.send(DaemonRequest::CliScan {
            path: path.display().to_string(),
            rebuild,
        })
        .await
    }

    pub async fn cli_status(&self, path: &Path) -> anyhow::Result<Value> {
        self.send(DaemonRequest::CliStatus {
            path: path.display().to_string(),
        })
        .await
    }

    pub async fn cli_debug(
        &self,
        path: &Path,
        stats: bool,
        deps: Option<&str>,
        file: Option<&str>,
    ) -> anyhow::Result<Value> {
        self.send(DaemonRequest::CliDebug {
            path: path.display().to_string(),
            stats,
            deps: deps.map(ToOwned::to_owned),
            file: file.map(ToOwned::to_owned),
        })
        .await
    }

    pub async fn invoke_tool<T: Serialize>(
        &self,
        workspace_root: &Path,
        tool: &str,
        arguments: &T,
        allowed_tools: Option<Vec<String>>,
    ) -> anyhow::Result<Value> {
        self.send(DaemonRequest::ToolCall {
            workspace_root: workspace_root.display().to_string(),
            tool: tool.to_string(),
            arguments: serde_json::to_value(arguments)?,
            allowed_tools,
        })
        .await
    }

    pub async fn ensure_http(
        &self,
        workspace_root: &Path,
        port: u16,
        allowed_tools: Option<Vec<String>>,
    ) -> anyhow::Result<Value> {
        self.send(DaemonRequest::EnsureHttp {
            workspace_root: workspace_root.display().to_string(),
            port,
            allowed_tools,
        })
        .await
    }

    async fn send(&self, request: DaemonRequest) -> anyhow::Result<Value> {
        match self.send_once(&request).await {
            Ok(value) => Ok(value),
            Err(error) if is_retryable_daemon_error(&error) => {
                let client = Self::reconnect_after_failure().await?;
                client.send_once(&request).await
            }
            Err(error) => Err(error),
        }
    }

    async fn send_once(&self, request: &DaemonRequest) -> anyhow::Result<Value> {
        let envelope = transport::send(&self.endpoint, request).await?;
        if envelope.ok {
            Ok(envelope.value.unwrap_or(Value::Null))
        } else {
            Err(anyhow!(
                envelope
                    .error
                    .unwrap_or_else(|| "daemon request failed".to_string())
            ))
        }
    }

    async fn reconnect_after_failure() -> anyhow::Result<Self> {
        if let Ok(client) = Self::connect().await {
            return Ok(client);
        }
        start_background_process()?;
        let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
        loop {
            match Self::connect().await {
                Ok(client) => return Ok(client),
                Err(error) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(CONNECT_RETRY_DELAY).await;
                    if is_retryable_daemon_error(&error) {
                        continue;
                    }
                }
                Err(error) => {
                    return Err(error.context("daemon did not recover in time"));
                }
            }
        }
    }
}

pub async fn run_foreground() -> anyhow::Result<()> {
    let state_dir = daemon_state_dir()?;
    std::fs::create_dir_all(&state_dir)
        .with_context(|| format!("failed to create daemon state dir {}", state_dir.display()))?;

    let endpoint = transport::endpoint()?;
    if let Err(error) = cleanup_stale_state(&endpoint).await {
        warn!(error = %error, "failed to clean stale daemon state");
    }

    let _lock = acquire_lock(&endpoint)?;
    transport::remove_endpoint_if_exists(&endpoint).await?;

    let manifest_path = state_dir.join("manifest.json");
    let manager = Arc::new(ProjectManager::with_scanner(&manifest_path).await);
    let runtime = QuickDepRuntime::new(manager.clone());
    let daemon = Arc::new(DaemonRuntime::new(manager, runtime));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    info!("QuickDep daemon listening on {}", endpoint);
    transport::serve(endpoint, daemon, shutdown_rx, shutdown_tx).await
}

pub async fn daemon_status() -> anyhow::Result<Value> {
    match DaemonClient::connect().await {
        Ok(client) => client.status().await,
        Err(error) if is_retryable_daemon_error(&error) => {
            clear_local_state().await?;
            Ok(json!({ "status": "stopped" }))
        }
        Err(error) => Err(error),
    }
}

pub async fn stop_daemon() -> anyhow::Result<Value> {
    match DaemonClient::connect().await {
        Ok(client) => client.stop().await,
        Err(error) if is_retryable_daemon_error(&error) => {
            clear_local_state().await?;
            Ok(json!({ "status": "stopped", "cleaned_stale_state": true }))
        }
        Err(error) => Err(error),
    }
}

struct DaemonRuntime {
    manager: Arc<ProjectManager>,
    cli_runtime: QuickDepRuntime,
    servers: Mutex<HashMap<WorkspaceKey, QuickDepServer>>,
    http_handles: Mutex<HashMap<WorkspaceKey, HttpHandle>>,
}

impl DaemonRuntime {
    fn new(manager: Arc<ProjectManager>, cli_runtime: QuickDepRuntime) -> Self {
        Self {
            manager,
            cli_runtime,
            servers: Mutex::new(HashMap::new()),
            http_handles: Mutex::new(HashMap::new()),
        }
    }

    async fn handle(&self, request: DaemonRequest) -> DaemonEnvelope {
        match self.handle_inner(request).await {
            Ok(value) => DaemonEnvelope::ok(value),
            Err(error) => DaemonEnvelope::err(error.to_string()),
        }
    }

    async fn handle_inner(&self, request: DaemonRequest) -> anyhow::Result<Value> {
        match request {
            DaemonRequest::Ping => Ok(json!({ "status": "ok" })),
            DaemonRequest::Status => {
                let managed_projects = self.manager.list_ids().await.len();
                let workspaces = self.servers.lock().await.len();
                let http_listeners = self.http_handles.lock().await.len();
                Ok(json!({
                    "status": "running",
                    "managed_projects": managed_projects,
                    "workspace_sessions": workspaces,
                    "http_listeners": http_listeners,
                    "state_dir": daemon_state_dir()?.display().to_string(),
                }))
            }
            DaemonRequest::Stop => Ok(json!({ "status": "stopping" })),
            DaemonRequest::CliScan { path, rebuild } => {
                self.cli_runtime
                    .scan_project(Path::new(&path), rebuild)
                    .await
            }
            DaemonRequest::CliStatus { path } => {
                self.cli_runtime.project_status(Path::new(&path)).await
            }
            DaemonRequest::CliDebug {
                path,
                stats,
                deps,
                file,
            } => {
                self.cli_runtime
                    .debug_project(
                        Path::new(&path),
                        stats,
                        deps.as_deref(),
                        file.as_deref(),
                    )
                    .await
            }
            DaemonRequest::ToolCall {
                workspace_root,
                tool,
                arguments,
                allowed_tools,
            } => {
                let server = self
                    .server_for_workspace(Path::new(&workspace_root), allowed_tools.as_deref())
                    .await?;
                dispatch_tool_call(&server, &tool, arguments).await
            }
            DaemonRequest::EnsureHttp {
                workspace_root,
                port,
                allowed_tools,
            } => {
                let root = PathBuf::from(&workspace_root);
                let server = self
                    .server_for_workspace(&root, allowed_tools.as_deref())
                    .await?;
                let key = WorkspaceKey::new(root.clone(), allowed_tools.as_deref());
                let mut handles = self.http_handles.lock().await;
                if let Some(handle) = handles.get(&key) {
                    if handle.port != port {
                        bail!(
                            "HTTP listener for {} already runs on port {}",
                            root.display(),
                            handle.port
                        );
                    }
                } else {
                    let task = http::spawn_http_server(server, port).await?;
                    handles.insert(key, HttpHandle { port, _task: task });
                }
                Ok(json!({
                    "status": "ready",
                    "url": format!("http://127.0.0.1:{port}/mcp"),
                    "port": port,
                    "workspace_root": workspace_root,
                }))
            }
        }
    }

    async fn server_for_workspace(
        &self,
        workspace_root: &Path,
        allowed_tools: Option<&[String]>,
    ) -> anyhow::Result<QuickDepServer> {
        let root = workspace_root.canonicalize().with_context(|| {
            format!(
                "failed to canonicalize workspace root {}",
                workspace_root.display()
            )
        })?;
        let key = WorkspaceKey::new(root.clone(), allowed_tools);

        if let Some(server) = self.servers.lock().await.get(&key).cloned() {
            return Ok(server);
        }

        let server = QuickDepServer::from_workspace_with_manager_and_tools(
            &root,
            self.manager.clone(),
            allowed_tools.map(|items| items.to_vec()),
        )
        .await?;

        let mut servers = self.servers.lock().await;
        Ok(servers.entry(key).or_insert_with(|| server.clone()).clone())
    }
}

async fn dispatch_tool_call(
    server: &QuickDepServer,
    tool: &str,
    arguments: Value,
) -> anyhow::Result<Value> {
    match tool {
        "list_projects" => Ok(server.list_projects().await?.0),
        "scan_project" => {
            let request = from_value::<ScanProjectRequest>(arguments)?;
            Ok(server.scan_project(rmcp::handler::server::wrapper::Parameters(request)).await?.0)
        }
        "get_scan_status" => {
            let request = from_value::<ProjectStatusRequest>(arguments)?;
            Ok(server
                .get_scan_status(rmcp::handler::server::wrapper::Parameters(request))
                .await?
                .0)
        }
        "get_project_overview" => {
            let request = from_value::<ProjectOverviewRequest>(arguments)?;
            Ok(server
                .get_project_overview(rmcp::handler::server::wrapper::Parameters(request))
                .await?
                .0)
        }
        "cancel_scan" => {
            let request = from_value::<ProjectStatusRequest>(arguments)?;
            Ok(server.cancel_scan(rmcp::handler::server::wrapper::Parameters(request)).await?.0)
        }
        "find_interfaces" => {
            let request = from_value::<FindInterfacesRequest>(arguments)?;
            Ok(server
                .find_interfaces(rmcp::handler::server::wrapper::Parameters(request))
                .await?
                .0)
        }
        "get_interface" => {
            let request = from_value::<InterfaceLookupRequest>(arguments)?;
            Ok(server
                .get_interface(rmcp::handler::server::wrapper::Parameters(request))
                .await?
                .0)
        }
        "get_dependencies" => {
            let request = from_value::<DependenciesRequest>(arguments)?;
            Ok(server
                .get_dependencies(rmcp::handler::server::wrapper::Parameters(request))
                .await?
                .0)
        }
        "get_call_chain" => {
            let request = from_value::<CallChainRequest>(arguments)?;
            Ok(server
                .get_call_chain(rmcp::handler::server::wrapper::Parameters(request))
                .await?
                .0)
        }
        "get_file_interfaces" => {
            let request = from_value::<FileInterfacesRequest>(arguments)?;
            Ok(server
                .get_file_interfaces(rmcp::handler::server::wrapper::Parameters(request))
                .await?
                .0)
        }
        "get_task_context" => {
            let request = from_value::<TaskContextRequest>(arguments)?;
            Ok(server
                .get_task_context(rmcp::handler::server::wrapper::Parameters(request))
                .await?
                .0)
        }
        "analyze_workflow_context" => {
            let request = from_value::<TaskContextRequest>(arguments)?;
            Ok(server
                .analyze_workflow_context(rmcp::handler::server::wrapper::Parameters(request))
                .await?
                .0)
        }
        "analyze_change_impact" => {
            let request = from_value::<TaskContextRequest>(arguments)?;
            Ok(server
                .analyze_change_impact(rmcp::handler::server::wrapper::Parameters(request))
                .await?
                .0)
        }
        "analyze_behavior_context" => {
            let request = from_value::<TaskContextRequest>(arguments)?;
            Ok(server
                .analyze_behavior_context(rmcp::handler::server::wrapper::Parameters(request))
                .await?
                .0)
        }
        "locate_relevant_code" => {
            let request = from_value::<TaskContextRequest>(arguments)?;
            Ok(server
                .locate_relevant_code(rmcp::handler::server::wrapper::Parameters(request))
                .await?
                .0)
        }
        "batch_query" => {
            let request = from_value::<BatchQueryRequest>(arguments)?;
            Ok(server.batch_query(rmcp::handler::server::wrapper::Parameters(request)).await?.0)
        }
        "rebuild_database" => {
            let request = from_value::<ProjectStatusRequest>(arguments)?;
            Ok(server
                .rebuild_database(rmcp::handler::server::wrapper::Parameters(request))
                .await?
                .0)
        }
        _ => bail!("unsupported daemon tool call '{}'", tool),
    }
}

fn from_value<T: DeserializeOwned>(value: Value) -> anyhow::Result<T> {
    serde_json::from_value(value).context("failed to decode daemon request arguments")
}

fn sorted_tools_signature(tools: &[String]) -> String {
    let mut values = tools
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values.join(",")
}

fn is_retryable_daemon_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        let message = cause.to_string();
        message.contains("No such file or directory")
            || message.contains("Connection refused")
            || message.contains("daemon closed response stream")
    })
}

fn daemon_state_dir() -> anyhow::Result<PathBuf> {
    let home = home_dir().ok_or_else(|| anyhow!("failed to determine home directory"))?;
    Ok(home.join(".quickdep").join("daemon"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

fn metadata_path() -> anyhow::Result<PathBuf> {
    Ok(daemon_state_dir()?.join("daemon.json"))
}

fn lock_path() -> anyhow::Result<PathBuf> {
    Ok(daemon_state_dir()?.join("daemon.lock"))
}

async fn read_metadata() -> anyhow::Result<DaemonMetadata> {
    let content = tokio::fs::read_to_string(metadata_path()?).await?;
    Ok(serde_json::from_str(&content)?)
}

fn write_metadata(metadata: &DaemonMetadata) -> anyhow::Result<()> {
    let path = metadata_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(metadata)?)?;
    Ok(())
}

fn remove_metadata() {
    if let Ok(path) = metadata_path() {
        let _ = std::fs::remove_file(path);
    }
}

async fn cleanup_stale_state(endpoint: &str) -> anyhow::Result<()> {
    if let Ok(client) = DaemonClient::connect().await {
        client.ping().await?;
        bail!("quickdep daemon is already running");
    }
    clear_local_state_for_endpoint(endpoint).await?;
    Ok(())
}

async fn clear_local_state() -> anyhow::Result<()> {
    let endpoint = transport::endpoint()?;
    clear_local_state_for_endpoint(&endpoint).await
}

async fn clear_local_state_for_endpoint(endpoint: &str) -> anyhow::Result<()> {
    transport::remove_endpoint_if_exists(endpoint).await?;
    if let Ok(path) = lock_path() {
        let _ = std::fs::remove_file(path);
    }
    remove_metadata();
    Ok(())
}

struct LockGuard {
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        remove_metadata();
    }
}

fn acquire_lock(endpoint: &str) -> anyhow::Result<LockGuard> {
    let path = lock_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let metadata = DaemonMetadata {
        pid: std::process::id(),
        endpoint: endpoint.to_string(),
        started_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    };
    let payload = serde_json::to_vec_pretty(&metadata)?;

    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(&payload)?;
            write_metadata(&metadata)?;
            Ok(LockGuard { path })
        }
        Err(error) => Err(anyhow!(error)).context("daemon lock already exists"),
    }
}

fn start_background_process() -> anyhow::Result<()> {
    let executable = daemon_executable()?;
    let mut command = std::process::Command::new(executable);
    command
        .arg("daemon")
        .arg("run")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    inherit_log_level(&mut command);
    command.spawn().context("failed to start quickdep daemon")?;
    Ok(())
}

fn daemon_executable() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_quickdep") {
        return Ok(PathBuf::from(path));
    }
    Ok(std::env::current_exe()?)
}

fn inherit_log_level(command: &mut std::process::Command) {
    if let Ok(level) = std::env::var("RUST_LOG") {
        command.env("RUST_LOG", level);
    }
}

#[cfg(unix)]
mod transport {
    use super::*;
    use tokio::net::{UnixListener, UnixStream};

    pub fn endpoint() -> anyhow::Result<String> {
        Ok(daemon_state_dir()?.join("daemon.sock").display().to_string())
    }

    pub async fn remove_endpoint_if_exists(endpoint: &str) -> anyhow::Result<()> {
        match tokio::fs::remove_file(endpoint).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn send(endpoint: &str, request: &DaemonRequest) -> anyhow::Result<DaemonEnvelope> {
        let stream = UnixStream::connect(endpoint)
            .await
            .with_context(|| format!("failed to connect to daemon socket {}", endpoint))?;
        exchange(stream, request).await
    }

    pub async fn serve(
        endpoint: String,
        daemon: Arc<DaemonRuntime>,
        mut shutdown_rx: watch::Receiver<bool>,
        shutdown_tx: watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        let listener = UnixListener::bind(&endpoint)
            .with_context(|| format!("failed to bind daemon socket {}", endpoint))?;

        loop {
            tokio::select! {
                changed = shutdown_rx.changed() => {
                    changed?;
                    if *shutdown_rx.borrow() {
                        break;
                    }
                }
                accepted = listener.accept() => {
                    let (stream, _) = accepted?;
                    let daemon = daemon.clone();
                    let shutdown_tx = shutdown_tx.clone();
                    tokio::spawn(async move {
                        if let Err(error) = handle_connection(stream, daemon, shutdown_tx).await {
                            warn!(error = %error, "daemon connection failed");
                        }
                    });
                }
            }
        }

        remove_endpoint_if_exists(&endpoint).await?;
        Ok(())
    }

    async fn handle_connection(
        stream: UnixStream,
        daemon: Arc<DaemonRuntime>,
        shutdown_tx: watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        let line = lines
            .next_line()
            .await?
            .ok_or_else(|| anyhow!("daemon client closed connection"))?;
        let request: DaemonRequest = serde_json::from_str(&line)?;
        let is_stop = matches!(request, DaemonRequest::Stop);
        let response = daemon.handle(request).await;
        writer
            .write_all(format!("{}\n", serde_json::to_string(&response)?).as_bytes())
            .await?;
        writer.flush().await?;
        if is_stop {
            let _ = shutdown_tx.send(true);
        }
        Ok(())
    }

    async fn exchange(stream: UnixStream, request: &DaemonRequest) -> anyhow::Result<DaemonEnvelope> {
        let (reader, mut writer) = stream.into_split();
        writer
            .write_all(format!("{}\n", serde_json::to_string(request)?).as_bytes())
            .await?;
        writer.flush().await?;
        let mut lines = BufReader::new(reader).lines();
        let line = lines
            .next_line()
            .await?
            .ok_or_else(|| anyhow!("daemon closed response stream"))?;
        Ok(serde_json::from_str(&line)?)
    }
}

#[cfg(windows)]
mod transport {
    use super::*;
    use tokio::net::{TcpListener, TcpStream};

    const WINDOWS_ADDR: &str = "127.0.0.1:41237";

    pub fn endpoint() -> anyhow::Result<String> {
        Ok(WINDOWS_ADDR.to_string())
    }

    pub async fn remove_endpoint_if_exists(_endpoint: &str) -> anyhow::Result<()> {
        Ok(())
    }

    pub async fn send(endpoint: &str, request: &DaemonRequest) -> anyhow::Result<DaemonEnvelope> {
        let stream = TcpStream::connect(endpoint)
            .await
            .with_context(|| format!("failed to connect to daemon TCP endpoint {}", endpoint))?;
        exchange(stream, request).await
    }

    pub async fn serve(
        endpoint: String,
        daemon: Arc<DaemonRuntime>,
        mut shutdown_rx: watch::Receiver<bool>,
        shutdown_tx: watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&endpoint)
            .await
            .with_context(|| format!("failed to bind daemon TCP endpoint {}", endpoint))?;

        loop {
            tokio::select! {
                changed = shutdown_rx.changed() => {
                    changed?;
                    if *shutdown_rx.borrow() {
                        break;
                    }
                }
                accepted = listener.accept() => {
                    let (stream, _) = accepted?;
                    let daemon = daemon.clone();
                    let shutdown_tx = shutdown_tx.clone();
                    tokio::spawn(async move {
                        if let Err(error) = handle_connection(stream, daemon, shutdown_tx).await {
                            warn!(error = %error, "daemon connection failed");
                        }
                    });
                }
            }
        }

        Ok(())
    }

    async fn handle_connection(
        stream: TcpStream,
        daemon: Arc<DaemonRuntime>,
        shutdown_tx: watch::Sender<bool>,
    ) -> anyhow::Result<()> {
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        let line = lines
            .next_line()
            .await?
            .ok_or_else(|| anyhow!("daemon client closed connection"))?;
        let request: DaemonRequest = serde_json::from_str(&line)?;
        let is_stop = matches!(request, DaemonRequest::Stop);
        let response = daemon.handle(request).await;
        writer
            .write_all(format!("{}\n", serde_json::to_string(&response)?).as_bytes())
            .await?;
        writer.flush().await?;
        if is_stop {
            let _ = shutdown_tx.send(true);
        }
        Ok(())
    }

    async fn exchange(stream: TcpStream, request: &DaemonRequest) -> anyhow::Result<DaemonEnvelope> {
        let (reader, mut writer) = stream.into_split();
        writer
            .write_all(format!("{}\n", serde_json::to_string(request)?).as_bytes())
            .await?;
        writer.flush().await?;
        let mut lines = BufReader::new(reader).lines();
        let line = lines
            .next_line()
            .await?
            .ok_or_else(|| anyhow!("daemon closed response stream"))?;
        Ok(serde_json::from_str(&line)?)
    }
}
