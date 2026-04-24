//! Project structure definition
//!
//! A Project represents a code repository being analyzed by QuickDep.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;

use crate::project::{ProjectId, ProjectState};

/// Error type for project operations
#[derive(Debug, Error)]
pub enum ProjectError {
    /// Invalid project path
    #[error("Invalid project path: {0}")]
    InvalidPath(String),

    /// Project path not found
    #[error("Project path not found: {0}")]
    PathNotFound(PathBuf),

    /// Failed to create project cache directory
    #[error("Failed to create cache directory: {0}")]
    CacheDirectoryError(#[source] std::io::Error),

    /// Project already registered
    #[error("Project already registered with ID: {0}")]
    AlreadyRegistered(String),

    /// Project not found
    #[error("Project not found: {0}")]
    NotFound(String),

    /// Invalid state transition
    #[error("Invalid state transition from {from} to {to}")]
    InvalidStateTransition { from: String, to: String },

    /// Scan failed
    #[error("Scan failed: {0}")]
    ScanFailed(String),

    /// Cancelled
    #[error("Operation cancelled")]
    Cancelled,
}

/// Project configuration
#[derive(Debug, Clone)]
pub struct ProjectConfig {
    /// Include patterns for file scanning (glob patterns)
    pub include: Vec<String>,

    /// Exclude patterns for file scanning (glob patterns)
    pub exclude: Vec<String>,

    /// Languages to parse
    pub languages: Vec<String>,

    /// Whether to include test files in scanning
    pub include_tests: bool,

    /// Custom parser extension overrides
    pub parser_map: HashMap<String, String>,

    /// Idle timeout for watching (in seconds)
    pub idle_timeout_secs: u64,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            include: vec!["**/*".to_string()],
            exclude: vec![
                "target/**".to_string(),
                "node_modules/**".to_string(),
                ".git/**".to_string(),
                ".quickdep/**".to_string(),
                "dist/**".to_string(),
                "build/**".to_string(),
                "venv/**".to_string(),
                ".venv/**".to_string(),
                "__pycache__/**".to_string(),
            ],
            languages: vec![
                "rust".to_string(),
                "typescript".to_string(),
                "python".to_string(),
                "go".to_string(),
                "c".to_string(),
                "cpp".to_string(),
            ],
            include_tests: false,
            parser_map: HashMap::new(),
            idle_timeout_secs: 300, // 5 minutes
        }
    }
}

impl ProjectConfig {
    /// Create a new configuration with custom include patterns
    pub fn with_include(mut self, patterns: Vec<String>) -> Self {
        self.include = patterns;
        self
    }

    /// Create a new configuration with custom exclude patterns
    pub fn with_exclude(mut self, patterns: Vec<String>) -> Self {
        self.exclude = patterns;
        self
    }

    /// Create a new configuration with specific languages
    pub fn with_languages(mut self, languages: Vec<String>) -> Self {
        self.languages = languages;
        self
    }

    /// Create a new configuration including test files
    pub fn with_tests(mut self, include: bool) -> Self {
        self.include_tests = include;
        self
    }

    /// Create a new configuration with parser extension overrides
    pub fn with_parser_map(mut self, parser_map: HashMap<String, String>) -> Self {
        self.parser_map = parser_map;
        self
    }

    /// Create a new configuration with custom idle timeout
    pub fn with_idle_timeout(mut self, secs: u64) -> Self {
        self.idle_timeout_secs = secs;
        self
    }
}

/// A project being managed by QuickDep
#[derive(Debug)]
pub struct Project {
    /// Unique project identifier
    pub id: ProjectId,

    /// Canonical path to project root
    pub path: PathBuf,

    /// Human-readable project name
    pub name: String,

    /// Current project state
    pub state: ProjectState,

    /// Project configuration
    pub config: ProjectConfig,

    /// Time of last access (for idle timeout tracking)
    last_access: Instant,

    /// Scan cancellation flag (Arc for thread-safe sharing)
    cancel_flag: Arc<std::sync::atomic::AtomicBool>,
}

