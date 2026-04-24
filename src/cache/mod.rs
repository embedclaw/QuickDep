//! In-memory caches for query acceleration.
//!
//! The cache layer currently provides:
//! - A symbol-name index for fast interface lookups
//! - A generic TTL query cache for expensive graph lookups
//! - File-based invalidation hooks for incremental updates

pub mod index;
pub mod query;

pub use index::SymbolIndexCache;
pub use query::{QueryCache, DEFAULT_QUERY_TTL};
