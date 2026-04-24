//! Project state management
//!
//! Defines the lifecycle states for projects managed by QuickDep.

use serde::{Deserialize, Serialize};

/// Project loading state
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectState {
    /// Project registered but not yet loaded
    #[default]
    NotLoaded,

    /// Project is currently being loaded/scanned
    Loading {
        /// Total number of files to scan (0 if unknown)
        total_files: usize,
        /// Number of files scanned so far
        scanned_files: usize,
        /// Current file being scanned (if any)
        current_file: Option<String>,
        /// Timestamp when loading started
        started_at: u64, // Unix timestamp in seconds
    },

    /// Project fully loaded and being watched for changes
    Loaded {
        /// Number of files in the project
        file_count: usize,
        /// Number of symbols in the project
        symbol_count: usize,
        /// Number of dependencies in the project
        dependency_count: usize,
        /// Timestamp when loading completed
        loaded_at: u64, // Unix timestamp in seconds
        /// Whether file watching is active
        watching: bool,
    },

    /// File watching is paused (e.g., after idle timeout)
    WatchPaused {
        /// Number of files in the project
        file_count: usize,
        /// Number of symbols in the project
        symbol_count: usize,
        /// Number of dependencies in the project
        dependency_count: usize,
        /// Timestamp when watching was paused
        paused_at: u64, // Unix timestamp in seconds
        /// Reason for pausing
        reason: String,
    },

    /// Project failed to load
    Failed {
        /// Error message
        error: String,
        /// Timestamp when failure occurred
        failed_at: u64, // Unix timestamp in seconds
    },
}

impl ProjectState {
    /// Create a new NotLoaded state
    pub fn not_loaded() -> Self {
        ProjectState::NotLoaded
    }

    /// Create a new Loading state
    pub fn loading() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        ProjectState::Loading {
            total_files: 0,
            scanned_files: 0,
            current_file: None,
            started_at: now,
        }
    }

    /// Create a new Loaded state
    pub fn loaded(file_count: usize, symbol_count: usize, dependency_count: usize) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        ProjectState::Loaded {
            file_count,
            symbol_count,
            dependency_count,
            loaded_at: now,
            watching: true,
        }
    }

    /// Create a new WatchPaused state
    pub fn watch_paused(
        file_count: usize,
        symbol_count: usize,
        dependency_count: usize,
        reason: impl Into<String>,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        ProjectState::WatchPaused {
            file_count,
            symbol_count,
            dependency_count,
            paused_at: now,
            reason: reason.into(),
        }
    }

    /// Create a new Failed state
    pub fn failed(error: impl Into<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        ProjectState::Failed {
            error: error.into(),
            failed_at: now,
        }
    }

    /// Check if the project is in a loaded state (Loaded or WatchPaused)
    pub fn is_loaded(&self) -> bool {
        matches!(
            self,
            ProjectState::Loaded { .. } | ProjectState::WatchPaused { .. }
        )
    }

    /// Check if the project is currently loading
    pub fn is_loading(&self) -> bool {
        matches!(self, ProjectState::Loading { .. })
    }

    /// Check if file watching is active
    pub fn is_watching(&self) -> bool {
        match self {
            ProjectState::Loaded { watching, .. } => *watching,
            _ => false,
        }
    }

    /// Get the file count if available
    pub fn file_count(&self) -> Option<usize> {
        match self {
            ProjectState::Loaded { file_count, .. } => Some(*file_count),
            ProjectState::WatchPaused { file_count, .. } => Some(*file_count),
            _ => None,
        }
    }

    /// Get the symbol count if available
    pub fn symbol_count(&self) -> Option<usize> {
        match self {
            ProjectState::Loaded { symbol_count, .. } => Some(*symbol_count),
            ProjectState::WatchPaused { symbol_count, .. } => Some(*symbol_count),
            _ => None,
        }
    }

    /// Get the dependency count if available
    pub fn dependency_count(&self) -> Option<usize> {
        match self {
            ProjectState::Loaded {
                dependency_count, ..
            } => Some(*dependency_count),
            ProjectState::WatchPaused {
                dependency_count, ..
            } => Some(*dependency_count),
            _ => None,
        }
    }

    /// Update progress during loading
    pub fn update_progress(&mut self, scanned_files: usize, current_file: Option<String>) {
        if let ProjectState::Loading {
            total_files,
            started_at,
            ..
        } = self
        {
            *self = ProjectState::Loading {
                total_files: *total_files,
                scanned_files,
                current_file,
                started_at: *started_at,
            };
        }
    }

    /// Set total files during loading
    pub fn set_total_files(&mut self, total: usize) {
        if let ProjectState::Loading {
            scanned_files,
            current_file,
            started_at,
            ..
        } = self
        {
            *self = ProjectState::Loading {
                total_files: total,
                scanned_files: *scanned_files,
                current_file: current_file.clone(),
                started_at: *started_at,
            };
        }
    }

    /// Transition from WatchPaused back to Loaded (resume watching)
    pub fn resume_watching(&mut self) {
        if let ProjectState::WatchPaused {
            file_count,
            symbol_count,
            dependency_count,
            ..
        } = self
        {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            *self = ProjectState::Loaded {
                file_count: *file_count,
                symbol_count: *symbol_count,
                dependency_count: *dependency_count,
                loaded_at: now,
                watching: true,
            };
        }
    }

    /// Transition from Loaded to WatchPaused (pause watching)
    pub fn pause_watching(&mut self, reason: impl Into<String>) {
        if let ProjectState::Loaded {
            file_count,
            symbol_count,
            dependency_count,
            ..
        } = self
        {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            *self = ProjectState::WatchPaused {
                file_count: *file_count,
                symbol_count: *symbol_count,
                dependency_count: *dependency_count,
                paused_at: now,
                reason: reason.into(),
            };
        }
    }
}

