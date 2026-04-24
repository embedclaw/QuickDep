//! Logging module for QuickDep
//!
//! This module provides tracing-based logging with:
//! - File logging with daily rotation
//! - stderr output for key information
//! - Configurable log levels (DEBUG/INFO/WARN/ERROR)
//!
//! # Example
//!
//! ```no_run
//! use quickdep::log::{init_logging, LogLevel};
//!
//! fn main() -> Result<(), quickdep::log::LogError> {
//!     // Initialize logging with INFO level and default log directory
//!     init_logging(LogLevel::Info, None)?;
//!
//!     // Or with custom log directory
//!     init_logging(LogLevel::Debug, Some("/custom/log/path".into()))?;
//!     Ok(())
//! }
//! ```

mod setup;

pub use setup::{init_logging, LogError, LogLevel};

/// Default log file name
pub const LOG_FILE_NAME: &str = "quickdep.log";

/// Default log directory name within .quickdep
pub const LOG_DIR_NAME: &str = "logs";
