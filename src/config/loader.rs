//! Configuration loader for QuickDep.
//!
//! This module provides functions to load configuration from TOML files.
//! Configuration files are searched in a specific order:
//! 1. `quickdep.toml` in the current directory
//! 2. `.quickdep/config.toml` in the current directory

use std::path::{Path, PathBuf};

use super::{ConfigError, Result, Settings};

/// Default configuration file names to search.
pub const CONFIG_FILE_NAMES: &[&str] = &["quickdep.toml", ".quickdep/config.toml"];

/// Configuration loader.
///
/// Handles finding and loading configuration files.
#[derive(Debug, Clone)]
pub struct ConfigLoader {
    /// Base directory to search for configuration files.
    base_dir: PathBuf,
}

impl ConfigLoader {
    /// Creates a new loader for the given base directory.
    ///
    /// # Arguments
    ///
    /// * `base_dir` - The directory to search for configuration files.
    ///
    /// # Examples
    ///
    /// ```
    /// use quickdep::config::ConfigLoader;
    /// use std::path::Path;
    ///
    /// let loader = ConfigLoader::new(Path::new("/path/to/project"));
    /// ```
    #[must_use]
    pub fn new(base_dir: &Path) -> Self {
        Self {
            base_dir: base_dir.to_path_buf(),
        }
    }

    /// Creates a loader for the current working directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the current working directory cannot be determined.
    pub fn for_current_dir() -> Result<Self> {
        let base_dir = std::env::current_dir().map_err(|e| {
            ConfigError::ValidationError(format!("Cannot get current directory: {}", e))
        })?;
        Ok(Self::new(&base_dir))
    }

    /// Searches for a configuration file in the base directory.
    ///
    /// Returns the first existing file from the search order:
    /// 1. `quickdep.toml`
    /// 2. `.quickdep/config.toml`
    ///
    /// # Returns
    ///
    /// The path to the found configuration file, or `None` if no file exists.
    #[must_use]
    pub fn find_config_file(&self) -> Option<PathBuf> {
        for name in CONFIG_FILE_NAMES {
            let path = self.base_dir.join(name);
            if path.exists() {
                return Some(path);
            }
        }
        None
    }

    /// Loads configuration from a file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the configuration file.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be read
    /// - The TOML content is malformed
    /// - The configuration is invalid
    ///
    /// # Examples
    ///
    /// ```
    /// use quickdep::config::{ConfigLoader, Settings};
    /// use std::path::Path;
    ///
    /// let loader = ConfigLoader::new(Path::new("/path/to/project"));
    /// let settings = loader.load_from_file(Path::new("quickdep.toml")).unwrap();
    /// ```
    pub fn load_from_file(&self, path: &Path) -> Result<Settings> {
        tracing::debug!("Loading configuration from: {}", path.display());

        let content = std::fs::read_to_string(path)?;
        let settings: Settings = toml::from_str(&content)?;

        // Validate the loaded configuration
        settings.validate()?;

        tracing::info!("Configuration loaded successfully from: {}", path.display());
        Ok(settings)
    }

    /// Loads configuration, searching for a file or using defaults.
    ///
    /// This is the primary method for loading configuration. It:
    /// 1. Searches for a configuration file
    /// 2. If found, loads and validates it
    /// 3. If not found, returns default settings
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - A configuration file exists but cannot be read
    /// - The TOML content is malformed
    /// - The configuration is invalid
    ///
    /// # Examples
    ///
    /// ```
    /// use quickdep::config::ConfigLoader;
    /// use std::path::Path;
    ///
    /// let loader = ConfigLoader::new(Path::new("/path/to/project"));
    /// let settings = loader.load().unwrap();
    /// ```
    pub fn load(&self) -> Result<Settings> {
        match self.find_config_file() {
            Some(path) => self.load_from_file(&path),
            None => {
                tracing::debug!("No configuration file found, using defaults");
                let settings = Settings::default();
                settings.validate()?;
                Ok(settings)
            }
        }
    }

    /// Returns the base directory for this loader.
    #[must_use]
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }
}

