//! Settings structure definitions for QuickDep configuration.
//!
//! This module defines all configuration options organized into logical groups:
//! - ScanConfig: File scanning options
//! - ParserConfig: Parser configuration
//! - ServerConfig: Server settings
//! - LogConfig: Logging configuration
//! - WatcherConfig: File watcher settings

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use super::ConfigError;

/// Root configuration structure containing all QuickDep settings.
///
/// This structure can be loaded from a TOML configuration file or created
/// with default values. All fields use `#[serde(default)]` to allow partial
/// configuration files.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// Scanning configuration.
    #[serde(default)]
    pub scan: ScanConfig,

    /// Parser configuration.
    #[serde(default)]
    pub parser: ParserConfig,

    /// Server configuration.
    #[serde(default)]
    pub server: ServerConfig,

    /// Logging configuration.
    #[serde(default)]
    pub log: LogConfig,

    /// File watcher configuration.
    #[serde(default)]
    pub watcher: WatcherConfig,
}

impl Settings {
    /// Creates a new Settings instance with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validates all configuration settings.
    ///
    /// # Errors
    ///
    /// Returns an error if any configuration value is invalid:
    /// - Invalid glob patterns in include/exclude
    /// - Invalid log level
    /// - Invalid idle timeout format
    /// - Invalid HTTP port range
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.scan.validate()?;
        self.parser.validate()?;
        self.server.validate()?;
        self.log.validate()?;
        self.watcher.validate()?;
        Ok(())
    }
}

/// File scanning configuration.
///
/// Controls which files are scanned and parsed for symbols.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScanConfig {
    /// Glob patterns for files to include in scanning.
    ///
    /// Default: `["**/*"]`
    ///
    /// Supports standard glob patterns:
    /// - `**` matches any number of directories
    /// - `*` matches any file name
    ///
    /// # Examples
    ///
    /// - `**/*` - all files under the project root
    /// - `lib/**/*.rs` - all Rust files under lib/
    /// - `**/*.ts` - all TypeScript files
    #[serde(default = "default_include")]
    pub include: Vec<String>,

    /// Glob patterns for files to exclude from scanning.
    ///
    /// Default: common build, cache, VCS, and dependency directories
    ///
    /// Patterns are matched against the relative path from the project root.
    #[serde(default = "default_exclude")]
    pub exclude: Vec<String>,

    /// Whether to include test files in scanning.
    ///
    /// Default: `false`
    ///
    /// When `false`, files matching common test patterns are excluded:
    /// - `*_test.rs`, `*_tests.rs`, `test_*.rs`
    /// - `*_test.go`, `*_tests.go`
    /// - `*.test.ts`, `*.spec.ts`
    /// - `test_*.py`, `*_test.py`
    #[serde(default)]
    pub include_tests: bool,

    /// Programming languages to scan for.
    ///
    /// Default: all supported languages
    ///
    /// Supported languages:
    /// - `rust` - Rust source files (`.rs`)
    /// - `typescript` - TypeScript files (`.ts`, `.tsx`)
    /// - `javascript` - JavaScript files (`.js`, `.jsx`, `.mjs`, `.cjs`)
    /// - `java` - Java source files (`.java`)
    /// - `csharp` - C# source files (`.cs`)
    /// - `kotlin` - Kotlin source files (`.kt`, `.kts`)
    /// - `php` - PHP source files (`.php`, `.phtml`)
    /// - `ruby` - Ruby source files (`.rb`, `.rake`)
    /// - `swift` - Swift source files (`.swift`)
    /// - `objc` - Objective-C source files (`.m`)
    /// - `python` - Python files (`.py`)
    /// - `go` - Go source files (`.go`)
    /// - `c` - C source and header files (`.c`, `.h`)
    /// - `cpp` - C++ source and header files (`.cc`, `.cpp`, `.cxx`, `.hh`, `.hpp`, `.hxx`)
    #[serde(default = "default_languages")]
    pub languages: Vec<String>,
}

