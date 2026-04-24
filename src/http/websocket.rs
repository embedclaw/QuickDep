//! WebSocket handlers for streaming project status updates.

use std::time::Duration;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::Response,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::api::http_error;
use crate::mcp::{ProjectStatusRequest, ProjectTarget, QuickDepServer};

const DEFAULT_INTERVAL_MS: u64 = 1_000;
const MIN_INTERVAL_MS: u64 = 100;
const MAX_INTERVAL_MS: u64 = 30_000;

/// Query parameters accepted by `/ws/projects`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProjectStatusSubscription {
    /// Registered project ID to observe.
    #[serde(default)]
    pub project_id: Option<String>,
    /// Project path to observe.
    #[serde(default)]
    pub path: Option<String>,
    /// Poll interval in milliseconds.
    #[serde(default)]
    pub interval_ms: Option<u64>,
}

/// Build the WebSocket router.
pub fn router() -> axum::Router<QuickDepServer> {
    use axum::routing::get;

    axum::Router::new().route("/ws/projects", get(project_status_ws))
}

/// `GET /ws/projects`
pub async fn project_status_ws(
    ws: WebSocketUpgrade,
    State(server): State<QuickDepServer>,
    Query(query): Query<ProjectStatusSubscription>,
) -> Result<Response, Response> {
    let request = ProjectStatusRequest {
        project: ProjectTarget {
            project_id: query.project_id.clone(),
            path: query.path.clone(),
        },
    };
    let interval_ms = query
        .interval_ms
        .unwrap_or(DEFAULT_INTERVAL_MS)
        .clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS);

    let Json(value) = server
        .get_scan_status(Parameters(request.clone()))
        .await
        .map_err(http_error)?;

    Ok(ws.on_upgrade(move |socket| {
        stream_project_status(
            socket,
            server,
            request,
            Duration::from_millis(interval_ms),
            value,
        )
    }))
}

async fn stream_project_status(
    mut socket: WebSocket,
    server: QuickDepServer,
    request: ProjectStatusRequest,
    interval: Duration,
    initial_value: Value,
) {
    let mut last_status = initial_value;
    if send_status(&mut socket, &last_status).await.is_err() {
        return;
    }

    let mut ticker = tokio::time::interval(interval);
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                match server.get_scan_status(Parameters(request.clone())).await {
                    Ok(rmcp::Json(next_status)) => {
                        if next_status != last_status {
                            if send_status(&mut socket, &next_status).await.is_err() {
                                break;
                            }
                            last_status = next_status;
                        }
                    }
                    Err(error) => {
                        let payload = json!({
                            "type": "error",
                            "error": {
                                "code": error.code.0,
                                "message": error.message,
                                "data": error.data,
                            }
                        });
                        let _ = socket.send(Message::Text(payload.to_string().into())).await;
                        break;
                    }
                }
            }
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Text(text))) if text == "refresh" => {
                        match server.get_scan_status(Parameters(request.clone())).await {
                            Ok(rmcp::Json(next_status)) => {
                                if send_status(&mut socket, &next_status).await.is_err() {
                                    break;
                                }
                                last_status = next_status;
                            }
                            Err(error) => {
                                let payload = json!({
                                    "type": "error",
                                    "error": {
                                        "code": error.code.0,
                                        "message": error.message,
                                        "data": error.data,
                                    }
                                });
                                let _ = socket.send(Message::Text(payload.to_string().into())).await;
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

async fn send_status(socket: &mut WebSocket, status: &Value) -> Result<(), ()> {
    let payload = json!({
        "type": "status",
        "data": status,
    });
    socket
        .send(Message::Text(payload.to_string().into()))
        .await
        .map_err(|_| ())
}