/// Loads configuration for the given directory.
///
/// This is a convenience function that creates a loader and loads settings.
///
/// # Arguments
///
/// * `dir` - Directory to search for configuration files.
///
/// # Errors
///
/// Returns an error if:
/// - A configuration file exists but cannot be read
/// - The TOML content is malformed
/// - The configuration is invalid
///
/// # Examples
///
/// ```
/// use quickdep::config::load_settings;
/// use std::path::Path;
///
/// let settings = load_settings(Path::new("/path/to/project")).unwrap();
/// ```
pub fn load_settings(dir: &Path) -> Result<Settings> {
    let loader = ConfigLoader::new(dir);
    loader.load()
}

/// Loads configuration from the current working directory.
///
/// This is a convenience function for loading settings from the current directory.
///
/// # Errors
///
/// Returns an error if:
/// - The current directory cannot be determined
/// - A configuration file exists but cannot be read
/// - The TOML content is malformed
/// - The configuration is invalid
///
/// # Examples
///
/// ```
/// use quickdep::config::load_settings_for_current_dir;
///
/// let settings = load_settings_for_current_dir().unwrap();
/// ```
pub fn load_settings_for_current_dir() -> Result<Settings> {
    let loader = ConfigLoader::for_current_dir()?;
    loader.load()
}

/// Parses settings from a TOML string.
///
/// # Arguments
///
/// * `content` - TOML content string.
///
/// # Errors
///
/// Returns an error if:
/// - The TOML content is malformed
/// - The configuration is invalid
///
/// # Examples
///
/// ```
/// use quickdep::config::parse_settings;
///
/// let toml = r#"
/// [scan]
/// include = ["src/**"]
/// "#;
///
/// let settings = parse_settings(toml).unwrap();
/// ```
pub fn parse_settings(content: &str) -> Result<Settings> {
    let settings: Settings = toml::from_str(content)?;
    settings.validate()?;
    Ok(settings)
}

/// Creates a sample configuration file.
///
/// This generates a TOML file with all default values, useful as a template
/// for users to customize.
///
/// # Arguments
///
/// * `path` - Path to write the sample configuration file.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
///
/// # Examples
///
/// ```
/// use quickdep::config::write_sample_config;
/// use std::path::Path;
///
/// write_sample_config(Path::new("quickdep.toml")).unwrap();
/// ```
pub fn write_sample_config(path: &Path) -> Result<()> {
    let settings = Settings::default();
    let content = toml::to_string_pretty(&settings)
        .map_err(|e| ConfigError::ValidationError(format!("Failed to serialize config: {}", e)))?;

    std::fs::write(path, content)?;
    tracing::info!("Sample configuration written to: {}", path.display());
    Ok(())
}

// Keep backward compatibility with existing function signatures
/// Load configuration from a file.
///
/// This is an alias for [`load_settings`] for backward compatibility.
pub fn load_config<P: AsRef<Path>>(path: P) -> Result<Settings> {
    let loader = ConfigLoader::new(path.as_ref().parent().unwrap_or_else(|| Path::new(".")));
    loader.load_from_file(path.as_ref())
}

