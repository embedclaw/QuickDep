//! Path validation and security functions
//!
//! This module provides security functions to:
//! - Validate paths and prevent path traversal attacks
//! - Generate and validate project IDs based on path hashes

use std::path::{Path, PathBuf};

use blake3::Hash;
use path_clean::PathClean;
use thiserror::Error;

/// Security-related errors
#[derive(Error, Debug)]
pub enum SecurityError {
    /// The provided path is invalid (does not exist or cannot be accessed)
    #[error("Invalid path: {0}")]
    InvalidPath(String),

    /// Path traversal attempt detected - path escapes the allowed root
    #[error("Path traversal detected: path '{path}' escapes root '{root}'")]
    PathTraversal {
        /// The problematic path
        path: String,
        /// The root directory that was escaped
        root: String,
    },

    /// Project ID does not match the expected hash for the given path
    #[error("Project ID mismatch: expected '{expected}', got '{actual}'")]
    ProjectIdMismatch {
        /// Expected project ID
        expected: String,
        /// Actual project ID provided
        actual: String,
    },

    /// I/O error during path validation
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Validates that a path is within the allowed project root directory.
///
/// This function performs path traversal detection by:
/// 1. Canonicalizing both the root and input paths
/// 2. Checking that the input path starts with the root path
///
/// # Arguments
///
/// * `project_root` - The allowed root directory (must be an absolute, canonical path)
/// * `input_path` - The path to validate (can be relative or absolute)
///
/// # Returns
///
/// Returns the canonicalized path if validation succeeds.
///
/// # Errors
///
/// Returns `SecurityError::InvalidPath` if the path cannot be canonicalized.
/// Returns `SecurityError::PathTraversal` if the path escapes the root directory.
///
/// # Examples
///
/// ```ignore
/// use std::path::Path;
/// use quickdep::security::validate_path;
///
/// let root = Path::new("/home/user/project");
/// let valid_path = validate_path(root, "src/main.rs")?;
/// let invalid_path = validate_path(root, "../etc/passwd"); // Returns PathTraversal error
/// ```
pub fn validate_path<P1: AsRef<Path>, P2: AsRef<Path>>(
    project_root: P1,
    input_path: P2,
) -> Result<PathBuf, SecurityError> {
    let project_root = project_root.as_ref();
    let input_path = input_path.as_ref();

    // Canonicalize the project root first
    let canonical_root = project_root.canonicalize().map_err(|e| {
        SecurityError::InvalidPath(format!(
            "Cannot canonicalize project root '{}': {}",
            project_root.display(),
            e
        ))
    })?;

    // Handle the input path - it can be relative to root or absolute
    let full_path = if input_path.is_absolute() {
        input_path.to_path_buf()
    } else {
        canonical_root.join(input_path)
    };

    let cleaned_path = full_path.clean();
    if !cleaned_path.starts_with(&canonical_root) {
        return Err(SecurityError::PathTraversal {
            path: cleaned_path.display().to_string(),
            root: canonical_root.display().to_string(),
        });
    }

    // Canonicalize the full path
    let canonical_path = cleaned_path.canonicalize().map_err(|e| {
        SecurityError::InvalidPath(format!(
            "Cannot canonicalize path '{}': {}",
            cleaned_path.display(),
            e
        ))
    })?;

    // Check if the canonicalized path starts with the root
    if !canonical_path.starts_with(&canonical_root) {
        return Err(SecurityError::PathTraversal {
            path: canonical_path.display().to_string(),
            root: canonical_root.display().to_string(),
        });
    }

    Ok(canonical_path)
}

/// Generates a unique project ID from a project path.
///
/// The project ID is derived from the blake3 hash of the canonical path,
/// providing a deterministic and collision-resistant identifier.
///
/// # Arguments
///
/// * `project_path` - The path to the project directory
///
/// # Returns
///
/// Returns a 16-character hex string derived from the blake3 hash of the canonical path.
///
/// # Errors
///
/// Returns `SecurityError::InvalidPath` if the path cannot be canonicalized.
///
/// # Examples
///
/// ```ignore
/// use std::path::Path;
/// use quickdep::security::generate_project_id;
///
/// let project_id = generate_project_id("/home/user/myproject")?;
/// println!("Project ID: {}", project_id); // e.g., "a1b2c3d4e5f6g7h8"
/// ```
pub fn generate_project_id<P: AsRef<Path>>(project_path: P) -> Result<String, SecurityError> {
    let project_path = project_path.as_ref();

    // Canonicalize the path first
    let canonical_path = project_path.canonicalize().map_err(|e| {
        SecurityError::InvalidPath(format!(
            "Cannot canonicalize project path '{}': {}",
            project_path.display(),
            e
        ))
    })?;

    // Generate blake3 hash of the canonical path string
    let path_str = canonical_path.to_string_lossy();
    let hash: Hash = blake3::hash(path_str.as_bytes());

    // Take first 16 characters (8 bytes) of the hex hash for a compact ID
    let project_id = hash.to_hex()[..16].to_string();

    Ok(project_id)
}

/// Validates that a project ID matches the expected hash for a given path.
///
/// This function ensures that the provided project ID was generated from
/// the specified project path, preventing project ID spoofing.
///
/// # Arguments
///
/// * `project_path` - The project directory path
/// * `project_id` - The project ID to validate
///
/// # Returns
///
/// Returns `Ok(())` if the project ID matches the expected hash.
///
/// # Errors
///
/// Returns `SecurityError::ProjectIdMismatch` if the IDs don't match.
/// Returns `SecurityError::InvalidPath` if the path cannot be canonicalized.
///
/// # Examples
///
/// ```ignore
/// use quickdep::security::{generate_project_id, validate_project_id};
///
/// let path = "/home/user/myproject";
/// let project_id = generate_project_id(path)?;
///
/// // This will succeed
/// validate_project_id(path, &project_id)?;
///
/// // This will fail with ProjectIdMismatch
/// validate_project_id(path, "invalid_id")?;
/// ```
pub fn validate_project_id<P: AsRef<Path>>(
    project_path: P,
    project_id: &str,
) -> Result<(), SecurityError> {
    let expected_id = generate_project_id(&project_path)?;

    if expected_id != project_id {
        return Err(SecurityError::ProjectIdMismatch {
            expected: expected_id,
            actual: project_id.to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_validate_path_within_root() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Create a file in the directory
        let file_path = root.join("test.txt");
        fs::write(&file_path, "test").unwrap();

        // Valid path within root
        let result = validate_path(root, "test.txt");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), file_path.canonicalize().unwrap());
    }

    #[test]
    fn test_validate_path_subdirectory() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Create subdirectory and file
        let subdir = root.join("src");
        fs::create_dir(&subdir).unwrap();
        let file_path = subdir.join("main.rs");
        fs::write(&file_path, "fn main() {}").unwrap();

        // Valid path in subdirectory
        let result = validate_path(root, "src/main.rs");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), file_path.canonicalize().unwrap());
    }

    #[test]
    fn test_validate_path_traversal_blocked() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Create a file outside root
        let parent_dir = dir.path().parent().unwrap();
        let outside_file = parent_dir.join("outside.txt");
        fs::write(&outside_file, "outside").unwrap();

        // Attempt to traverse outside root with "../"
        let result = validate_path(root, "../outside.txt");
        assert!(matches!(result, Err(SecurityError::PathTraversal { .. })));
    }

    #[test]
    fn test_validate_path_absolute_outside_root() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Create a file outside the root
        let another_dir = tempdir().unwrap();
        let outside_file = another_dir.path().join("outside.txt");
        fs::write(&outside_file, "outside").unwrap();

        // Attempt to access file outside root using absolute path
        let result = validate_path(root, &outside_file);
        assert!(matches!(result, Err(SecurityError::PathTraversal { .. })));
    }

    #[test]
    fn test_validate_path_nonexistent() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Non-existent path should return InvalidPath error
        let result = validate_path(root, "nonexistent.txt");
        assert!(matches!(result, Err(SecurityError::InvalidPath(_))));
    }

    #[test]
    fn test_generate_project_id_consistency() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        let id1 = generate_project_id(path).unwrap();
        let id2 = generate_project_id(path).unwrap();

        // Same path should produce same ID
        assert_eq!(id1, id2);

        // ID should be 16 hex characters
        assert_eq!(id1.len(), 16);
        assert!(id1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_generate_project_id_different_paths() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();

        let id1 = generate_project_id(dir1.path()).unwrap();
        let id2 = generate_project_id(dir2.path()).unwrap();

        // Different paths should produce different IDs
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_validate_project_id_valid() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        let project_id = generate_project_id(path).unwrap();
        let result = validate_project_id(path, &project_id);

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_project_id_invalid() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        let result = validate_project_id(path, "invalid_id12345");

        assert!(matches!(
            result,
            Err(SecurityError::ProjectIdMismatch { .. })
        ));
    }

    #[test]
    fn test_validate_project_id_wrong_path() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();

        let id1 = generate_project_id(dir1.path()).unwrap();

        // Using ID from dir1 with path from dir2 should fail
        let result = validate_project_id(dir2.path(), &id1);

        assert!(matches!(
            result,
            Err(SecurityError::ProjectIdMismatch { .. })
        ));
    }

    #[test]
    fn test_multiple_traversal_attempts() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Create a file in parent directory
        let parent = root.parent().unwrap();
        let outside_file = parent.join("secret.txt");
        fs::write(&outside_file, "secret").unwrap();

        // Various traversal attempts
        let attempts = vec![
            "../secret.txt",
            "./../secret.txt",
            "subdir/../../secret.txt",
        ];

        for attempt in attempts {
            let result = validate_path(root, attempt);
            assert!(
                matches!(result, Err(SecurityError::PathTraversal { .. })),
                "Traversal should be blocked: {}",
                attempt
            );
        }
    }
}
