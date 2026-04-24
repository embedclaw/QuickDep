//! Project ID generation using blake3 hash
//!
//! Project IDs are deterministic and based on the canonical path of the project.
//! This ensures the same project always gets the same ID, enabling project isolation.

use std::path::{Path, PathBuf};
use thiserror::Error;

/// Error type for project ID operations
#[derive(Debug, Error)]
pub enum ProjectIdError {
    /// Path does not exist
    #[error("Path does not exist: {0}")]
    PathNotFound(PathBuf),

    /// Failed to canonicalize path
    #[error("Failed to canonicalize path: {0}")]
    CanonicalizeError(#[source] std::io::Error),

    /// Invalid path
    #[error("Invalid path: {0}")]
    InvalidPath(String),
}

/// A unique identifier for a project, derived from its canonical path
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(String);

impl ProjectId {
    /// Length of the hex string (16 characters = 8 bytes)
    const HEX_LENGTH: usize = 16;

    /// Generate a project ID from a path
    ///
    /// The ID is computed as: blake3(canonical_path)[0:16]
    ///
    /// # Arguments
    /// * `path` - The path to the project directory
    ///
    /// # Returns
    /// * `Ok(ProjectId)` - The generated project ID
    /// * `Err(ProjectIdError)` - If the path doesn't exist or can't be canonicalized
    ///
    /// # Example
    /// ```ignore
    /// use quickdep::project::ProjectId;
    /// let id = ProjectId::from_path("/path/to/project")?;
    /// println!("Project ID: {}", id);
    /// ```
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, ProjectIdError> {
        let path = path.as_ref();

        // Check if path exists
        if !path.exists() {
            return Err(ProjectIdError::PathNotFound(path.to_path_buf()));
        }

        // Get canonical path (resolves symlinks, .., .)
        let canonical = path
            .canonicalize()
            .map_err(ProjectIdError::CanonicalizeError)?;

        // Convert to string for hashing
        let path_str = canonical
            .to_str()
            .ok_or_else(|| ProjectIdError::InvalidPath("Path contains invalid UTF-8".into()))?;

        // Compute blake3 hash
        let hash = blake3::hash(path_str.as_bytes());

        // Take first 16 hex characters (8 bytes)
        let hex = hash.to_hex();
        let id = &hex.as_str()[..Self::HEX_LENGTH];

        Ok(ProjectId(id.to_string()))
    }

    /// Create a ProjectId from a known string (e.g., from storage)
    ///
    /// # Arguments
    /// * `s` - The string representation of the ID
    ///
    /// # Returns
    /// A new ProjectId with the given string
    pub fn from_string(s: impl Into<String>) -> Self {
        ProjectId(s.into())
    }

    /// Get the string representation of the ID
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Convert to owned string
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for ProjectId {
    type Err = ProjectIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != Self::HEX_LENGTH {
            return Err(ProjectIdError::InvalidPath(format!(
                "ProjectId must be {} hex characters, got {}",
                Self::HEX_LENGTH,
                s.len()
            )));
        }
        // Validate hex characters
        if !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ProjectIdError::InvalidPath(
                "ProjectId must contain only hex characters".into(),
            ));
        }
        Ok(ProjectId(s.to_string()))
    }
}

impl serde::Serialize for ProjectId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for ProjectId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        // Use from_string which accepts any string input
        // The from_str validation is for strict parsing; here we trust stored data
        Ok(ProjectId::from_string(&s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_project_id_from_path() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let path = temp_dir.path();

        let id1 = ProjectId::from_path(path).expect("Failed to generate ID");
        let id2 = ProjectId::from_path(path).expect("Failed to generate ID");

        // Same path should produce same ID
        assert_eq!(id1, id2);

        // ID should be 16 hex characters
        assert_eq!(id1.as_str().len(), 16);
        assert!(id1.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_project_id_different_paths() {
        let temp_dir1 = TempDir::new().expect("Failed to create temp dir");
        let temp_dir2 = TempDir::new().expect("Failed to create temp dir");

        let id1 = ProjectId::from_path(temp_dir1.path()).expect("Failed to generate ID");
        let id2 = ProjectId::from_path(temp_dir2.path()).expect("Failed to generate ID");

        // Different paths should produce different IDs
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_project_id_nonexistent_path() {
        let result = ProjectId::from_path("/nonexistent/path/to/project");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ProjectIdError::PathNotFound(_)
        ));
    }

    #[test]
    fn test_project_id_from_string() {
        let id = ProjectId::from_string("a1b2c3d4e5f6a7b8");
        assert_eq!(id.as_str(), "a1b2c3d4e5f6a7b8");
    }

    #[test]
    fn test_project_id_display() {
        let id = ProjectId::from_string("a1b2c3d4e5f6a7b8");
        assert_eq!(format!("{}", id), "a1b2c3d4e5f6a7b8");
    }

    #[test]
    fn test_project_id_from_str() {
        let id: ProjectId = "a1b2c3d4e5f6a7b8".parse().unwrap();
        assert_eq!(id.as_str(), "a1b2c3d4e5f6a7b8");
    }

    #[test]
    fn test_project_id_from_str_invalid_length() {
        let result: Result<ProjectId, _> = "abc".parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_project_id_from_str_invalid_chars() {
        let result: Result<ProjectId, _> = "g1h2i3j4k5l6m7n8".parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_project_id_serde() {
        let id = ProjectId::from_string("a1b2c3d4e5f6a7b8");

        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"a1b2c3d4e5f6a7b8\"");

        let deserialized: ProjectId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, id);
    }

    #[test]
    fn test_project_id_symlink() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let real_path = temp_dir.path();

        // Create a symlink to the temp dir
        let link_path = temp_dir.path().parent().unwrap().join("project_link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(real_path, &link_path).expect("Failed to create symlink");

        let id_from_real = ProjectId::from_path(real_path).expect("Failed to generate ID");
        let id_from_link = ProjectId::from_path(&link_path).expect("Failed to generate ID");

        // Symlink should resolve to same canonical path, thus same ID
        assert_eq!(id_from_real, id_from_link);

        // Cleanup
        #[cfg(unix)]
        fs::remove_file(&link_path).ok();
    }
}
