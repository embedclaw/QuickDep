//! QuickDep - A Rust MCP service for scanning project code interface dependencies
//!
//! This crate provides:
//! - Code dependency analysis via Tree-sitter
//! - MCP protocol integration for AI agents
//! - SQLite-based persistent storage
//! - Real-time file watching and incremental updates

pub mod cache;
pub mod cli;
pub mod config;
pub mod core;
pub mod daemon;
pub mod log;
pub mod mcp;
pub mod parser;
pub mod project;
pub mod resolver;
pub mod runtime;
pub mod security;
pub mod storage;
pub mod watcher;

// Optional HTTP module
pub mod http;

/// QuickDep version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default cache directory name
pub const CACHE_DIR: &str = ".quickdep";

/// Default database file name
pub const DB_FILE: &str = "symbols.db";

/// Manifest file name
pub const MANIFEST_FILE: &str = "manifest.json";

/// Configuration file name
pub const CONFIG_FILE: &str = "quickdep.toml";
