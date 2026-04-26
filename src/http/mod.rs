//! Streamable HTTP server support for QuickDep.

use std::net::Ipv4Addr;

use anyhow::Context;
use axum::{routing::get, Json, Router};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use serde_json::json;
use tokio::net::TcpListener;
use tracing::info;

use crate::mcp::QuickDepServer;

mod api;
mod cors;
mod websocket;

/// Spawn the QuickDep HTTP server in the background.
pub async fn spawn_http_server(
    server: QuickDepServer,
    port: u16,
) -> anyhow::Result<tokio::task::JoinHandle<anyhow::Result<()>>> {
    let listener = bind_listener(port).await?;
    Ok(tokio::spawn(async move {
        serve_http_listener(listener, server).await
    }))
}

/// Serve QuickDep over streamable HTTP on localhost.
pub async fn serve_http_server(server: QuickDepServer, port: u16) -> anyhow::Result<()> {
    let listener = bind_listener(port).await?;
    serve_http_listener(listener, server).await
}

fn build_router(server: QuickDepServer) -> Router {
    let app_state = server.clone();
    let service: StreamableHttpService<QuickDepServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(server.clone()),
            Default::default(),
            StreamableHttpServerConfig::default()
                .with_stateful_mode(false)
                .with_json_response(true),
        );

    Router::new()
        .route("/health", get(health))
        .nest("/api", api::router())
        .merge(websocket::router())
        .nest_service("/mcp", service)
        .layer(cors::cors_layer())
        .with_state(app_state)
}

async fn bind_listener(port: u16) -> anyhow::Result<TcpListener> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .await
        .with_context(|| format!("failed to bind HTTP listener on 127.0.0.1:{port}"))
}

