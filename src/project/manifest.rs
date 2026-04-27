//! Project manifest management
//!
//! Manifest stores project metadata in a JSON file for persistence across sessions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::project::ProjectId;

/// Error type for manifest operations
#[derive(Debug, Error)]
pub enum ManifestError {
    /// Failed to read manifest file
    #[error("Failed to read manifest file: {0}")]
    ReadError(#[source] std::io::Error),

    /// Failed to write manifest file
    #[error("Failed to write manifest file: {0}")]
    WriteError(#[source] std::io::Error),

    /// Failed to parse manifest JSON
    #[error("Failed to parse manifest JSON: {0}")]
    ParseError(#[source] serde_json::Error),

    /// Failed to serialize manifest to JSON
    #[error("Failed to serialize manifest to JSON: {0}")]
    SerializeError(#[source] serde_json::Error),

    /// Manifest directory does not exist
    #[error("Manifest directory does not exist: {0}")]
    DirectoryNotFound(PathBuf),

    /// Failed to create manifest directory
    #[error("Failed to create manifest directory: {0}")]
    CreateDirectoryError(#[source] std::io::Error),
}

/// Project manifest containing metadata about known projects
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Version of the manifest format
    pub version: u32,

    /// List of known projects
    pub projects: Vec<ProjectEntry>,

    /// Last updated timestamp (Unix timestamp in seconds)
    pub updated_at: u64,
}

impl Manifest {
    /// Current manifest version
    const VERSION: u32 = 1;

    /// Create a new empty manifest
    pub fn new() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            version: Self::VERSION,
            projects: Vec::new(),
            updated_at: now,
        }
    }

    /// Load manifest from a file
    ///
    /// If the file doesn't exist, returns a new empty manifest
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let path = path.as_ref();

        if !path.exists() {
            return Ok(Self::new());
        }

        let content = std::fs::read_to_string(path).map_err(ManifestError::ReadError)?;
        let manifest: Manifest =
            serde_json::from_str(&content).map_err(ManifestError::ParseError)?;

        Ok(manifest)
    }

    /// Save manifest to a file
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ManifestError> {
        let path = path.as_ref();

        // Ensure parent directory exists
        let parent = path.parent();
        if let Some(parent) = parent {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(ManifestError::CreateDirectoryError)?;
            }
        }

        let content = serde_json::to_string_pretty(self).map_err(ManifestError::SerializeError)?;
        std::fs::write(path, content).map_err(ManifestError::WriteError)?;

        Ok(())
    }

    /// Add a project entry
    pub fn add_project(&mut self, entry: ProjectEntry) {
        // Remove existing entry with same ID if present
        self.projects.retain(|p| p.id != entry.id);

        self.projects.push(entry);
        self.update_timestamp();
    }

    /// Remove a project by ID
    pub fn remove_project(&mut self, id: &ProjectId) {
        self.projects.retain(|p| p.id != *id);
        self.update_timestamp();
    }

    /// Remove projects whose root paths no longer exist.
    ///
    /// Returns the IDs that were removed.
    pub fn prune_missing_projects(&mut self) -> Vec<ProjectId> {
        let mut removed = Vec::new();
        self.projects.retain(|entry| {
            let exists = Path::new(&entry.path).exists();
            if !exists {
                removed.push(entry.id.clone());
            }
            exists
        });
        if !removed.is_empty() {
            self.update_timestamp();
        }
        removed
    }

    /// Get a project by ID
    pub fn get_project(&self, id: &ProjectId) -> Option<&ProjectEntry> {
        self.projects.iter().find(|p| p.id == *id)
    }

    /// Check if a project exists
    pub fn contains_project(&self, id: &ProjectId) -> bool {
        self.projects.iter().any(|p| p.id == *id)
    }

    /// List all project IDs
    pub fn project_ids(&self) -> Vec<ProjectId> {
        self.projects.iter().map(|p| p.id.clone()).collect()
    }

    /// Update timestamp
    fn update_timestamp(&mut self) {
        self.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
    }
}

impl Default for Manifest {
    fn default() -> Self {
        Self::new()
    }
}

/// Entry for a single project in the manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    /// Project ID (derived from canonical path)
    pub id: ProjectId,

    /// Project name (human-readable)
    pub name: String,

    /// Canonical path to project root
    pub path: String,

    /// Timestamp when project was registered
    pub registered_at: u64,

    /// Timestamp when project was last accessed
    pub last_accessed: u64,

    /// Timestamp when project was last scanned
    pub last_scanned: Option<u64>,

    /// Number of files in project
    pub file_count: usize,

    /// Number of symbols in project
    pub symbol_count: usize,

    /// Number of dependencies in project
    pub dependency_count: usize,

    /// Project configuration (optional)
    pub config: Option<ProjectConfig>,
}

impl ProjectEntry {
    /// Create a new project entry
    pub fn new(id: ProjectId, name: String, path: impl AsRef<Path>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            id,
            name,
            path: path.as_ref().to_string_lossy().to_string(),
            registered_at: now,
            last_accessed: now,
            last_scanned: None,
            file_count: 0,
            symbol_count: 0,
            dependency_count: 0,
            config: None,
        }
    }

    /// Update access timestamp
    pub fn update_accessed(&mut self) {
        self.last_accessed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
    }

    /// Update scan timestamp and counts
    pub fn update_scanned(
        &mut self,
        file_count: usize,
        symbol_count: usize,
        dependency_count: usize,
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.last_scanned = Some(now);
        self.file_count = file_count;
        self.symbol_count = symbol_count;
        self.dependency_count = dependency_count;
    }
}

