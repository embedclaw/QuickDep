//! Project Manager Module
//!
//! This module manages multiple projects with lazy loading and state transitions.
//!
//! ## Overview
//!
//! The Project Manager is responsible for:
//! - Registering and tracking multiple code projects
//! - Generating unique project IDs from canonical paths (using blake3 hash)
//! - Managing project lifecycle states (NotLoaded, Loading, Loaded, WatchPaused, Failed)
//! - Implementing lazy loading - projects are only scanned when first accessed
//! - Persisting project metadata via manifest files
//!
//! ## Key Components
//!
//! - [`ProjectId`] - Unique identifier derived from project path hash
//! - [`ProjectState`] - Lifecycle states with progress tracking
//! - [`Project`] - A single project being analyzed
//! - [`ProjectManager`] - Manager for multiple projects with async support
//! - [`Manifest`] - JSON-based persistence for project metadata
//!
//! ## Example Usage
//!
//! ```ignore
//! use quickdep::project::{ProjectManager, ProjectId, ProjectConfig};
//!
//! // Create a project manager
//! let manager = ProjectManager::new(".quickdep/manifest.json");
//!
//! // Register a project
//! let id = manager.register("/path/to/project", "my-project", None).await?;
//!
//! // Get project (triggers lazy loading if needed)
//! let project = manager.get(&id).await?;
//!
//! // List all projects
//! let projects = manager.list().await;
//!
//! // Pause watching for idle project
//! manager.pause_watch(&id, "Idle timeout").await?;
//! ```

pub mod id;
pub mod manager;
pub mod manifest;
#[allow(clippy::module_inception)]
pub mod project;
pub mod scanner;
pub mod state;

// Re-export key types for convenience
pub use id::{ProjectId, ProjectIdError};
pub use manager::{start_idle_checker, ManagerError, ProjectManager, ScanMessage, ScanResult};
pub use manifest::{
    get_database_path, get_manifest_path, Manifest, ManifestError,
    ProjectConfig as ManifestProjectConfig, ProjectEntry,
};
pub use project::{Project, ProjectConfig, ProjectError};
pub use scanner::{ProjectScanner, ScanError, ScanSummary};
pub use state::{ProjectState, ScanProgress};

#[cfg(test)]
mod integration_tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_full_project_workflow() {
        // Create temp directories
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");

        // Create project manager
        let manager = ProjectManager::new(&manifest_path);

        // Create a project directory
        let project_dir = TempDir::new().expect("Failed to create project dir");

        // Register project
        let id = manager
            .register(project_dir.path(), "test-project", None)
            .await
            .expect("Failed to register");

        // Verify project exists
        assert!(manager.exists(&id).await);

        // Get project (lazy load triggered)
        let project = manager
            .get(&id)
            .await
            .expect("Failed to get")
            .expect("Project not found");

        // Verify project properties
        assert_eq!(project.name, "test-project");
        assert!(!project.id.as_str().is_empty());

        // List projects
        let list = manager.list().await;
        assert_eq!(list.len(), 1);

        // Unregister project
        manager.unregister(&id).await.expect("Failed to unregister");
        assert!(!manager.exists(&id).await);

        // Verify manifest was updated
        let manifest = manager.get_manifest().await;
        assert!(manifest.get_project(&id).is_none());
    }

    #[test]
    fn test_project_id_consistency() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Generate ID twice from same path
        let id1 = ProjectId::from_path(temp_dir.path()).expect("Failed to generate ID");
        let id2 = ProjectId::from_path(temp_dir.path()).expect("Failed to generate ID");

        // IDs should be identical
        assert_eq!(id1, id2);
        assert_eq!(id1.as_str(), id2.as_str());
    }

    #[test]
    fn test_state_transitions() {
        let state = ProjectState::not_loaded();
        assert!(!state.is_loaded());

        let loading_state = ProjectState::loading();
        assert!(loading_state.is_loading());

        let loaded_state = ProjectState::loaded(100, 500, 1000);
        assert!(loaded_state.is_loaded());
        assert!(loaded_state.is_watching());
        assert_eq!(loaded_state.file_count(), Some(100));

        let paused_state = ProjectState::watch_paused(100, 500, 1000, "Idle");
        assert!(paused_state.is_loaded());
        assert!(!paused_state.is_watching());
    }

    #[test]
    fn test_manifest_crud() {
        let mut manifest = Manifest::new();

        let id = ProjectId::from_string("a1b2c3d4e5f6a7b8");
        let entry = ProjectEntry::new(id.clone(), "test".to_string(), "/path");

        // Add
        manifest.add_project(entry);
        assert!(manifest.contains_project(&id));

        // Get
        let retrieved = manifest.get_project(&id).expect("Not found");
        assert_eq!(retrieved.name, "test");

        // Remove
        manifest.remove_project(&id);
        assert!(!manifest.contains_project(&id));
    }
}
