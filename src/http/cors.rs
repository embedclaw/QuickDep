//! CORS configuration for the HTTP server.

use axum::http::{header, Method};
use tower_http::cors::{Any, CorsLayer};

/// Build the shared CORS layer for QuickDep HTTP routes.
pub fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::ACCEPT, header::ORIGIN])
}
