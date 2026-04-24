//! File watcher and incremental update planning.
//!
//! This module provides:
//! - `notify`-based filesystem watching
//! - 500ms debouncing for bursty file events
//! - blake3 content hashing for change filtering
//! - incremental update planning from `file_state` snapshots

pub mod debounce;
pub mod fs;

use crate::storage::FileState;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use thiserror::Error;

pub use debounce::EventDebouncer;
pub use fs::{FileChangeEvent, FileSystemWatcher, WatchEventKind};

/// Watcher-specific errors.
#[derive(Debug, Error)]
pub enum WatcherError {
    /// Filesystem notification backend error.
    #[error("Watcher backend error: {0}")]
    Notify(#[from] notify::Error),

    /// I/O error while reading the file system.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A watched path was outside the configured project root.
    #[error("Path '{path}' is outside project root '{root}'")]
    PathOutsideRoot { path: String, root: String },
}

/// Incremental update classification for a changed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateKind {
    Added,
    Modified,
    Deleted,
}

/// Planned update for a single file after hash comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncrementalFileUpdate {
    /// Absolute file path.
    pub absolute_path: PathBuf,
    /// File path relative to the project root.
    pub relative_path: String,
    /// Update classification.
    pub kind: UpdateKind,
    /// New content hash when the file still exists.
    pub hash: Option<String>,
    /// Last-modified timestamp in seconds when the file still exists.
    pub last_modified: Option<u64>,
}

/// Result of comparing a batch of changed paths with stored file state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncrementalUpdatePlan {
    /// Files that require parser/storage refresh.
    pub updates: Vec<IncrementalFileUpdate>,
    /// Paths skipped because the content hash did not change.
    pub skipped: Vec<PathBuf>,
}

/// Compute the blake3 hash of a file.
pub fn compute_file_hash(path: &Path) -> Result<String, WatcherError> {
    let contents = std::fs::read(path)?;
    Ok(blake3::hash(&contents).to_hex().to_string())
}

/// Build an incremental update plan from changed paths and previous file state.
pub fn build_update_plan(
    project_root: &Path,
    changed_paths: &[PathBuf],
    previous_states: &HashMap<String, FileState>,
) -> Result<IncrementalUpdatePlan, WatcherError> {
    let canonical_root = project_root.canonicalize()?;
    let mut unique_paths = BTreeSet::new();
    for path in changed_paths {
        unique_paths.insert(normalize_path(&canonical_root, path));
    }

    let mut updates = Vec::new();
    let mut skipped = Vec::new();

    for absolute_path in unique_paths {
        let relative_path = relative_path(&canonical_root, &absolute_path)?;
        let previous = previous_states.get(&relative_path);

        if absolute_path.exists() {
            if absolute_path.is_dir() {
                skipped.push(absolute_path);
                continue;
            }

            let hash = compute_file_hash(&absolute_path)?;
            if previous.is_some_and(|state| state.hash == hash) {
                skipped.push(absolute_path);
                continue;
            }

            updates.push(IncrementalFileUpdate {
                absolute_path: absolute_path.clone(),
                relative_path,
                kind: if previous.is_some() {
                    UpdateKind::Modified
                } else {
                    UpdateKind::Added
                },
                hash: Some(hash),
                last_modified: Some(last_modified_secs(&absolute_path)?),
            });
        } else if previous.is_some() {
            updates.push(IncrementalFileUpdate {
                absolute_path,
                relative_path,
                kind: UpdateKind::Deleted,
                hash: None,
                last_modified: None,
            });
        }
    }

    Ok(IncrementalUpdatePlan { updates, skipped })
}

fn normalize_path(project_root: &Path, path: &Path) -> PathBuf {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    };

    canonicalize_maybe_missing(&absolute_path)
}

fn canonicalize_maybe_missing(path: &Path) -> PathBuf {
    let mut current = path;
    let mut suffix = Vec::new();

    while !current.exists() {
        let Some(name) = current.file_name() else {
            return path.to_path_buf();
        };
        suffix.push(name.to_os_string());

        let Some(parent) = current.parent() else {
            return path.to_path_buf();
        };
        current = parent;
    }

    let mut normalized = current
        .canonicalize()
        .unwrap_or_else(|_| current.to_path_buf());
    for component in suffix.iter().rev() {
        normalized.push(component);
    }

    normalized
}

