//! REST API handlers that mirror MCP tool operations.

use axum::{
    extract::{Json as AxumJson, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use rmcp::{handler::server::wrapper::Parameters, model::ErrorCode, ErrorData as McpError, Json};
use serde_json::{json, Value};

use crate::mcp::{
    BatchQueryRequest, CallChainRequest, DependenciesRequest, FileInterfacesRequest,
    FindInterfacesRequest, InterfaceLookupRequest, ProjectStatusRequest, QuickDepServer,
    ScanProjectRequest, TaskContextRequest,
};

type HttpResult = Result<AxumJson<Value>, Response>;

fn disabled_tool_response(server: &QuickDepServer, tool_name: &str) -> Option<Response> {
    if server.is_tool_enabled(tool_name) {
        return None;
    }

    Some(http_error(McpError::new(
        ErrorCode::METHOD_NOT_FOUND,
        format!("tool '{}' is disabled by server configuration", tool_name),
        None,
    )))
}

/// Build the REST API router mounted under `/api`.
pub fn router() -> axum::Router<QuickDepServer> {
    use axum::routing::{get, post};

    axum::Router::new()
        .route("/projects", get(list_projects))
        .route("/projects/scan", post(scan_project))
        .route("/projects/status", post(get_scan_status))
        .route("/projects/cancel", post(cancel_scan))
        .route("/projects/rebuild", post(rebuild_database))
        .route("/interfaces/search", post(find_interfaces))
        .route("/interfaces/detail", post(get_interface))
        .route("/dependencies", post(get_dependencies))
        .route("/call-chain", post(get_call_chain))
        .route("/files/interfaces", post(get_file_interfaces))
        .route("/task-context", post(get_task_context))
        .route("/query/batch", post(batch_query))
}

/// `GET /api/projects`
pub async fn list_projects(State(server): State<QuickDepServer>) -> HttpResult {
    if let Some(response) = disabled_tool_response(&server, "list_projects") {
        return Err(response);
    }
    match server.list_projects().await {
        Ok(Json(value)) => Ok(AxumJson(value)),
        Err(error) => Err(http_error(error)),
    }
}

/// `POST /api/projects/scan`
pub async fn scan_project(
    State(server): State<QuickDepServer>,
    AxumJson(request): AxumJson<ScanProjectRequest>,
) -> HttpResult {
    if let Some(response) = disabled_tool_response(&server, "scan_project") {
        return Err(response);
    }
    match server.scan_project(Parameters(request)).await {
        Ok(Json(value)) => Ok(AxumJson(value)),
        Err(error) => Err(http_error(error)),
    }
}

/// `POST /api/projects/status`
pub async fn get_scan_status(
    State(server): State<QuickDepServer>,
    AxumJson(request): AxumJson<ProjectStatusRequest>,
) -> HttpResult {
    if let Some(response) = disabled_tool_response(&server, "get_scan_status") {
        return Err(response);
    }
    match server.get_scan_status(Parameters(request)).await {
        Ok(Json(value)) => Ok(AxumJson(value)),
        Err(error) => Err(http_error(error)),
    }
}

/// `POST /api/projects/cancel`
pub async fn cancel_scan(
    State(server): State<QuickDepServer>,
    AxumJson(request): AxumJson<ProjectStatusRequest>,
) -> HttpResult {
    if let Some(response) = disabled_tool_response(&server, "cancel_scan") {
        return Err(response);
    }
    match server.cancel_scan(Parameters(request)).await {
        Ok(Json(value)) => Ok(AxumJson(value)),
        Err(error) => Err(http_error(error)),
    }
}

/// `POST /api/projects/rebuild`
pub async fn rebuild_database(
    State(server): State<QuickDepServer>,
    AxumJson(request): AxumJson<ProjectStatusRequest>,
) -> HttpResult {
    if let Some(response) = disabled_tool_response(&server, "rebuild_database") {
        return Err(response);
    }
    match server.rebuild_database(Parameters(request)).await {
        Ok(Json(value)) => Ok(AxumJson(value)),
        Err(error) => Err(http_error(error)),
    }
}

/// `POST /api/interfaces/search`
pub async fn find_interfaces(
    State(server): State<QuickDepServer>,
    AxumJson(request): AxumJson<FindInterfacesRequest>,
) -> HttpResult {
    if let Some(response) = disabled_tool_response(&server, "find_interfaces") {
        return Err(response);
    }
    match server.find_interfaces(Parameters(request)).await {
        Ok(Json(value)) => Ok(AxumJson(value)),
        Err(error) => Err(http_error(error)),
    }
}

/// `POST /api/interfaces/detail`
pub async fn get_interface(
    State(server): State<QuickDepServer>,
    AxumJson(request): AxumJson<InterfaceLookupRequest>,
) -> HttpResult {
    if let Some(response) = disabled_tool_response(&server, "get_interface") {
        return Err(response);
    }
    match server.get_interface(Parameters(request)).await {
        Ok(Json(value)) => Ok(AxumJson(value)),
        Err(error) => Err(http_error(error)),
    }
}

/// `POST /api/dependencies`
pub async fn get_dependencies(
    State(server): State<QuickDepServer>,
    AxumJson(request): AxumJson<DependenciesRequest>,
) -> HttpResult {
    if let Some(response) = disabled_tool_response(&server, "get_dependencies") {
        return Err(response);
    }
    match server.get_dependencies(Parameters(request)).await {
        Ok(Json(value)) => Ok(AxumJson(value)),
        Err(error) => Err(http_error(error)),
    }
}

/// `POST /api/call-chain`
pub async fn get_call_chain(
    State(server): State<QuickDepServer>,
    AxumJson(request): AxumJson<CallChainRequest>,
) -> HttpResult {
    if let Some(response) = disabled_tool_response(&server, "get_call_chain") {
        return Err(response);
    }
    match server.get_call_chain(Parameters(request)).await {
        Ok(Json(value)) => Ok(AxumJson(value)),
        Err(error) => Err(http_error(error)),
    }
}

/// `POST /api/files/interfaces`
pub async fn get_file_interfaces(
    State(server): State<QuickDepServer>,
    AxumJson(request): AxumJson<FileInterfacesRequest>,
) -> HttpResult {
    if let Some(response) = disabled_tool_response(&server, "get_file_interfaces") {
        return Err(response);
    }
    match server.get_file_interfaces(Parameters(request)).await {
        Ok(Json(value)) => Ok(AxumJson(value)),
        Err(error) => Err(http_error(error)),
    }
}

/// `POST /api/task-context`
pub async fn get_task_context(
    State(server): State<QuickDepServer>,
    AxumJson(request): AxumJson<TaskContextRequest>,
) -> HttpResult {
    if let Some(response) = disabled_tool_response(&server, "get_task_context") {
        return Err(response);
    }
    match server.get_task_context(Parameters(request)).await {
        Ok(Json(value)) => Ok(AxumJson(value)),
        Err(error) => Err(http_error(error)),
    }
}

/// `POST /api/query/batch`
pub async fn batch_query(
    State(server): State<QuickDepServer>,
    AxumJson(request): AxumJson<BatchQueryRequest>,
) -> HttpResult {
    if let Some(response) = disabled_tool_response(&server, "batch_query") {
        return Err(response);
    }
    match server.batch_query(Parameters(request)).await {
        Ok(Json(value)) => Ok(AxumJson(value)),
        Err(error) => Err(http_error(error)),
    }
}

/// Convert MCP errors into HTTP JSON responses.
pub fn http_error(error: McpError) -> Response {
    let status = match error.code {
        code if code == ErrorCode::RESOURCE_NOT_FOUND || code == ErrorCode::METHOD_NOT_FOUND => {
            StatusCode::NOT_FOUND
        }
        code if code == ErrorCode::INVALID_PARAMS
            || code == ErrorCode::INVALID_REQUEST
            || code == ErrorCode::PARSE_ERROR =>
        {
            StatusCode::BAD_REQUEST
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };

    (
        status,
        AxumJson(json!({
            "error": {
                "code": error.code.0,
                "message": error.message,
                "data": error.data,
            }
        })),
    )
        .into_response()
}
