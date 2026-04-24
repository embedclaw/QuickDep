//! Security module for QuickDep
//!
//! Provides security validation functions to prevent path traversal attacks
//! and ensure project isolation.

mod path;

pub use path::{generate_project_id, validate_path, validate_project_id, SecurityError};