/// Project configuration stored in manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Include patterns for file scanning
    pub include: Vec<String>,

    /// Exclude patterns for file scanning
    pub exclude: Vec<String>,

    /// Languages to scan
    pub languages: Vec<String>,

    /// Whether to include test files
    pub include_tests: bool,

    /// Parser extension overrides
    #[serde(default)]
    pub parser_map: HashMap<String, String>,
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
        }
    }
}

/// Helper to get manifest path for a project
pub fn get_manifest_path(project_root: impl AsRef<Path>) -> PathBuf {
    project_root
        .as_ref()
        .join(crate::CACHE_DIR)
        .join(crate::MANIFEST_FILE)
}

/// Helper to get database path for a project
pub fn get_database_path(project_root: impl AsRef<Path>) -> PathBuf {
    project_root
        .as_ref()
        .join(crate::CACHE_DIR)
        .join(crate::DB_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_manifest_new() {
        let manifest = Manifest::new();
        assert_eq!(manifest.version, 1);
        assert!(manifest.projects.is_empty());
    }

    #[test]
    fn test_manifest_add_remove() {
        let mut manifest = Manifest::new();
        let id = ProjectId::from_string("a1b2c3d4e5f6a7b8");
        let entry = ProjectEntry::new(id.clone(), "test-project".to_string(), "/path/to/project");

        manifest.add_project(entry);
        assert_eq!(manifest.projects.len(), 1);
        assert!(manifest.contains_project(&id));

        manifest.remove_project(&id);
        assert!(manifest.projects.is_empty());
    }

    #[test]
    fn test_manifest_get_project() {
        let mut manifest = Manifest::new();
        let id = ProjectId::from_string("a1b2c3d4e5f6a7b8");
        let entry = ProjectEntry::new(id.clone(), "test-project".to_string(), "/path/to/project");

        manifest.add_project(entry);
        let retrieved = manifest.get_project(&id).unwrap();
        assert_eq!(retrieved.name, "test-project");
    }

    #[test]
    fn test_manifest_prune_missing_projects() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let live_id = ProjectId::from_path(temp_dir.path()).expect("Failed to build id");

        let stale_dir = TempDir::new().expect("Failed to create stale dir");
        let stale_id = ProjectId::from_path(stale_dir.path()).expect("Failed to build stale id");
        let stale_path = stale_dir.path().to_path_buf();
        drop(stale_dir);

        let mut manifest = Manifest::new();
        manifest.add_project(ProjectEntry::new(
            live_id.clone(),
            "live-project".to_string(),
            temp_dir.path(),
        ));
        manifest.add_project(ProjectEntry::new(
            stale_id.clone(),
            "stale-project".to_string(),
            &stale_path,
        ));

        let removed = manifest.prune_missing_projects();
        assert_eq!(removed, vec![stale_id.clone()]);
        assert!(manifest.contains_project(&live_id));
        assert!(!manifest.contains_project(&stale_id));
    }

    #[test]
    fn test_manifest_save_load() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");

        let mut manifest = Manifest::new();
        let id = ProjectId::from_string("a1b2c3d4e5f6a7b8");
        let entry = ProjectEntry::new(id.clone(), "test-project".to_string(), "/path/to/project");
        manifest.add_project(entry);

        manifest
            .save(&manifest_path)
            .expect("Failed to save manifest");

        let loaded = Manifest::load(&manifest_path).expect("Failed to load manifest");
        assert_eq!(loaded.version, manifest.version);
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].name, "test-project");
    }

    #[test]
    fn test_manifest_load_nonexistent() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("nonexistent.json");

        let loaded = Manifest::load(&manifest_path).expect("Failed to load manifest");
        assert!(loaded.projects.is_empty());
    }

    #[test]
    fn test_project_entry() {
        let id = ProjectId::from_string("a1b2c3d4e5f6a7b8");
        let mut entry =
            ProjectEntry::new(id.clone(), "test-project".to_string(), "/path/to/project");

        assert_eq!(entry.id, id);
        assert_eq!(entry.name, "test-project");
        assert_eq!(entry.path, "/path/to/project");
        assert_eq!(entry.file_count, 0);

        entry.update_scanned(100, 500, 1000);
        assert_eq!(entry.file_count, 100);
        assert_eq!(entry.symbol_count, 500);
        assert!(entry.last_scanned.is_some());
    }

    #[test]
    fn test_project_config_default() {
        let config = ProjectConfig::default();
        assert!(config.include.contains(&"**/*".to_string()));
        assert!(config.exclude.contains(&"target/**".to_string()));
        assert!(!config.include_tests);
        assert!(config.languages.contains(&"go".to_string()));
    }

    #[test]
    fn test_get_manifest_path() {
        let path = get_manifest_path("/path/to/project");
        assert_eq!(
            path,
            PathBuf::from("/path/to/project/.quickdep/manifest.json")
        );
    }

    #[test]
    fn test_get_database_path() {
        let path = get_database_path("/path/to/project");
        assert_eq!(path, PathBuf::from("/path/to/project/.quickdep/symbols.db"));
    }
}