fn default_include() -> Vec<String> {
    vec!["**/*".to_string()]
}

fn default_exclude() -> Vec<String> {
    vec![
        "target/**".to_string(),
        "node_modules/**".to_string(),
        ".git/**".to_string(),
        ".quickdep/**".to_string(),
        ".research/**".to_string(),
        ".cache/**".to_string(),
        "dist/**".to_string(),
        "build/**".to_string(),
        "out/**".to_string(),
        "coverage/**".to_string(),
        "artifacts/**".to_string(),
        "tmp/**".to_string(),
        ".tmp/**".to_string(),
        "temp/**".to_string(),
        "venv/**".to_string(),
        ".venv/**".to_string(),
        "__pycache__/**".to_string(),
    ]
}

fn default_languages() -> Vec<String> {
    vec![
        "rust".to_string(),
        "typescript".to_string(),
        "javascript".to_string(),
        "java".to_string(),
        "csharp".to_string(),
        "kotlin".to_string(),
        "php".to_string(),
        "ruby".to_string(),
        "swift".to_string(),
        "objc".to_string(),
        "python".to_string(),
        "go".to_string(),
        "c".to_string(),
        "cpp".to_string(),
    ]
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            include: default_include(),
            exclude: default_exclude(),
            include_tests: false,
            languages: default_languages(),
        }
    }
}

impl ScanConfig {
    /// Validates scan configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Include patterns are empty
    /// - Any glob pattern is invalid
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate include patterns
        if self.include.is_empty() {
            return Err(ConfigError::ValidationError(
                "scan.include cannot be empty".to_string(),
            ));
        }

        // Validate glob patterns
        for pattern in &self.include {
            Self::validate_glob_pattern(pattern)?;
        }

        for pattern in &self.exclude {
            Self::validate_glob_pattern(pattern)?;
        }

        // Validate languages
        for lang in &self.languages {
            if !Self::is_supported_language(lang) {
                return Err(ConfigError::ValidationError(format!(
                    "Unsupported language: {}",
                    lang
                )));
            }
        }

        Ok(())
    }

    /// Validates a glob pattern for correctness.
    fn validate_glob_pattern(pattern: &str) -> Result<(), ConfigError> {
        // Check for empty pattern
        if pattern.is_empty() {
            return Err(ConfigError::ValidationError(
                "Glob pattern cannot be empty".to_string(),
            ));
        }

        // Check for obviously invalid patterns
        // These are patterns that would cause runtime errors in glob matching
        if pattern.contains("***") {
            return Err(ConfigError::ValidationError(format!(
                "Invalid glob pattern '{}': consecutive wildcards are not allowed",
                pattern
            )));
        }

        // Check for unbalanced brackets
        let bracket_count = pattern.chars().filter(|&c| c == '[' || c == ']').count();
        if bracket_count % 2 != 0 {
            return Err(ConfigError::ValidationError(format!(
                "Invalid glob pattern '{}': unbalanced brackets",
                pattern
            )));
        }

        Ok(())
    }

    /// Checks if a language is supported.
    fn is_supported_language(lang: &str) -> bool {
        matches!(
            lang,
            "rust"
                | "typescript"
                | "javascript"
                | "java"
                | "python"
                | "go"
                | "c"
                | "cpp"
                | "csharp"
                | "kotlin"
                | "php"
                | "ruby"
                | "swift"
                | "objc"
        )
    }

    /// Returns file extensions for the configured languages.
    #[must_use]
    pub fn file_extensions(&self) -> Vec<&'static str> {
        self.languages
            .iter()
            .flat_map(|lang| Self::language_extensions(lang))
            .copied()
            .collect()
    }

    /// Returns file extensions for a given language.
    fn language_extensions(lang: &str) -> &'static [&'static str] {
        match lang {
            "rust" => &["rs"],
            "typescript" => &["ts", "tsx"],
            "javascript" => &["js", "jsx", "mjs", "cjs"],
            "java" => &["java"],
            "csharp" => &["cs"],
            "kotlin" => &["kt", "kts"],
            "php" => &["php", "phtml"],
            "ruby" => &["rb", "rake"],
            "swift" => &["swift"],
            "objc" => &["m"],
            "python" => &["py"],
            "go" => &["go"],
            "c" => &["c", "h"],
            "cpp" => &["cc", "cpp", "cxx", "hh", "hpp", "hxx"],
            _ => &[],
        }
    }
}