/// Scan progress information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    /// Total number of files to scan
    pub total_files: usize,
    /// Number of files scanned so far
    pub scanned_files: usize,
    /// Number of files that failed to parse
    pub failed_files: usize,
    /// Current file being processed
    pub current_file: Option<String>,
    /// Percentage complete (0-100)
    pub percentage: f32,
    /// Elapsed time in seconds
    pub elapsed_secs: u64,
}

impl ScanProgress {
    /// Create a new scan progress
    pub fn new(total_files: usize) -> Self {
        Self {
            total_files,
            scanned_files: 0,
            failed_files: 0,
            current_file: None,
            percentage: 0.0,
            elapsed_secs: 0,
        }
    }

    /// Update progress with a new scanned file
    pub fn update(&mut self, file: String, success: bool, elapsed_secs: u64) {
        self.scanned_files += 1;
        if !success {
            self.failed_files += 1;
        }
        self.current_file = Some(file);
        self.elapsed_secs = elapsed_secs;
        self.percentage = if self.total_files > 0 {
            (self.scanned_files as f32 / self.total_files as f32) * 100.0
        } else {
            0.0
        };
    }

    /// Check if scan is complete
    pub fn is_complete(&self) -> bool {
        self.scanned_files >= self.total_files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_state_not_loaded() {
        let state = ProjectState::not_loaded();
        assert!(!state.is_loaded());
        assert!(!state.is_loading());
        assert!(!state.is_watching());
        assert!(state.file_count().is_none());
    }

    #[test]
    fn test_project_state_loading() {
        let mut state = ProjectState::loading();
        assert!(state.is_loading());
        assert!(!state.is_loaded());
        assert!(!state.is_watching());

        state.set_total_files(100);
        state.update_progress(50, Some("src/main.rs".to_string()));

        if let ProjectState::Loading {
            total_files,
            scanned_files,
            current_file,
            ..
        } = state
        {
            assert_eq!(total_files, 100);
            assert_eq!(scanned_files, 50);
            assert_eq!(current_file, Some("src/main.rs".to_string()));
        } else {
            panic!("Expected Loading state");
        }
    }

    #[test]
    fn test_project_state_loaded() {
        let state = ProjectState::loaded(100, 500, 1000);
        assert!(state.is_loaded());
        assert!(!state.is_loading());
        assert!(state.is_watching());
        assert_eq!(state.file_count(), Some(100));
        assert_eq!(state.symbol_count(), Some(500));
        assert_eq!(state.dependency_count(), Some(1000));
    }

    #[test]
    fn test_project_state_pause_resume() {
        let mut state = ProjectState::loaded(100, 500, 1000);
        assert!(state.is_watching());

        state.pause_watching("Idle timeout");
        assert!(!state.is_watching());
        assert!(state.is_loaded());

        if let ProjectState::WatchPaused { reason, .. } = &state {
            assert_eq!(reason, "Idle timeout");
        } else {
            panic!("Expected WatchPaused state");
        }

        state.resume_watching();
        assert!(state.is_watching());
        assert!(state.is_loaded());
    }

    #[test]
    fn test_project_state_failed() {
        let state = ProjectState::failed("Scan error: permission denied");
        assert!(!state.is_loaded());
        assert!(!state.is_loading());

        if let ProjectState::Failed { error, .. } = state {
            assert_eq!(error, "Scan error: permission denied");
        } else {
            panic!("Expected Failed state");
        }
    }

    #[test]
    fn test_scan_progress() {
        let mut progress = ScanProgress::new(100);
        assert_eq!(progress.total_files, 100);
        assert_eq!(progress.scanned_files, 0);
        assert_eq!(progress.percentage, 0.0);
        assert!(!progress.is_complete());

        progress.update("src/main.rs".to_string(), true, 1);
        assert_eq!(progress.scanned_files, 1);
        assert_eq!(progress.failed_files, 0);
        assert_eq!(progress.percentage, 1.0);

        progress.update("src/error.rs".to_string(), false, 2);
        assert_eq!(progress.scanned_files, 2);
        assert_eq!(progress.failed_files, 1);
        assert_eq!(progress.percentage, 2.0);
    }

    #[test]
    fn test_project_state_serde() {
        let state = ProjectState::loaded(100, 500, 1000);
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: ProjectState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, deserialized);
    }
}
