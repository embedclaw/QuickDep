//! Logging setup implementation
//!
//! Provides tracing initialization with:
//! - Rolling file appender (daily rotation)
//! - stderr output for real-time monitoring
//! - Layer-based architecture for flexibility

use std::path::PathBuf;
use thiserror::Error;
use tracing::Level;
use tracing_appender::{non_blocking, rolling::RollingFileAppender};
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter, Layer,
};

/// Log level configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogLevel {
    /// Debug level - detailed diagnostic information
    Debug,
    /// Info level - general operational information (default)
    #[default]
    Info,
    /// Warn level - potential issues
    Warn,
    /// Error level - errors and failures
    Error,
}

impl std::str::FromStr for LogLevel {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "debug" => Ok(LogLevel::Debug),
            "info" => Ok(LogLevel::Info),
            "warn" | "warning" => Ok(LogLevel::Warn),
            "error" => Ok(LogLevel::Error),
            _ => Err(()),
        }
    }
}

impl LogLevel {
    /// Convert to tracing::Level
    pub fn to_tracing_level(self) -> Level {
        match self {
            LogLevel::Debug => Level::DEBUG,
            LogLevel::Info => Level::INFO,
            LogLevel::Warn => Level::WARN,
            LogLevel::Error => Level::ERROR,
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Debug => write!(f, "debug"),
            LogLevel::Info => write!(f, "info"),
            LogLevel::Warn => write!(f, "warn"),
            LogLevel::Error => write!(f, "error"),
        }
    }
}

/// Logging initialization errors
#[derive(Debug, Error)]
pub enum LogError {
    /// Failed to create log directory
    #[error("Failed to create log directory: {0}")]
    CreateDir(String),

    /// Failed to initialize tracing subscriber
    #[error("Failed to initialize logging: {0}")]
    Init(String),
}

/// Initialize the logging system
///
/// This function sets up a layered logging system with:
/// 1. A rolling file appender that rotates daily, writing to `.quickdep/logs/quickdep.log`
/// 2. A stderr writer for real-time output of important messages
///
/// # Arguments
///
/// * `level` - The minimum log level to record
/// * `log_dir` - Optional custom log directory. If None, uses `<project_root>/.quickdep/logs/`
///
/// # Returns
///
/// Returns `Ok(())` on success, or a `LogError` on failure.
///
/// # Example
///
/// ```no_run
/// use quickdep::log::{init_logging, LogLevel};
///
/// fn main() -> Result<(), quickdep::log::LogError> {
///     // Basic initialization with default settings
///     init_logging(LogLevel::Info, None)?;
///
///     // With custom log directory
///     init_logging(LogLevel::Debug, Some("/var/log/quickdep".into()))?;
///     Ok(())
/// }
/// ```
pub fn init_logging(level: LogLevel, log_dir: Option<PathBuf>) -> Result<(), LogError> {
    // Determine log directory
    let log_directory = match log_dir {
        Some(dir) => dir,
        None => {
            // Default to .quickdep/logs in current directory
            let mut dir = std::env::current_dir()
                .map_err(|e| LogError::CreateDir(format!("Cannot get current directory: {}", e)))?;
            dir.push(".quickdep");
            dir.push("logs");
            dir
        }
    };

    // Create log directory if it doesn't exist
    if !log_directory.exists() {
        std::fs::create_dir_all(&log_directory).map_err(|e| {
            LogError::CreateDir(format!("Cannot create {}: {}", log_directory.display(), e))
        })?;
    }

    // Create rolling file appender (daily rotation)
    let file_appender = RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("quickdep")
        .filename_suffix("log")
        .max_log_files(7) // Keep up to 7 days of logs
        .build(&log_directory)
        .map_err(|e| LogError::CreateDir(format!("Cannot create file appender: {}", e)))?;

    // Create non-blocking writers
    let (file_writer, file_guard) = non_blocking(file_appender);
    let (stderr_writer, stderr_guard) = non_blocking(std::io::stderr());

    // Create environment filter for log level
    let filter = EnvFilter::from_default_env().add_directive(level.to_tracing_level().into());

    // File layer - verbose format with timestamps and span events
    let file_layer = fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false) // No ANSI colors in file
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_line_number(true)
        .with_file(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_filter(filter.clone());

    // Stderr layer - compact format for real-time viewing
    // Only show INFO and above for stderr to reduce noise
    let stderr_filter = EnvFilter::from_default_env().add_directive(match level {
        LogLevel::Debug => Level::INFO.into(), // Debug mode: show INFO+ on stderr
        LogLevel::Info => Level::INFO.into(),
        LogLevel::Warn => Level::WARN.into(),
        LogLevel::Error => Level::ERROR.into(),
    });

    let stderr_layer = fmt::layer()
        .with_writer(stderr_writer)
        .with_ansi(true) // ANSI colors for terminal
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_line_number(false)
        .with_file(false)
        .with_filter(stderr_filter);

    // Initialize subscriber with both layers
    tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .try_init()
        .map_err(|e| LogError::Init(format!("Failed to set global subscriber: {}", e)))?;

    // Store guards to prevent them from being dropped
    // These need to live for the duration of the program
    std::mem::forget(file_guard);
    std::mem::forget(stderr_guard);

    tracing::info!(
        "Logging initialized: level={}, file={}/quickdep.log",
        level,
        log_directory.display()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_to_tracing() {
        assert_eq!(LogLevel::Debug.to_tracing_level(), Level::DEBUG);
        assert_eq!(LogLevel::Info.to_tracing_level(), Level::INFO);
        assert_eq!(LogLevel::Warn.to_tracing_level(), Level::WARN);
        assert_eq!(LogLevel::Error.to_tracing_level(), Level::ERROR);
    }

    #[test]
    fn test_log_level_from_str() {
        assert_eq!("debug".parse::<LogLevel>(), Ok(LogLevel::Debug));
        assert_eq!("DEBUG".parse::<LogLevel>(), Ok(LogLevel::Debug));
        assert_eq!("info".parse::<LogLevel>(), Ok(LogLevel::Info));
        assert_eq!("INFO".parse::<LogLevel>(), Ok(LogLevel::Info));
        assert_eq!("warn".parse::<LogLevel>(), Ok(LogLevel::Warn));
        assert_eq!("warning".parse::<LogLevel>(), Ok(LogLevel::Warn));
        assert_eq!("error".parse::<LogLevel>(), Ok(LogLevel::Error));
        assert_eq!("invalid".parse::<LogLevel>(), Err(()));
    }

    #[test]
    fn test_log_level_display() {
        assert_eq!(format!("{}", LogLevel::Debug), "debug");
        assert_eq!(format!("{}", LogLevel::Info), "info");
        assert_eq!(format!("{}", LogLevel::Warn), "warn");
        assert_eq!(format!("{}", LogLevel::Error), "error");
    }

    #[test]
    fn test_log_level_default() {
        assert_eq!(LogLevel::default(), LogLevel::Info);
    }
}