/// Parser configuration.
///
/// Controls how source files are parsed.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ParserConfig {
    /// Custom mapping from file extensions to language names.
    ///
    /// This allows overriding the default language detection.
    /// Keys are file extensions (including the dot, e.g., `.py`).
    /// Values are language names (e.g., `python`).
    ///
    /// # Example
    ///
    /// ```toml
    /// [parser.map]
    /// ".py" = "python"
    /// ".vue" = "typescript"
    /// ```
    #[serde(default)]
    pub map: HashMap<String, String>,
}

impl ParserConfig {
    /// Validates parser configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - An extension key is empty or malformed
    /// - A mapped language is unsupported
    pub fn validate(&self) -> Result<(), ConfigError> {
        for (extension, language) in &self.map {
            let normalized = extension.trim().trim_start_matches('.');
            if normalized.is_empty() {
                return Err(ConfigError::ValidationError(
                    "parser.map keys must include a file extension".to_string(),
                ));
            }

            if normalized.contains(['/', '\\']) {
                return Err(ConfigError::ValidationError(format!(
                    "parser.map key '{}' must be a file extension, not a path",
                    extension
                )));
            }

            if !ScanConfig::is_supported_language(language) {
                return Err(ConfigError::ValidationError(format!(
                    "Unsupported parser.map language '{}' for extension '{}'",
                    language, extension
                )));
            }
        }

        Ok(())
    }
}

/// Server configuration.
///
/// Controls the MCP and HTTP server settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerConfig {
    /// Whether to enable the HTTP server.
    ///
    /// Default: `false`
    ///
    /// When enabled, QuickDep provides a REST API alongside the MCP protocol.
    /// The HTTP server is useful for:
    /// - Web frontend integration
    /// - Third-party tool integration
    /// - Debugging and monitoring
    #[serde(default)]
    pub http_enabled: bool,

    /// HTTP server port.
    ///
    /// Default: `8080`
    ///
    /// Only used when `http_enabled` is `true`.
    #[serde(default = "default_http_port")]
    pub http_port: u16,
}

fn default_http_port() -> u16 {
    8080
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            http_enabled: false,
            http_port: default_http_port(),
        }
    }
}

impl ServerConfig {
    /// Validates server configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - HTTP port is 0
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.http_port == 0 {
            return Err(ConfigError::ValidationError(
                "server.http_port cannot be 0".to_string(),
            ));
        }
        Ok(())
    }
}

/// Logging configuration.
///
/// Controls logging output and level.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogConfig {
    /// Log level.
    ///
    /// Default: `"info"`
    ///
    /// Valid values: `trace`, `debug`, `info`, `warn`, `error`
    #[serde(default = "default_log_level")]
    pub level: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

impl LogConfig {
    /// Validates log configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the log level is not one of the valid values.
    pub fn validate(&self) -> Result<(), ConfigError> {
        const VALID_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

        if !VALID_LEVELS.contains(&self.level.as_str()) {
            return Err(ConfigError::ValidationError(format!(
                "Invalid log level '{}'. Valid values: {:?}",
                self.level, VALID_LEVELS
            )));
        }

        Ok(())
    }
}