/// Load configuration from default locations.
///
/// This is an alias for [`load_settings_for_current_dir`] for backward compatibility.
pub fn load_default_config() -> Result<Settings> {
    load_settings_for_current_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_default_settings_with_loader() {
        let dir = tempdir().unwrap();
        let loader = ConfigLoader::new(dir.path());
        let settings = loader.load().unwrap();

        assert_eq!(settings.scan.include, vec!["**/*"]);
    }

    #[test]
    fn test_load_from_file() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("quickdep.toml");

        let content = r#"
[scan]
include = ["lib/**"]
exclude = ["vendor/**"]
languages = ["rust"]

[parser.map]
".vue" = "typescript"

[server]
http_enabled = true
http_port = 9000

[log]
level = "debug"
"#;
        fs::write(&config_path, content).unwrap();

        let loader = ConfigLoader::new(dir.path());
        let settings = loader.load().unwrap();

        assert_eq!(settings.scan.include, vec!["lib/**"]);
        assert_eq!(settings.scan.exclude, vec!["vendor/**"]);
        assert_eq!(settings.scan.languages, vec!["rust"]);
        assert_eq!(
            settings.parser.map.get(".vue").map(String::as_str),
            Some("typescript")
        );
        assert!(settings.server.http_enabled);
        assert_eq!(settings.server.http_port, 9000);
        assert_eq!(settings.log.level, "debug");
    }

    #[test]
    fn test_find_config_file_quickdep_toml() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("quickdep.toml");
        fs::write(&config_path, "").unwrap();

        let loader = ConfigLoader::new(dir.path());
        let found = loader.find_config_file().unwrap();

        assert_eq!(found, config_path);
    }

    #[test]
    fn test_find_config_file_nested() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".quickdep")).unwrap();
        let config_path = dir.path().join(".quickdep/config.toml");
        fs::write(&config_path, "").unwrap();

        let loader = ConfigLoader::new(dir.path());
        let found = loader.find_config_file().unwrap();

        assert_eq!(found, config_path);
    }

    #[test]
    fn test_find_config_file_priority() {
        let dir = tempdir().unwrap();

        // Create both files - quickdep.toml should be found first
        fs::write(dir.path().join("quickdep.toml"), "").unwrap();
        fs::create_dir(dir.path().join(".quickdep")).unwrap();
        fs::write(dir.path().join(".quickdep/config.toml"), "").unwrap();

        let loader = ConfigLoader::new(dir.path());
        let found = loader.find_config_file().unwrap();

        assert_eq!(found.file_name().unwrap(), "quickdep.toml");
    }

    #[test]
    fn test_no_config_file() {
        let dir = tempdir().unwrap();
        let loader = ConfigLoader::new(dir.path());

        assert!(loader.find_config_file().is_none());
        let settings = loader.load().unwrap();
        assert_eq!(settings.scan.include, vec!["**/*"]);
    }

    #[test]
    fn test_parse_settings() {
        let content = r#"
[scan]
include = ["src/**", "lib/**"]
"#;
        let settings = parse_settings(content).unwrap();
        assert_eq!(settings.scan.include, vec!["src/**", "lib/**"]);
    }

    #[test]
    fn test_parse_settings_invalid_toml() {
        let content = "invalid [toml";
        assert!(parse_settings(content).is_err());
    }

    #[test]
    fn test_parse_settings_invalid_config() {
        let content = r#"
[scan]
include = []
"#;
        assert!(parse_settings(content).is_err());
    }

    #[test]
    fn test_write_sample_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("quickdep.toml");

        write_sample_config(&path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("[scan]"));
        assert!(content.contains("[server]"));
        assert!(content.contains("[log]"));
        assert!(content.contains("[watcher]"));
    }

    #[test]
    fn test_partial_config() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("quickdep.toml");

        // Only specify some fields - rest should use defaults
        let content = r#"
[server]
http_port = 3000
"#;
        fs::write(&config_path, content).unwrap();

        let loader = ConfigLoader::new(dir.path());
        let settings = loader.load().unwrap();

        assert_eq!(settings.server.http_port, 3000);
        // Should have default values for other fields
        assert_eq!(settings.scan.include, vec!["**/*"]);
        assert_eq!(settings.log.level, "info");
    }

    #[test]
    fn test_duration_config() {
        let content = r#"
[watcher]
idle_timeout = "30s"
"#;
        let settings = parse_settings(content).unwrap();
        assert_eq!(
            settings.watcher.idle_timeout,
            std::time::Duration::from_secs(30)
        );
    }

    #[test]
    fn test_duration_config_minutes() {
        let content = r#"
[watcher]
idle_timeout = "10m"
"#;
        let settings = parse_settings(content).unwrap();
        assert_eq!(
            settings.watcher.idle_timeout,
            std::time::Duration::from_secs(600)
        );
    }

    #[test]
    fn test_backward_compat_load_config() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("quickdep.toml");

        let content = r#"
[scan]
include = ["src/**"]
"#;
        fs::write(&config_path, content).unwrap();

        let settings = load_config(&config_path).unwrap();
        assert_eq!(settings.scan.include, vec!["src/**"]);
    }

    #[test]
    fn test_backward_compat_load_default_config() {
        let settings = load_default_config().unwrap();
        assert_eq!(settings.scan.include, vec!["**/*"]);
    }
}