fn relative_path(project_root: &Path, absolute_path: &Path) -> Result<String, WatcherError> {
    absolute_path
        .strip_prefix(project_root)
        .map(|path| path.to_string_lossy().to_string())
        .map_err(|_| WatcherError::PathOutsideRoot {
            path: absolute_path.display().to_string(),
            root: project_root.display().to_string(),
        })
}

fn last_modified_secs(path: &Path) -> Result<u64, WatcherError> {
    let modified = std::fs::metadata(path)?.modified()?;
    Ok(modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::FileStatus;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_compute_file_hash_changes_with_content() {
        let temp_dir = tempdir().expect("create temp dir");
        let file_path = temp_dir.path().join("main.rs");

        fs::write(&file_path, "fn main() {}\n").expect("write file");
        let first_hash = compute_file_hash(&file_path).expect("hash file");

        fs::write(&file_path, "fn main() { println!(\"hi\"); }\n").expect("rewrite file");
        let second_hash = compute_file_hash(&file_path).expect("hash file");

        assert_ne!(first_hash, second_hash);
    }

    #[test]
    fn test_build_update_plan_classifies_changes() {
        let temp_dir = tempdir().expect("create temp dir");
        let root = temp_dir.path();

        let unchanged_path = root.join("src/unchanged.rs");
        let added_path = root.join("src/added.rs");
        let modified_path = root.join("src/modified.rs");
        let deleted_path = root.join("src/deleted.rs");

        fs::create_dir_all(root.join("src")).expect("create source dir");
        fs::write(&unchanged_path, "fn unchanged() {}\n").expect("write unchanged file");
        fs::write(&added_path, "fn added() {}\n").expect("write added file");
        fs::write(&modified_path, "fn modified() { 1 }\n").expect("write modified file");

        let unchanged_hash = compute_file_hash(&unchanged_path).expect("hash unchanged");
        let old_modified_hash = blake3::hash(b"old contents").to_hex().to_string();

        let previous_states = HashMap::from([
            (
                "src/unchanged.rs".to_string(),
                FileState {
                    path: "src/unchanged.rs".to_string(),
                    hash: unchanged_hash.clone(),
                    last_modified: 1,
                    status: FileStatus::Ok,
                    error_message: None,
                },
            ),
            (
                "src/modified.rs".to_string(),
                FileState {
                    path: "src/modified.rs".to_string(),
                    hash: old_modified_hash,
                    last_modified: 1,
                    status: FileStatus::Ok,
                    error_message: None,
                },
            ),
            (
                "src/deleted.rs".to_string(),
                FileState {
                    path: "src/deleted.rs".to_string(),
                    hash: "old_deleted_hash".to_string(),
                    last_modified: 1,
                    status: FileStatus::Ok,
                    error_message: None,
                },
            ),
        ]);

        let plan = build_update_plan(
            root,
            &[
                unchanged_path.clone(),
                added_path.clone(),
                modified_path.clone(),
                deleted_path.clone(),
            ],
            &previous_states,
        )
        .expect("build update plan");

        assert_eq!(
            plan.skipped,
            vec![canonicalize_maybe_missing(&unchanged_path)]
        );
        assert_eq!(plan.updates.len(), 3);
        assert!(plan.updates.iter().any(
            |update| update.relative_path == "src/added.rs" && update.kind == UpdateKind::Added
        ));
        assert!(plan.updates.iter().any(|update| {
            update.relative_path == "src/modified.rs" && update.kind == UpdateKind::Modified
        }));
        assert!(plan.updates.iter().any(|update| {
            update.relative_path == "src/deleted.rs" && update.kind == UpdateKind::Deleted
        }));
    }

    #[test]
    fn test_build_update_plan_rejects_paths_outside_root() {
        let temp_dir = tempdir().expect("create temp dir");
        let outside_dir = tempdir().expect("create outside temp dir");
        let outside_file = outside_dir.path().join("outside.rs");
        fs::write(&outside_file, "fn outside() {}\n").expect("write outside file");

        let result = build_update_plan(temp_dir.path(), &[outside_file], &HashMap::new());
        assert!(matches!(result, Err(WatcherError::PathOutsideRoot { .. })));
    }
}