impl Project {
    /// Create a new project
    ///
    /// # Arguments
    /// * `path` - Path to the project directory
    /// * `name` - Human-readable name for the project
    /// * `config` - Optional configuration (uses default if not provided)
    ///
    /// # Returns
    /// * `Ok(Project)` - The created project
    /// * `Err(ProjectError)` - If the path is invalid or doesn't exist
    pub fn new(
        path: impl AsRef<Path>,
        name: impl Into<String>,
        config: Option<ProjectConfig>,
    ) -> Result<Self, ProjectError> {
        let path = path.as_ref();

        // Validate path exists
        if !path.exists() {
            return Err(ProjectError::PathNotFound(path.to_path_buf()));
        }

        // Generate project ID from canonical path
        let id = ProjectId::from_path(path).map_err(|e| {
            ProjectError::InvalidPath(format!("Failed to generate project ID: {}", e))
        })?;

        // Get canonical path
        let canonical = path
            .canonicalize()
            .map_err(|e| ProjectError::InvalidPath(format!("Failed to canonicalize: {}", e)))?;

        // Ensure cache directory exists
        let cache_dir = canonical.join(crate::CACHE_DIR);
        std::fs::create_dir_all(&cache_dir).map_err(ProjectError::CacheDirectoryError)?;

        Ok(Self {
            id,
            path: canonical,
            name: name.into(),
            state: ProjectState::not_loaded(),
            config: config.unwrap_or_default(),
            last_access: Instant::now(),
            cancel_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }

    /// Get the cache directory path for this project
    pub fn cache_dir(&self) -> PathBuf {
        self.path.join(crate::CACHE_DIR)
    }

    /// Get the database file path for this project
    pub fn database_path(&self) -> PathBuf {
        self.cache_dir().join(crate::DB_FILE)
    }

    /// Get the manifest file path for this project
    pub fn manifest_path(&self) -> PathBuf {
        self.cache_dir().join(crate::MANIFEST_FILE)
    }

    /// Check if the project needs to be loaded (lazy loading trigger)
    pub fn needs_loading(&self) -> bool {
        matches!(self.state, ProjectState::NotLoaded)
    }

    /// Check if the project is currently loading
    pub fn is_loading(&self) -> bool {
        self.state.is_loading()
    }

    /// Check if the project is fully loaded
    pub fn is_loaded(&self) -> bool {
        self.state.is_loaded()
    }

    /// Check if file watching is active
    pub fn is_watching(&self) -> bool {
        self.state.is_watching()
    }

    /// Update last access time
    pub fn update_access(&mut self) {
        self.last_access = Instant::now();
    }

    /// Check if project has been idle longer than the configured timeout
    pub fn is_idle(&self) -> bool {
        let elapsed = self.last_access.elapsed().as_secs();
        elapsed > self.config.idle_timeout_secs
    }

    /// Start loading the project
    pub fn start_loading(&mut self) {
        self.state = ProjectState::loading();
        self.cancel_flag
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Update loading progress
    pub fn update_progress(&mut self, scanned: usize, current_file: Option<String>) {
        self.state.update_progress(scanned, current_file);
    }

    /// Set total files to scan
    pub fn set_total_files(&mut self, total: usize) {
        self.state.set_total_files(total);
    }

    /// Complete loading with results
    pub fn complete_loading(
        &mut self,
        file_count: usize,
        symbol_count: usize,
        dependency_count: usize,
    ) {
        self.state = ProjectState::loaded(file_count, symbol_count, dependency_count);
    }

    /// Fail loading with error
    pub fn fail_loading(&mut self, error: impl Into<String>) {
        self.state = ProjectState::failed(error);
    }

    /// Pause file watching
    pub fn pause_watching(&mut self, reason: impl Into<String>) {
        self.state.pause_watching(reason);
    }

    /// Resume file watching
    pub fn resume_watching(&mut self) {
        self.state.resume_watching();
        self.update_access();
    }

    /// Request cancellation of current operation
    pub fn request_cancel(&self) {
        self.cancel_flag
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Check if cancellation was requested
    pub fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Reset cancellation flag
    pub fn reset_cancel(&self) {
        self.cancel_flag
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Transition to a new state with validation
    pub fn transition_to(&mut self, new_state: ProjectState) -> Result<(), ProjectError> {
        // Validate transition
        let valid = match (&self.state, &new_state) {
            // NotLoaded can transition to Loading
            (ProjectState::NotLoaded, ProjectState::Loading { .. }) => true,
            // Loading can transition to Loaded or Failed
            (ProjectState::Loading { .. }, ProjectState::Loaded { .. }) => true,
            (ProjectState::Loading { .. }, ProjectState::Failed { .. }) => true,
            // Loaded can transition to WatchPaused
            (ProjectState::Loaded { .. }, ProjectState::WatchPaused { .. }) => true,
            // WatchPaused can transition to Loaded
            (ProjectState::WatchPaused { .. }, ProjectState::Loaded { .. }) => true,
            // Loaded can transition back to Loading (for rebuild)
            (ProjectState::Loaded { .. }, ProjectState::Loading { .. }) => true,
            // WatchPaused can transition back to Loading (for rebuild)
            (ProjectState::WatchPaused { .. }, ProjectState::Loading { .. }) => true,
            // Failed can transition back to Loading (for retry)
            (ProjectState::Failed { .. }, ProjectState::Loading { .. }) => true,
            // NotLoaded can transition to Failed (if registration fails)
            (ProjectState::NotLoaded, ProjectState::Failed { .. }) => true,
            _ => false,
        };

        if !valid {
            return Err(ProjectError::InvalidStateTransition {
                from: format!("{:?}", self.state),
                to: format!("{:?}", new_state),
            });
        }

        self.state = new_state;
        Ok(())
    }

    /// Get file count if available
    pub fn file_count(&self) -> Option<usize> {
        self.state.file_count()
    }

    /// Get symbol count if available
    pub fn symbol_count(&self) -> Option<usize> {
        self.state.symbol_count()
    }

    /// Get dependency count if available
    pub fn dependency_count(&self) -> Option<usize> {
        self.state.dependency_count()
    }
}

impl Clone for Project {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            path: self.path.clone(),
            name: self.name.clone(),
            state: self.state.clone(),
            config: self.config.clone(),
            last_access: self.last_access,
            cancel_flag: self.cancel_flag.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_project_new() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let project =
            Project::new(temp_dir.path(), "test-project", None).expect("Failed to create project");

        assert!(!project.id.as_str().is_empty());
        assert_eq!(project.name, "test-project");
        assert!(project.needs_loading());
        assert!(!project.is_loading());
        assert!(!project.is_loaded());
    }

    #[test]
    fn test_project_nonexistent_path() {
        let result = Project::new("/nonexistent/path", "test-project", None);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ProjectError::PathNotFound(_)));
    }

    #[test]
    fn test_project_config_default() {
        let config = ProjectConfig::default();
        assert!(config.include.contains(&"**/*".to_string()));
        assert!(!config.include_tests);
        assert!(config.parser_map.is_empty());
        assert_eq!(config.idle_timeout_secs, 300);
        assert!(config.languages.contains(&"python".to_string()));
    }

    #[test]
    fn test_project_config_custom() {
        let parser_map = HashMap::from([(".vue".to_string(), "typescript".to_string())]);
        let config = ProjectConfig::default()
            .with_include(vec!["lib/**".to_string()])
            .with_tests(true)
            .with_parser_map(parser_map.clone())
            .with_idle_timeout(600);

        assert!(config.include.contains(&"lib/**".to_string()));
        assert!(config.include_tests);
        assert_eq!(config.parser_map, parser_map);
        assert_eq!(config.idle_timeout_secs, 600);
    }

    #[test]
    fn test_project_paths() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let project =
            Project::new(temp_dir.path(), "test-project", None).expect("Failed to create project");

        assert!(project.cache_dir().ends_with(crate::CACHE_DIR));
        assert!(project.database_path().ends_with(crate::DB_FILE));
        assert!(project.manifest_path().ends_with(crate::MANIFEST_FILE));
    }

    #[test]
    fn test_project_state_transitions() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let mut project =
            Project::new(temp_dir.path(), "test-project", None).expect("Failed to create project");

        // NotLoaded -> Loading
        project.start_loading();
        assert!(project.is_loading());

        // Loading -> Loaded
        project.complete_loading(100, 500, 1000);
        assert!(project.is_loaded());
        assert!(project.is_watching());
        assert_eq!(project.file_count(), Some(100));

        // Loaded -> WatchPaused
        project.pause_watching("Idle timeout");
        assert!(project.is_loaded());
        assert!(!project.is_watching());

        // WatchPaused -> Loaded
        project.resume_watching();
        assert!(project.is_loaded());
        assert!(project.is_watching());
    }