async fn serve_http_listener(listener: TcpListener, server: QuickDepServer) -> anyhow::Result<()> {
    let address = listener
        .local_addr()
        .context("failed to read HTTP listener address")?;

    info!("QuickDep HTTP server listening on http://{address}/mcp");
    axum::serve(listener, build_router(server))
        .await
        .context("QuickDep HTTP server failed")?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{header, Method, Request, StatusCode},
    };
    use futures_util::StreamExt;
    use tempfile::TempDir;
    use tokio::net::TcpListener;
    use tokio_tungstenite::connect_async;
    use tower::ServiceExt;
    use tungstenite::Message;

    async fn sample_server() -> (TempDir, QuickDepServer) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        std::fs::create_dir_all(temp_dir.path().join("src")).expect("Failed to create src dir");
        std::fs::write(
            temp_dir.path().join("src/lib.rs"),
            "pub fn entry() { helper(); }\npub fn helper() {}\n",
        )
        .expect("Failed to write fixture source");
        let server = QuickDepServer::from_workspace(temp_dir.path())
            .await
            .expect("Failed to create server");
        (temp_dir, server)
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let (_temp_dir, server) = sample_server().await;
        let response = build_router(server)
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Request failed");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Failed to read body");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("Failed to parse health response");
        assert_eq!(payload, json!({ "status": "ok" }));
    }

    #[tokio::test]
    async fn test_rest_scan_and_search_endpoints() {
        let (_temp_dir, server) = sample_server().await;
        let router = build_router(server.clone());

        let scan_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/projects/scan")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .expect("Failed to build scan request"),
            )
            .await
            .expect("Scan request failed");
        assert_eq!(scan_response.status(), StatusCode::OK);

        let search_response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/interfaces/search")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"query":"helper","limit":5}"#))
                    .expect("Failed to build search request"),
            )
            .await
            .expect("Search request failed");
        assert_eq!(search_response.status(), StatusCode::OK);

        let body = to_bytes(search_response.into_body(), usize::MAX)
            .await
            .expect("Failed to read response body");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("Failed to parse search response");
        assert_eq!(payload["interfaces"][0]["name"], "helper");
    }

    #[tokio::test]
    async fn test_rest_project_overview_endpoint() {
        let (_temp_dir, server) = sample_server().await;
        let router = build_router(server.clone());

        let scan_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/projects/scan")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .expect("Failed to build scan request"),
            )
            .await
            .expect("Scan request failed");
        assert_eq!(scan_response.status(), StatusCode::OK);

        let overview_response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/projects/overview")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"max_symbols":10,"max_edges":10}"#))
                    .expect("Failed to build overview request"),
            )
            .await
            .expect("Overview request failed");
        assert_eq!(overview_response.status(), StatusCode::OK);

        let body = to_bytes(overview_response.into_body(), usize::MAX)
            .await
            .expect("Failed to read response body");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("Failed to parse overview response");
        assert_eq!(payload["overview"]["total_symbols"], 2);
        assert_eq!(payload["overview"]["displayed_symbols"], 2);
        assert_eq!(payload["overview"]["total_edges"], 1);
        assert_eq!(payload["overview"]["displayed_edges"], 1);

        let node_names = payload["overview"]["nodes"]
            .as_array()
            .expect("nodes should be an array")
            .iter()
            .map(|node| node["name"].as_str().expect("node name"))
            .collect::<Vec<_>>();
        assert!(node_names.contains(&"entry"));
        assert!(node_names.contains(&"helper"));

        let edge = &payload["overview"]["edges"][0];
        assert_eq!(edge["weight"], 1);
        assert_eq!(edge["kinds"][0], "call");
    }

    #[tokio::test]
    async fn test_rest_task_context_endpoint() {
        let (_temp_dir, server) = sample_server().await;
        let router = build_router(server.clone());

        let scan_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/projects/scan")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .expect("Failed to build scan request"),
            )
            .await
            .expect("Scan request failed");
        assert_eq!(scan_response.status(), StatusCode::OK);

        let context_response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/task-context")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"question":"改 helper 会影响谁？","anchor_symbols":["helper"],"budget":"normal"}"#,
                    ))
                    .expect("Failed to build task context request"),
            )
            .await
            .expect("Task context request failed");
        assert_eq!(context_response.status(), StatusCode::OK);

        let body = to_bytes(context_response.into_body(), usize::MAX)
            .await
            .expect("Failed to read response body");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("Failed to parse context response");
        assert_eq!(payload["scene"], "impact");
        assert_eq!(payload["status"], "ready");
        assert_eq!(
            payload["package"]["target"]["qualified_name"],
            "src/lib.rs::helper"
        );
        assert_eq!(payload["package"]["risk_summary"]["risk"], "low");
    }

    #[tokio::test]
    async fn test_rest_task_context_can_include_source_snippet() {
        let (_temp_dir, server) = sample_server().await;
        let router = build_router(server.clone());

        let scan_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/projects/scan")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .expect("Failed to build scan request"),
            )
            .await
            .expect("Scan request failed");
        assert_eq!(scan_response.status(), StatusCode::OK);

        let context_response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/task-context")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"question":"为什么这里失败？","runtime":{"stacktrace_symbols":["helper"]},"allow_source_snippets":true,"budget":"lean"}"#,
                    ))
                    .expect("Failed to build task context request"),
            )
            .await
            .expect("Task context request failed");
        assert_eq!(context_response.status(), StatusCode::OK);

        let body = to_bytes(context_response.into_body(), usize::MAX)
            .await
            .expect("Failed to read response body");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("Failed to parse context response");
        assert_eq!(payload["scene"], "behavior");
        assert_eq!(payload["status"], "needs_code_read");
        assert!(!payload["package"]["source_snippets"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(payload["package"]["source_snippets"][0]["snippet"]
            .as_str()
            .unwrap()
            .contains("pub fn helper()"));
    }

    #[tokio::test]
    async fn test_disabled_rest_tool_returns_not_found() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        std::fs::create_dir_all(temp_dir.path().join("src")).expect("Failed to create src dir");
        std::fs::write(
            temp_dir.path().join("src/lib.rs"),
            "pub fn entry() { helper(); }\npub fn helper() {}\n",
        )
        .expect("Failed to write fixture source");
        let server =
            QuickDepServer::from_workspace_with_tools(temp_dir.path(), vec!["scan_project".into()])
                .await
                .expect("Failed to create filtered server");

        let response = build_router(server)
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/interfaces/search")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"query":"helper"}"#))
                    .expect("Failed to build search request"),
            )
            .await
            .expect("Search request failed");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_cors_preflight_headers() {
        let (_temp_dir, server) = sample_server().await;
        let response = build_router(server)
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/api/projects")
                    .header(header::ORIGIN, "http://localhost:3000")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                    .body(Body::empty())
                    .expect("Failed to build preflight request"),
            )
            .await
            .expect("Preflight request failed");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&header::HeaderValue::from_static("*"))
        );
        assert!(response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_METHODS));
    }

    #[tokio::test]
    async fn test_websocket_status_stream_reports_updates() {
        let (_temp_dir, server) = sample_server().await;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("Failed to bind test listener");
        let address = listener
            .local_addr()
            .expect("Failed to read test listener address");
        let router = build_router(server.clone());

        let serve_handle = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("Test server failed");
        });

        let url = format!("ws://{address}/ws/projects?interval_ms=100");
        let (mut socket, _response) = connect_async(&url)
            .await
            .expect("Failed to connect websocket");

        let initial = socket
            .next()
            .await
            .expect("Expected initial websocket message")
            .expect("Websocket message should be valid");
        let initial = match initial {
            Message::Text(text) => serde_json::from_str::<serde_json::Value>(&text)
                .expect("Failed to parse initial websocket payload"),
            other => panic!("Unexpected websocket message: {other:?}"),
        };
        assert_eq!(initial["type"], "status");

        server
            .scan_project(rmcp::handler::server::wrapper::Parameters(
                crate::mcp::ScanProjectRequest {
                    project: crate::mcp::ProjectTarget::default(),
                    rebuild: false,
                },
            ))
            .await
            .expect("Failed to trigger scan");

        let mut saw_loaded_status = false;
        for _ in 0..10 {
            let updated = tokio::time::timeout(std::time::Duration::from_secs(2), socket.next())
                .await
                .expect("Timed out waiting for websocket status")
                .expect("Expected updated websocket message")
                .expect("Updated websocket message should be valid");
            let updated = match updated {
                Message::Text(text) => serde_json::from_str::<serde_json::Value>(&text)
                    .expect("Failed to parse updated websocket payload"),
                other => panic!("Unexpected websocket message: {other:?}"),
            };

            if updated["type"] == "status"
                && updated["data"]["project"]["state"]["Loaded"]["watching"] == true
            {
                saw_loaded_status = true;
                break;
            }
        }

        assert!(
            saw_loaded_status,
            "expected websocket to report loaded status"
        );

        serve_handle.abort();
    }

    #[tokio::test]
    async fn test_bind_error_is_returned_before_spawn() {
        let occupied = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("Failed to bind occupied socket");
        let port = occupied
            .local_addr()
            .expect("Failed to read occupied socket address")
            .port();
        let (_temp_dir, server) = sample_server().await;

        let error = spawn_http_server(server, port)
            .await
            .expect_err("binding should fail");

        assert!(error.to_string().contains("failed to bind HTTP listener"));
    }
}
