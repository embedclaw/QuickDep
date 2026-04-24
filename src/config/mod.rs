//! Configuration management module for QuickDep.
//!
//! This module provides:
//! - Settings structure for all configuration options
//! - TOML configuration file loading
//! - Default configuration values
//! - Configuration validation
//!
//! # Configuration File
//!
//! QuickDep looks for configuration in the following order:
//! 1. `quickdep.toml` in the current directory
//! 2. `.quickdep/config.toml` in the current directory
//!
//! # Example Configuration
//!
//! ```toml
//! [scan]
//! include = ["src/**"]
//! exclude = ["target/**", "node_modules/**"]
//! include_tests = false
//! languages = ["rust", "typescript", "javascript", "java", "csharp", "kotlin", "php", "ruby", "swift", "objc"]
//!
//! [parser]
//! [parser.map]
//! ".py" = "python"
//!
//! [server]
//! http_enabled = false
//! http_port = 8080
//!
//! [log]
//! level = "info"
//!
//! [watcher]
//! idle_timeout = "5m"
//! ```

mod loader;
mod settings;

pub use loader::*;
pub use settings::*;

use thiserror::Error;

/// Configuration error types.
#[derive(Error, Debug)]
pub enum ConfigError {
    /// Error reading configuration file.
    #[error("Failed to read configuration file: {0}")]
    ReadError(#[from] std::io::Error),

    /// Error parsing TOML configuration.
    #[error("Failed to parse configuration: {0}")]
    ParseError(#[from] toml::de::Error),

    /// Configuration validation error.
    #[error("Configuration validation failed: {0}")]
    ValidationError(String),
}

/// Result type for configuration operations.
pub type Result<T> = std::result::Result<T, ConfigError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings_is_valid() {
        let settings = Settings::default();
        assert!(settings.validate().is_ok());
    }
}