    #[test]
    fn test_project_invalid_transition() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let mut project =
            Project::new(temp_dir.path(), "test-project", None).expect("Failed to create project");

        // Try invalid transition: NotLoaded -> Loaded (should fail)
        let result = project.transition_to(ProjectState::loaded(100, 500, 1000));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ProjectError::InvalidStateTransition { .. }
        ));
    }

    #[test]
    fn test_project_cancel() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let project =
            Project::new(temp_dir.path(), "test-project", None).expect("Failed to create project");

        assert!(!project.is_cancelled());

        project.request_cancel();
        assert!(project.is_cancelled());

        project.reset_cancel();
        assert!(!project.is_cancelled());
    }

    #[test]
    fn test_project_clone_shares_cancel_flag() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let project =
            Project::new(temp_dir.path(), "test-project", None).expect("Failed to create project");
        let cloned = project.clone();

        project.request_cancel();

        assert!(cloned.is_cancelled());
    }

    #[test]
    fn test_project_progress() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let mut project =
            Project::new(temp_dir.path(), "test-project", None).expect("Failed to create project");

        project.start_loading();
        project.set_total_files(100);
        project.update_progress(50, Some("src/main.rs".to_string()));

        if let ProjectState::Loading { scanned_files, .. } = &project.state {
            assert_eq!(*scanned_files, 50);
        } else {
            panic!("Expected Loading state");
        }
    }
}
