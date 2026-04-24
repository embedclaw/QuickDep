//! Resolver module - Import parsing and cross-file symbol resolution
//!
//! This module handles:
//! - Import statement parsing (M6.1)
//! - Module path resolution (M6.2)
//! - Symbol matching (M6.3)
//! - Glob import handling (M6.4)
//! - Alias processing (M6.5)
//! - External symbol marking (M6.6)

pub mod import;
pub mod module;
pub mod symbol;

pub use import::{Import, ImportKind, ImportParser, RustImportParser};
pub use module::{normalize_module_path, rust_module_path, symbol_rust_path};
pub use symbol::{ResolutionSummary, Resolver, UnresolvedDependency};
