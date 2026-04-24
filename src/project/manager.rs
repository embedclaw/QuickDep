//! Project Manager implementation
//!
//! Manages multiple projects with lazy loading and state transitions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

use crate::project::{
    Manifest, ManifestError, ManifestProjectConfig, Project, ProjectConfig, ProjectError,
    ProjectId, ProjectScanner, ProjectState, ScanError, ScanSummary,
};

/// Error type for project manager operations
#[derive(Debug, Error)]
pub enum ManagerError {
    /// Project error
    #[error("Project error: {0}")]
    Project(#[from] ProjectError),

    /// Manifest error
    #[error("Manifest error: {0}")]
    Manifest(#[from] ManifestError),

    /// Project not found
    #[error("Project not found: {0}")]
    NotFound(ProjectId),

    /// Project already registered
    #[error("Project already registered: {0}")]
    AlreadyRegistered(ProjectId),

    /// Failed to start scan
    #[error("Failed to start scan: {0}")]
    ScanFailed(String),

    /// Invalid operation for current state
    #[error("Invalid operation for current state: {0}")]
    InvalidOperation(String),

    /// Channel error
    #[error("Channel communication error")]
    ChannelError,
}

/// Message for background scan operations
#[derive(Debug)]
pub enum ScanMessage {
    /// Start scanning a project
    Start {
        project_id: ProjectId,
        /// Whether the caller requested a rebuild.
        rebuild: bool,
        /// Response channel for scan result
        result_tx: mpsc::Sender<ScanResult>,
    },
    /// Cancel scanning a project
    Cancel { project_id: ProjectId },
    /// Shutdown the scanner
    Shutdown,
}

/// Result of a scan operation
#[derive(Debug)]
pub struct ScanResult {
    /// Project ID that was scanned
    pub project_id: ProjectId,
    /// Number of files scanned
    pub file_count: usize,
    /// Number of symbols found
    pub symbol_count: usize,
    /// Number of dependencies found
    pub dependency_count: usize,
    /// Error message if failed
    pub error: Option<String>,
}

/// Project Manager - manages multiple projects with lazy loading
#[derive(Debug)]
pub struct ProjectManager {
    /// Managed projects (HashMap for fast lookup by ID)
    projects: Arc<RwLock<HashMap<ProjectId, Project>>>,
    /// Global manifest for persistence
    manifest: Arc<RwLock<Manifest>>,
    /// Manifest file path
    manifest_path: PathBuf,
    /// Scan task channel sender
    scan_tx: Option<mpsc::Sender<ScanMessage>>,
}

impl ProjectManager {
    /// Create a new project manager
    ///
    /// # Arguments
    /// * `manifest_path` - Path to the manifest file for persistence
    ///
    /// # Returns
    /// A new ProjectManager instance
    pub fn new(manifest_path: impl AsRef<Path>) -> Self {
        let manifest_path = manifest_path.as_ref().to_path_buf();

        // Load existing manifest or create new
        let manifest = Manifest::load(&manifest_path).unwrap_or_else(|e| {
            warn!(
                "Failed to load manifest at {}: {}",
                manifest_path.display(),
                e
            );
            Manifest::new()
        });
        let projects = Self::projects_from_manifest(&manifest);

        Self {
            projects: Arc::new(RwLock::new(projects)),
            manifest: Arc::new(RwLock::new(manifest)),
            manifest_path,
            scan_tx: None,
        }
    }

    /// Rebuild the in-memory project map from persisted manifest entries.
    fn projects_from_manifest(manifest: &Manifest) -> HashMap<ProjectId, Project> {
        let mut projects = HashMap::new();

        for entry in &manifest.projects {
            let config = entry
                .config
                .as_ref()
                .map(Self::project_config_from_manifest);
            match Project::new(&entry.path, entry.name.clone(), config) {
                Ok(project) => {
                    projects.insert(project.id.clone(), project);
                }
                Err(error) => {
                    warn!(
                        "Skipping manifest entry '{}' at {}: {}",
                        entry.id, entry.path, error
                    );
                }
            }
        }

        projects
    }

    /// Convert persisted manifest configuration into runtime project configuration.
    fn project_config_from_manifest(config: &ManifestProjectConfig) -> ProjectConfig {
        ProjectConfig {
            include: config.include.clone(),
            exclude: config.exclude.clone(),
            languages: config.languages.clone(),
            include_tests: config.include_tests,
            parser_map: config.parser_map.clone(),
            ..ProjectConfig::default()
        }
    }

    /// Convert runtime project configuration into persisted manifest configuration.
    fn manifest_config_from_project(config: &ProjectConfig) -> ManifestProjectConfig {
        ManifestProjectConfig {
            include: config.include.clone(),
            exclude: config.exclude.clone(),
            languages: config.languages.clone(),
            include_tests: config.include_tests,
            parser_map: config.parser_map.clone(),
        }
    }

    /// Create a new project manager with scan capability
    ///
    /// This starts a background task for handling scan operations
    pub async fn with_scanner(manifest_path: impl AsRef<Path>) -> Self {
        let mut manager = Self::new(manifest_path);

        // Create scan channel
        let (scan_tx, scan_rx) = mpsc::channel::<ScanMessage>(32);
        manager.scan_tx = Some(scan_tx);

        // Start background scan handler
        let projects = manager.projects.clone();
        tokio::spawn(Self::scan_handler(scan_rx, projects));

        manager
    }

    /// Background scan handler
    async fn scan_handler(
        mut scan_rx: mpsc::Receiver<ScanMessage>,
        projects: Arc<RwLock<HashMap<ProjectId, Project>>>,
    ) {
        debug!("Scan handler started");

        while let Some(msg) = scan_rx.recv().await {
            match msg {
                ScanMessage::Start {
                    project_id,
                    rebuild,
                    result_tx,
                } => {
                    debug!("Scan requested for project: {}", project_id);

                    let project = {
                        let projects_guard = projects.read().await;
                        projects_guard.get(&project_id).cloned()
                    };

                    let result = match project {
                        Some(project) => {
                            let mut scanner = ProjectScanner::new();
                            let scan_result = match scanner.discover_files(&project) {
                                Ok(files) => match if rebuild {
                                    Ok(None)
                                } else {
                                    scanner.plan_updates(&project, &files).map(Some)
                                } {
                                    Ok(update_plan) => {
                                        let files_to_parse = update_plan
                                            .as_ref()
                                            .map(|plan| {
                                                plan.updates
                                                    .iter()
                                                    .filter(|update| {
                                                        !matches!(
                                                            update.kind,
                                                            crate::watcher::UpdateKind::Deleted
                                                        )
                                                    })
                                                    .map(|update| update.absolute_path.clone())
                                                    .collect::<Vec<_>>()
                                            })
                                            .unwrap_or_else(|| files.clone());

                                        {
                                            let mut projects_guard = projects.write().await;
                                            if let Some(project) =
                                                projects_guard.get_mut(&project_id)
                                            {
                                                project.set_total_files(files_to_parse.len());
                                            }
                                        }

                                        let mut parsed_files =
                                            Vec::with_capacity(files_to_parse.len());
                                        let mut scan_error = None;

                                        for (index, file_path) in files_to_parse.iter().enumerate()
                                        {
                                            if project.is_cancelled() {
                                                scan_error = Some(ScanError::Cancelled);
                                                break;
                                            }

                                            match scanner.parse_file(&project, file_path) {
                                                Ok(parsed_file) => {
                                                    {
                                                        let mut projects_guard =
                                                            projects.write().await;
                                                        if let Some(project) =
                                                            projects_guard.get_mut(&project_id)
                                                        {
                                                            project.update_progress(
                                                                index + 1,
                                                                Some(parsed_file.file_path.clone()),
                                                            );
                                                        }
                                                    }
                                                    parsed_files.push(parsed_file);
                                                }
                                                Err(error) => {
                                                    scan_error = Some(error);
                                                    break;
                                                }
                                            }
                                        }

                                        if let Some(error) = scan_error {
                                            Err(error)
                                        } else {
                                            scanner.persist_scan(
                                                &project,
                                                rebuild,
                                                update_plan,
                                                parsed_files,
                                            )
                                        }
                                    }
                                    Err(error) => Err(error),
                                },
                                Err(error) => Err(error),
                            };

                            match scan_result {
                                Ok(ScanSummary {
                                    file_count,
                                    symbol_count,
                                    dependency_count,
                                }) => ScanResult {
                                    project_id: project_id.clone(),
                                    file_count,
                                    symbol_count,
                                    dependency_count,
                                    error: None,
                                },
                                Err(error) => ScanResult {
                                    project_id: project_id.clone(),
                                    file_count: 0,
                                    symbol_count: 0,
                                    dependency_count: 0,
                                    error: Some(error.to_string()),
                                },
                            }
                        }
                        None => ScanResult {
                            project_id: project_id.clone(),
                            file_count: 0,
                            symbol_count: 0,
                            dependency_count: 0,
                            error: Some("Project not found".to_string()),
                        },
                    };

                    if result_tx.send(result).await.is_err() {
                        warn!("Failed to send scan result");
                    }
                }
                ScanMessage::Cancel { project_id } => {
                    debug!("Scan cancelled for project: {}", project_id);
                    let mut projects_guard = projects.write().await;
                    if let Some(project) = projects_guard.get_mut(&project_id) {
                        project.request_cancel();
                    }
                }
                ScanMessage::Shutdown => {
                    debug!("Scan handler shutting down");
                    break;
                }
            }
        }
    }

    /// Register a new project
    ///
    /// # Arguments
    /// * `path` - Path to the project directory
    /// * `name` - Human-readable name for the project
    /// * `config` - Optional configuration
    ///
    /// # Returns
    /// * `Ok(ProjectId)` - The project ID
    /// * `Err(ManagerError)` - If registration fails
    pub async fn register(
        &self,
        path: impl AsRef<Path>,
        name: impl Into<String>,
        config: Option<ProjectConfig>,
    ) -> Result<ProjectId, ManagerError> {
        let path = path.as_ref();
        let name = name.into();

        // Create project
        let project = Project::new(path, name.clone(), config)?;
        let manifest_config = Self::manifest_config_from_project(&project.config);

        let id = project.id.clone();

        // Check if already registered
        {
            let projects = self.projects.read().await;
            if projects.contains_key(&id) {
                return Err(ManagerError::AlreadyRegistered(id));
            }
        }

        // Add to projects map
        {
            let mut projects = self.projects.write().await;
            projects.insert(id.clone(), project);
        }

        // Update manifest
        {
            let mut manifest = self.manifest.write().await;
            let mut entry = crate::project::ProjectEntry::new(id.clone(), name.clone(), path);
            entry.config = Some(manifest_config);
            manifest.add_project(entry);
            manifest.save(&self.manifest_path)?;
        }

        info!("Registered project: {} ({})", name.clone(), id);
        Ok(id)
    }

    /// Register a project or refresh an existing registration with the latest metadata.
    ///
    /// Existing registrations keep their historical scan counters while updating the
    /// runtime configuration and manifest entry to match the latest project settings.
    pub async fn register_or_update(
        &self,
        path: impl AsRef<Path>,
        name: impl Into<String>,
        config: Option<ProjectConfig>,
    ) -> Result<ProjectId, ManagerError> {
        let project = Project::new(path, name.into(), config)?;
        let id = project.id.clone();
        let name = project.name.clone();
        let project_path = project.path.to_string_lossy().to_string();
        let runtime_config = project.config.clone();
        let manifest_config = Self::manifest_config_from_project(&runtime_config);

        let updated_existing = {
            let mut projects = self.projects.write().await;
            if let Some(existing) = projects.get_mut(&id) {
                existing.name = name.clone();
                existing.path = project.path.clone();
                existing.config = runtime_config;
                true
            } else {
                projects.insert(id.clone(), project);
                false
            }
        };

        {
            let mut manifest = self.manifest.write().await;
            if let Some(entry) = manifest.projects.iter_mut().find(|entry| entry.id == id) {
                entry.name = name.clone();
                entry.path = project_path.clone();
                entry.config = Some(manifest_config);
                entry.update_accessed();
                manifest.updated_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);
            } else {
                let mut entry =
                    crate::project::ProjectEntry::new(id.clone(), name.clone(), &project_path);
                entry.config = Some(manifest_config);
                manifest.add_project(entry);
            }
            manifest.save(&self.manifest_path)?;
        }

        if updated_existing {
            info!("Updated project registration: {} ({})", name, id);
        } else {
            info!("Registered project: {} ({})", name, id);
        }

        Ok(id)
    }

    /// Unregister a project
    ///
    /// # Arguments
    /// * `id` - The project ID to unregister
    ///
    /// # Returns
    /// * `Ok(())` - Successfully unregistered
    /// * `Err(ManagerError)` - If project not found
    pub async fn unregister(&self, id: &ProjectId) -> Result<(), ManagerError> {
        // Remove from projects map
        {
            let mut projects = self.projects.write().await;
            if projects.remove(id).is_none() {
                return Err(ManagerError::NotFound(id.clone()));
            }
        }

        // Update manifest
        {
            let mut manifest = self.manifest.write().await;
            manifest.remove_project(id);
            manifest.save(&self.manifest_path)?;
        }

        info!("Unregistered project: {}", id);
        Ok(())
    }

    /// Get a project by ID
    ///
    /// This triggers lazy loading if the project is in NotLoaded state
    pub async fn get(&self, id: &ProjectId) -> Result<Option<Project>, ManagerError> {
        enum AccessAction {
            Scan,
            ResumeWatch,
            Touch,
        }

        let action = {
            let projects = self.projects.read().await;
            match projects.get(id) {
                Some(project) if project.needs_loading() => AccessAction::Scan,
                Some(project) if project.is_loaded() && !project.is_watching() => {
                    AccessAction::ResumeWatch
                }
                Some(_) => AccessAction::Touch,
                None => return Err(ManagerError::NotFound(id.clone())),
            }
        };

        match action {
            AccessAction::Scan => {
                self.trigger_scan(id, false).await?;
                self.touch(id).await?;
            }
            AccessAction::ResumeWatch => {
                self.resume_watch(id).await?;
            }
            AccessAction::Touch => {
                self.touch(id).await?;
            }
        }

        let projects = self.projects.read().await;
        Ok(projects.get(id).cloned())
    }

    /// Trigger a full scan for a project.
    pub async fn scan(&self, id: &ProjectId, rebuild: bool) -> Result<(), ManagerError> {
        self.trigger_scan(id, rebuild).await
    }

    /// Get the current scan status for a project.
    pub async fn status(&self, id: &ProjectId) -> Result<ProjectState, ManagerError> {
        let projects = self.projects.read().await;
        projects
            .get(id)
            .map(|project| project.state.clone())
            .ok_or_else(|| ManagerError::NotFound(id.clone()))
    }

    /// Trigger a scan for a project (lazy loading)
    async fn trigger_scan(&self, id: &ProjectId, rebuild: bool) -> Result<(), ManagerError> {
        // Start loading state
        {
            let mut projects = self.projects.write().await;
            match projects.get_mut(id) {
                Some(project) if project.is_loading() => {
                    return Err(ManagerError::InvalidOperation(format!(
                        "project {} is already loading",
                        id
                    )));
                }
                Some(project) => project.start_loading(),
                None => return Err(ManagerError::NotFound(id.clone())),
            }
        }

        // Send scan message if scanner is available
        if let Some(scan_tx) = &self.scan_tx {
            let (result_tx, mut result_rx) = mpsc::channel::<ScanResult>(1);

            scan_tx
                .send(ScanMessage::Start {
                    project_id: id.clone(),
                    rebuild,
                    result_tx,
                })
                .await
                .map_err(|_| ManagerError::ChannelError)?;

            // Wait for scan result
            if let Some(result) = result_rx.recv().await {
                let mut projects = self.projects.write().await;
                if let Some(project) = projects.get_mut(&result.project_id) {
                    if let Some(ref error) = result.error {
                        project.fail_loading(error.clone());
                    } else {
                        project.complete_loading(
                            result.file_count,
                            result.symbol_count,
                            result.dependency_count,
                        );
                    }
                }

                if result.error.is_none() {
                    let mut manifest = self.manifest.write().await;
                    if let Some(entry) = manifest
                        .projects
                        .iter_mut()
                        .find(|entry| entry.id == result.project_id)
                    {
                        entry.update_scanned(
                            result.file_count,
                            result.symbol_count,
                            result.dependency_count,
                        );
                    }
                    manifest.save(&self.manifest_path)?;
                }
            }
        } else {
            // No scanner - mark as failed
            let mut projects = self.projects.write().await;
            if let Some(project) = projects.get_mut(id) {
                project.fail_loading("Scanner not available");
            }
        }

        Ok(())
    }

    /// List all registered project IDs
    pub async fn list_ids(&self) -> Vec<ProjectId> {
        let projects = self.projects.read().await;
        projects.keys().cloned().collect()
    }

    /// List all projects with their current state
    pub async fn list(&self) -> Vec<(ProjectId, ProjectState)> {
        let projects = self.projects.read().await;
        projects
            .iter()
            .map(|(id, p)| (id.clone(), p.state.clone()))
            .collect()
    }

    /// Check if a project exists
    pub async fn exists(&self, id: &ProjectId) -> bool {
        let projects = self.projects.read().await;
        projects.contains_key(id)
    }

    /// Get the count of managed projects
    pub async fn count(&self) -> usize {
        let projects = self.projects.read().await;
        projects.len()
    }

    /// Pause watching for a project
    pub async fn pause_watch(
        &self,
        id: &ProjectId,
        reason: impl Into<String>,
    ) -> Result<(), ManagerError> {
        let mut projects = self.projects.write().await;
        match projects.get_mut(id) {
            Some(project) => {
                project.pause_watching(reason);
                debug!("Paused watching for project: {}", id);
                Ok(())
            }
            None => Err(ManagerError::NotFound(id.clone())),
        }
    }

    /// Resume watching for a project
    pub async fn resume_watch(&self, id: &ProjectId) -> Result<(), ManagerError> {
        let mut projects = self.projects.write().await;
        match projects.get_mut(id) {
            Some(project) => {
                project.resume_watching();
                debug!("Resumed watching for project: {}", id);
                Ok(())
            }
            None => Err(ManagerError::NotFound(id.clone())),
        }
    }

    /// Update access time for a project (prevents idle timeout)
    pub async fn touch(&self, id: &ProjectId) -> Result<(), ManagerError> {
        let mut projects = self.projects.write().await;
        match projects.get_mut(id) {
            Some(project) => {
                project.update_access();
                Ok(())
            }
            None => Err(ManagerError::NotFound(id.clone())),
        }
    }

    /// Check for idle projects and pause their watchers
    pub async fn check_idle(&self) -> Vec<ProjectId> {
        let mut paused = Vec::new();
        let mut projects = self.projects.write().await;

        for (id, project) in projects.iter_mut() {
            if project.is_watching() && project.is_idle() {
                project.pause_watching("Idle timeout");
                paused.push(id.clone());
                debug!("Paused idle project: {}", id);
            }
        }

        paused
    }

    /// Cancel an ongoing scan
    pub async fn cancel_scan(&self, id: &ProjectId) -> Result<(), ManagerError> {
        if let Some(scan_tx) = &self.scan_tx {
            scan_tx
                .send(ScanMessage::Cancel {
                    project_id: id.clone(),
                })
                .await
                .map_err(|_| ManagerError::ChannelError)?;
        }

        let mut projects = self.projects.write().await;
        if let Some(project) = projects.get_mut(id) {
            project.request_cancel();
        }

        Ok(())
    }

    /// Shutdown the manager
    pub async fn shutdown(&self) {
        if let Some(scan_tx) = &self.scan_tx {
            if scan_tx.send(ScanMessage::Shutdown).await.is_err() {
                warn!("Failed to send shutdown message");
            }
        }

        // Save manifest
        let manifest = self.manifest.read().await;
        if let Err(e) = manifest.save(&self.manifest_path) {
            error!("Failed to save manifest on shutdown: {}", e);
        } else {
            info!("Manifest saved on shutdown");
        }
    }

    /// Get the manifest
    pub async fn get_manifest(&self) -> Manifest {
        self.manifest.read().await.clone()
    }

    /// Reload manifest from file
    pub async fn reload_manifest(&self) -> Result<(), ManagerError> {
        let new_manifest = Manifest::load(&self.manifest_path)?;
        let mut manifest = self.manifest.write().await;
        *manifest = new_manifest;
        Ok(())
    }
}

impl Clone for ProjectManager {
    fn clone(&self) -> Self {
        Self {
            projects: self.projects.clone(),
            manifest: self.manifest.clone(),
            manifest_path: self.manifest_path.clone(),
            scan_tx: self.scan_tx.clone(),
        }
    }
}

/// Idle check task - periodically checks for idle projects
pub async fn start_idle_checker(manager: ProjectManager, interval: Duration) {
    loop {
        tokio::time::sleep(interval).await;

        let paused = manager.check_idle().await;
        if !paused.is_empty() {
            info!("Paused {} idle projects", paused.len());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_manager_register() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");
        let manager = ProjectManager::new(&manifest_path);

        let project_dir = TempDir::new().expect("Failed to create project dir");
        let id = manager
            .register(project_dir.path(), "test-project", None)
            .await
            .expect("Failed to register");

        assert!(manager.exists(&id).await);
        assert_eq!(manager.count().await, 1);
    }

    #[tokio::test]
    async fn test_manager_unregister() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");
        let manager = ProjectManager::new(&manifest_path);

        let project_dir = TempDir::new().expect("Failed to create project dir");
        let id = manager
            .register(project_dir.path(), "test-project", None)
            .await
            .expect("Failed to register");

        manager.unregister(&id).await.expect("Failed to unregister");
        assert!(!manager.exists(&id).await);
        assert_eq!(manager.count().await, 0);
    }

    #[tokio::test]
    async fn test_manager_duplicate_register() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");
        let manager = ProjectManager::new(&manifest_path);

        let project_dir = TempDir::new().expect("Failed to create project dir");
        manager
            .register(project_dir.path(), "test-project", None)
            .await
            .expect("Failed to register");

        // Try to register again
        let result = manager
            .register(project_dir.path(), "test-project", None)
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ManagerError::AlreadyRegistered(_)
        ));
    }

    #[tokio::test]
    async fn test_manager_list() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");
        let manager = ProjectManager::new(&manifest_path);

        let project_dir1 = TempDir::new().expect("Failed to create project dir");
        let project_dir2 = TempDir::new().expect("Failed to create project dir");

        manager
            .register(project_dir1.path(), "project-1", None)
            .await
            .expect("Failed to register");
        manager
            .register(project_dir2.path(), "project-2", None)
            .await
            .expect("Failed to register");

        let ids = manager.list_ids().await;
        assert_eq!(ids.len(), 2);

        let list = manager.list().await;
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn test_manager_pause_resume() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");
        let manager = ProjectManager::new(&manifest_path);

        let project_dir = TempDir::new().expect("Failed to create project dir");
        let id = manager
            .register(project_dir.path(), "test-project", None)
            .await
            .expect("Failed to register");

        // Set to loaded state for pause/resume test
        {
            let mut projects = manager.projects.write().await;
            if let Some(project) = projects.get_mut(&id) {
                project.complete_loading(100, 500, 1000);
            }
        }

        manager
            .pause_watch(&id, "Test pause")
            .await
            .expect("Failed to pause");

        let project = manager
            .get(&id)
            .await
            .expect("Failed to get")
            .expect("Project not found");
        assert!(project.is_watching());

        manager
            .pause_watch(&id, "Test pause")
            .await
            .expect("Failed to pause");

        manager.resume_watch(&id).await.expect("Failed to resume");

        let project = manager
            .get(&id)
            .await
            .expect("Failed to get")
            .expect("Project not found");
        assert!(project.is_watching());
    }

    #[tokio::test]
    async fn test_manager_touch() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");
        let manager = ProjectManager::new(&manifest_path);

        let project_dir = TempDir::new().expect("Failed to create project dir");
        let id = manager
            .register(project_dir.path(), "test-project", None)
            .await
            .expect("Failed to register");

        manager.touch(&id).await.expect("Failed to touch");
    }

    #[tokio::test]
    async fn test_manager_manifest_persistence() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");

        let project_dir = TempDir::new().expect("Failed to create project dir");
        let id;

        // Create manager and register project
        {
            let manager = ProjectManager::new(&manifest_path);
            id = manager
                .register(project_dir.path(), "test-project", None)
                .await
                .expect("Failed to register");
        }

        // Create new manager - should load existing manifest
        {
            let manager = ProjectManager::new(&manifest_path);
            assert!(manager.exists(&id).await);
        }
    }

    #[tokio::test]
    async fn test_manager_not_found() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");
        let manager = ProjectManager::new(&manifest_path);

        let fake_id = ProjectId::from_string("a1b2c3d4e5f6a7b8");
        let result = manager.unregister(&fake_id).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ManagerError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_manager_with_scanner_loads_project_graph() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");
        let manager = ProjectManager::with_scanner(&manifest_path).await;

        let project_dir = TempDir::new().expect("Failed to create project dir");
        std::fs::create_dir_all(project_dir.path().join("src")).expect("Failed to create src dir");
        std::fs::write(
            project_dir.path().join("src/main.rs"),
            "use crate::utils::helper;\nfn main() { helper(); }\n",
        )
        .expect("Failed to write main.rs");
        std::fs::write(
            project_dir.path().join("src/utils.rs"),
            "pub fn helper() {}\n",
        )
        .expect("Failed to write utils.rs");

        let id = manager
            .register(
                project_dir.path(),
                "scan-project",
                Some(ProjectConfig::default().with_languages(vec!["rust".to_string()])),
            )
            .await
            .expect("Failed to register");

        manager.scan(&id, false).await.expect("Failed to scan");
        let project = manager
            .get(&id)
            .await
            .expect("Failed to get project")
            .expect("Project not found");

        assert!(project.is_loaded());
        assert_eq!(project.file_count(), Some(2));
        assert_eq!(project.symbol_count(), Some(2));
        assert_eq!(project.dependency_count(), Some(1));

        let storage =
            crate::storage::Storage::new(&project.database_path()).expect("Failed to open storage");
        assert_eq!(storage.count_symbols().unwrap(), 2);
        assert_eq!(storage.count_dependencies().unwrap(), 1);
        assert_eq!(storage.count_imports().unwrap(), 1);

        let main_symbol = storage
            .get_symbol_by_qualified_name("src/main.rs::main")
            .unwrap()
            .expect("main symbol missing");
        let chain = storage
            .get_dependency_chain_forward(&main_symbol.id, 1)
            .expect("Failed to query dependency chain");

        assert_eq!(chain.len(), 2);
        assert!(chain
            .iter()
            .any(|node| node.qualified_name == "src/utils.rs::helper"));

        manager.shutdown().await;
    }

    #[tokio::test]
    async fn test_manager_scan_supports_mixed_language_projects() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");
        let manager = ProjectManager::with_scanner(&manifest_path).await;

        let project_dir = TempDir::new().expect("Failed to create project dir");
        std::fs::create_dir_all(project_dir.path().join("src")).expect("Failed to create src dir");
        std::fs::write(
            project_dir.path().join("src/lib.rs"),
            "pub fn rust_entry() { rust_helper(); }\npub fn rust_helper() {}\n",
        )
        .expect("Failed to write lib.rs");
        std::fs::write(
            project_dir.path().join("src/service.ts"),
            "export function tsEntry() { return tsHelper(); }\nfunction tsHelper() { return 1; }\n",
        )
        .expect("Failed to write service.ts");
        std::fs::write(
            project_dir.path().join("src/tasks.py"),
            "def py_entry():\n    return py_helper()\n\ndef py_helper():\n    return 'ok'\n",
        )
        .expect("Failed to write tasks.py");

        let id = manager
            .register(
                project_dir.path(),
                "mixed-language-project",
                Some(
                    ProjectConfig::default()
                        .with_include(vec!["src/**".to_string()])
                        .with_languages(vec![
                            "rust".to_string(),
                            "typescript".to_string(),
                            "python".to_string(),
                        ]),
                ),
            )
            .await
            .expect("Failed to register");

        manager.scan(&id, false).await.expect("Failed to scan");
        let project = manager
            .get(&id)
            .await
            .expect("Failed to get project")
            .expect("Project not found");
        let storage =
            crate::storage::Storage::new(&project.database_path()).expect("Failed to open storage");

        assert_eq!(storage.count_file_states().unwrap(), 3);
        assert_eq!(storage.count_symbols().unwrap(), 6);
        assert_eq!(storage.count_dependencies().unwrap(), 3);

        manager.shutdown().await;
    }

    #[tokio::test]
    async fn test_manager_scan_resolves_typescript_and_python_import_aliases() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");
        let manager = ProjectManager::with_scanner(&manifest_path).await;

        let project_dir = TempDir::new().expect("Failed to create project dir");
        std::fs::create_dir_all(project_dir.path().join("src")).expect("Failed to create src dir");
        std::fs::write(
            project_dir.path().join("src/shared.ts"),
            "export function formatName(name: string): string { return name.trim(); }\n",
        )
        .expect("Failed to write shared.ts");
        std::fs::write(
            project_dir.path().join("src/main.ts"),
            "import { formatName as format } from './shared';\nexport function run() { return format('ok'); }\n",
        )
        .expect("Failed to write main.ts");
        std::fs::write(
            project_dir.path().join("src/helpers.py"),
            "def format_name(value):\n    return value.strip()\n",
        )
        .expect("Failed to write helpers.py");
        std::fs::write(
            project_dir.path().join("src/tasks.py"),
            "from helpers import format_name as helper\n\ndef run():\n    return helper('ok')\n",
        )
        .expect("Failed to write tasks.py");

        let id = manager
            .register(
                project_dir.path(),
                "import-alias-project",
                Some(
                    ProjectConfig::default()
                        .with_include(vec!["src/**".to_string()])
                        .with_languages(vec!["typescript".to_string(), "python".to_string()]),
                ),
            )
            .await
            .expect("Failed to register");

        manager.scan(&id, false).await.expect("Failed to scan");
        let project = manager
            .get(&id)
            .await
            .expect("Failed to get project")
            .expect("Project not found");
        let storage =
            crate::storage::Storage::new(&project.database_path()).expect("Failed to open storage");

        let ts_run = storage
            .get_symbol_by_qualified_name("src/main.ts::run")
            .unwrap()
            .expect("TypeScript run symbol missing");
        let py_run = storage
            .get_symbol_by_qualified_name("src/tasks.py::run")
            .unwrap()
            .expect("Python run symbol missing");

        let ts_chain = storage
            .get_dependency_chain_forward(&ts_run.id, 1)
            .expect("Failed to query TypeScript dependency chain");
        let py_chain = storage
            .get_dependency_chain_forward(&py_run.id, 1)
            .expect("Failed to query Python dependency chain");

        assert!(ts_chain
            .iter()
            .any(|symbol| symbol.qualified_name == "src/shared.ts::formatName"));
        assert!(py_chain
            .iter()
            .any(|symbol| symbol.qualified_name == "src/helpers.py::format_name"));

        manager.shutdown().await;
    }

    #[tokio::test]
    async fn test_manager_clone_preserves_scan_sender() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");
        let manager = ProjectManager::with_scanner(&manifest_path).await;
        let clone = manager.clone();

        let project_dir = TempDir::new().expect("Failed to create project dir");
        std::fs::create_dir_all(project_dir.path().join("src")).expect("Failed to create src dir");
        std::fs::write(
            project_dir.path().join("src/lib.rs"),
            "pub fn helper() {}\n",
        )
        .expect("Failed to write lib.rs");

        let id = manager
            .register(
                project_dir.path(),
                "cloned-scan-project",
                Some(ProjectConfig::default().with_languages(vec!["rust".to_string()])),
            )
            .await
            .expect("Failed to register");

        clone
            .scan(&id, false)
            .await
            .expect("Failed to scan via clone");

        let project = manager
            .get(&id)
            .await
            .expect("Failed to get project")
            .expect("Project not found");
        assert!(project.is_loaded());
        assert_eq!(project.symbol_count(), Some(1));
    }

    #[tokio::test]
    async fn test_manager_scan_updates_manifest_counts_and_config() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");
        let manager = ProjectManager::with_scanner(&manifest_path).await;

        let project_dir = TempDir::new().expect("Failed to create project dir");
        std::fs::create_dir_all(project_dir.path().join("src")).expect("Failed to create src dir");
        std::fs::write(
            project_dir.path().join("src/lib.rs"),
            "pub fn entry() { helper(); }\npub fn helper() {}\n",
        )
        .expect("Failed to write lib.rs");

        let config = ProjectConfig::default()
            .with_include(vec!["src/**".to_string()])
            .with_languages(vec!["rust".to_string()])
            .with_tests(true);
        let id = manager
            .register(project_dir.path(), "manifest-project", Some(config))
            .await
            .expect("Failed to register");

        manager.scan(&id, false).await.expect("Failed to scan");

        let manifest = manager.get_manifest().await;
        let entry = manifest.get_project(&id).expect("manifest entry missing");
        assert_eq!(entry.file_count, 1);
        assert_eq!(entry.symbol_count, 2);
        assert_eq!(entry.dependency_count, 1);
        assert!(entry.last_scanned.is_some());
        assert_eq!(
            entry
                .config
                .as_ref()
                .expect("manifest config missing")
                .languages,
            vec!["rust".to_string()]
        );
        assert!(entry
            .config
            .as_ref()
            .expect("manifest config missing")
            .parser_map
            .is_empty());
    }

    #[tokio::test]
    async fn test_register_or_update_refreshes_project_config() {
        use std::collections::HashMap;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp_dir.path().join("manifest.json");
        let manager = ProjectManager::new(&manifest_path);

        let project_dir = TempDir::new().expect("Failed to create project dir");
        let id = manager
            .register(
                project_dir.path(),
                "config-project",
                Some(ProjectConfig::default().with_include(vec!["src/**".to_string()])),
            )
            .await
            .expect("Failed to register");

        let updated_id = manager
            .register_or_update(
                project_dir.path(),
                "config-project",
                Some(
                    ProjectConfig::default()
                        .with_include(vec!["examples/**".to_string()])
                        .with_languages(vec!["rust".to_string()])
                        .with_parser_map(HashMap::from([(
                            ".vue".to_string(),
                            "typescript".to_string(),
                        )])),
                ),
            )
            .await
            .expect("Failed to refresh registration");

        assert_eq!(id, updated_id);
        assert_eq!(manager.count().await, 1);

        let project = {
            let projects = manager.projects.read().await;
            projects.get(&id).cloned().expect("Project not found")
        };
        assert_eq!(project.config.include, vec!["examples/**".to_string()]);
        assert_eq!(project.config.languages, vec!["rust".to_string()]);
        assert_eq!(
            project.config.parser_map.get(".vue").map(String::as_str),
            Some("typescript")
        );

        let manifest = manager.get_manifest().await;
        let entry = manifest.get_project(&id).expect("manifest entry missing");
        assert_eq!(
            entry
                .config
                .as_ref()
                .expect("manifest config missing")
                .include,
            vec!["examples/**".to_string()]
        );
        assert_eq!(
            entry
                .config
                .as_ref()
                .expect("manifest config missing")
                .parser_map
                .get(".vue")
                .map(String::as_str),
            Some("typescript")
        );
    }
}