/// File watcher configuration.
///
/// Controls file system watching behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WatcherConfig {
    /// Idle timeout before pausing file watching.
    ///
    /// Default: `5m` (5 minutes)
    ///
    /// After this duration of inactivity, file watching is paused
    /// to conserve system resources. Watching resumes automatically
    /// when a query is made.
    ///
    /// Format: Human-readable duration string (e.g., "5m", "30s", "1h")
    #[serde(default = "default_idle_timeout", with = "serde_duration")]
    pub idle_timeout: Duration,
}

fn default_idle_timeout() -> Duration {
    Duration::from_secs(5 * 60) // 5 minutes
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            idle_timeout: default_idle_timeout(),
        }
    }
}

impl WatcherConfig {
    /// Validates watcher configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if idle_timeout is zero.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.idle_timeout.is_zero() {
            return Err(ConfigError::ValidationError(
                "watcher.idle_timeout cannot be zero".to_string(),
            ));
        }
        Ok(())
    }
}

/// Custom serde module for Duration serialization.
mod serde_duration {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    /// Serializes a Duration as a human-readable string.
    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let secs = duration.as_secs();

        if secs.is_multiple_of(3600) {
            format!("{}h", secs / 3600)
        } else if secs.is_multiple_of(60) {
            format!("{}m", secs / 60)
        } else {
            format!("{}s", secs)
        }
        .serialize(serializer)
    }

    /// Deserializes a Duration from a human-readable string or integer.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        let s = String::deserialize(deserializer)?;

        // Try to parse as a human-readable duration string
        if let Ok(duration) = parse_human_duration(&s) {
            return Ok(duration);
        }

        // Try to parse as seconds (integer)
        if let Ok(secs) = s.parse::<u64>() {
            return Ok(Duration::from_secs(secs));
        }

        Err(D::Error::custom(format!(
            "Invalid duration format: '{}'. Expected format: '5m', '30s', '1h', or integer seconds",
            s
        )))
    }

    /// Parses a human-readable duration string.
    ///
    /// Supported formats:
    /// - `30s` - 30 seconds
    /// - `5m` - 5 minutes
    /// - `1h` - 1 hour
    pub(crate) fn parse_human_duration(s: &str) -> Result<Duration, ()> {
        let s = s.trim();

        if s.is_empty() {
            return Err(());
        }

        // Extract the numeric part and unit
        let numeric_end = s.find(|c: char| !c.is_ascii_digit()).ok_or(())?;
        let (num_str, unit) = s.split_at(numeric_end);
        let num: u64 = num_str.parse().map_err(|_| ())?;

        let duration = match unit {
            "s" => Duration::from_secs(num),
            "m" => Duration::from_secs(num * 60),
            "h" => Duration::from_secs(num * 3600),
            _ => return Err(()),
        };

        Ok(duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();

        assert_eq!(settings.scan.include, vec!["**/*"]);
        assert_eq!(settings.scan.exclude, default_exclude());
        assert!(!settings.scan.include_tests);
        assert_eq!(
            settings.scan.languages,
            vec![
                "rust",
                "typescript",
                "javascript",
                "java",
                "csharp",
                "kotlin",
                "php",
                "ruby",
                "swift",
                "objc",
                "python",
                "go",
                "c",
                "cpp"
            ]
        );

        assert!(settings.parser.map.is_empty());

        assert!(!settings.server.http_enabled);
        assert_eq!(settings.server.http_port, 8080);

        assert_eq!(settings.log.level, "info");

        assert_eq!(settings.watcher.idle_timeout, Duration::from_secs(5 * 60));
    }

    #[test]
    fn test_settings_validation() {
        let settings = Settings::default();
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn test_empty_include_is_invalid() {
        let mut settings = Settings::default();
        settings.scan.include.clear();
        assert!(settings.validate().is_err());
    }

    #[test]
    fn test_invalid_glob_pattern() {
        let mut settings = Settings::default();
        settings.scan.include = vec!["***".to_string()];
        assert!(settings.validate().is_err());

        settings.scan.include = vec!["[".to_string()];
        assert!(settings.validate().is_err());
    }

    #[test]
    fn test_unsupported_language() {
        let mut settings = Settings::default();
        settings.scan.languages = vec!["unknown".to_string()];
        assert!(settings.validate().is_err());
    }

    #[test]
    fn test_invalid_parser_map_language() {
        let mut settings = Settings::default();
        settings
            .parser
            .map
            .insert(".vue".to_string(), "unknown".to_string());
        assert!(settings.validate().is_err());
    }

    #[test]
    fn test_invalid_parser_map_extension() {
        let mut settings = Settings::default();
        settings
            .parser
            .map
            .insert("".to_string(), "typescript".to_string());
        assert!(settings.validate().is_err());

        settings.parser.map.clear();
        settings
            .parser
            .map
            .insert("src/vue".to_string(), "typescript".to_string());
        assert!(settings.validate().is_err());
    }

    #[test]
    fn test_valid_parser_map() {
        let mut settings = Settings::default();
        settings
            .parser
            .map
            .insert(".vue".to_string(), "typescript".to_string());
        settings
            .parser
            .map
            .insert("script".to_string(), "python".to_string());
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn test_invalid_log_level() {
        let mut settings = Settings::default();
        settings.log.level = "invalid".to_string();
        assert!(settings.validate().is_err());
    }

    #[test]
    fn test_zero_http_port() {
        let mut settings = Settings::default();
        settings.server.http_port = 0;
        assert!(settings.validate().is_err());
    }

    #[test]
    fn test_zero_idle_timeout() {
        let mut settings = Settings::default();
        settings.watcher.idle_timeout = Duration::ZERO;
        assert!(settings.validate().is_err());
    }

    #[test]
    fn test_file_extensions() {
        let config = ScanConfig::default();
        let extensions = config.file_extensions();
        assert!(extensions.contains(&"rs"));
        assert!(extensions.contains(&"ts"));
        assert!(extensions.contains(&"tsx"));
        assert!(extensions.contains(&"js"));
        assert!(extensions.contains(&"jsx"));
        assert!(extensions.contains(&"mjs"));
        assert!(extensions.contains(&"cjs"));
        assert!(extensions.contains(&"java"));
        assert!(extensions.contains(&"cs"));
        assert!(extensions.contains(&"kt"));
        assert!(extensions.contains(&"kts"));
        assert!(extensions.contains(&"php"));
        assert!(extensions.contains(&"phtml"));
        assert!(extensions.contains(&"rb"));
        assert!(extensions.contains(&"rake"));
        assert!(extensions.contains(&"swift"));
        assert!(extensions.contains(&"m"));

        let config = ScanConfig {
            languages: vec!["c".to_string()],
            ..ScanConfig::default()
        };
        let extensions = config.file_extensions();
        assert!(extensions.contains(&"c"));
        assert!(extensions.contains(&"h"));

        let config = ScanConfig {
            languages: vec!["cpp".to_string()],
            ..ScanConfig::default()
        };
        let extensions = config.file_extensions();
        assert!(extensions.contains(&"cpp"));
        assert!(extensions.contains(&"hpp"));
    }

    #[test]
    fn test_parse_human_duration() {
        use super::serde_duration::parse_human_duration;

        assert_eq!(
            parse_human_duration("30s").unwrap(),
            Duration::from_secs(30)
        );
        assert_eq!(
            parse_human_duration("5m").unwrap(),
            Duration::from_secs(300)
        );
        assert_eq!(
            parse_human_duration("1h").unwrap(),
            Duration::from_secs(3600)
        );
        assert!(parse_human_duration("invalid").is_err());
        assert!(parse_human_duration("").is_err());
    }

    #[test]
    fn test_serialize_duration() {
        let config = WatcherConfig::default();
        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("idle_timeout"));
    }
}
