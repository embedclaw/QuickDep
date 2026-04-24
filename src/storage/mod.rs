//! Storage module for QuickDep
//!
//! This module provides persistent storage for:
//! - Symbols (functions, classes, structs, etc.)
//! - Dependencies (call, inherit, implement, type use relationships)
//! - Imports (import statements)
//! - File state (hash, status, errors)
//!
//! ## Architecture
//!
//! - `schema.rs`: SQLite schema definitions and migrations
//! - `sqlite.rs`: Storage implementation with CRUD operations
//!
//! ## Features
//!
//! - SQLite with WAL mode for concurrent access
//! - Recursive CTE queries for dependency chain traversal
//! - Transaction-based batch operations
//! - Schema versioning for migration safety
//!
//! ## Usage
//!
//! ```rust,no_run
//! use quickdep::core::{Symbol, SymbolKind};
//! use quickdep::storage::Storage;
//! use std::path::Path;
//!
//! // Create storage instance
//! let storage = Storage::new(Path::new(".quickdep/symbols.db")).unwrap();
//!
//! // Insert a symbol
//! let symbol = Symbol::new(
//!     "helper".into(),
//!     "src/utils.rs::helper".into(),
//!     SymbolKind::Function,
//!     "src/utils.rs".into(),
//!     10,
//!     5,
//! );
//! storage.insert_symbol(&symbol).unwrap();
//!
//! // Query dependency chain
//! let chain = storage.get_dependency_chain_forward(&symbol.id, 5).unwrap();
//! assert!(chain.iter().all(|node| node.depth <= 5));
//! ```

pub(crate) mod fts;
pub mod schema;
pub mod sqlite;

pub use sqlite::*;
