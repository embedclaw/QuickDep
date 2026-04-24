//! SQLite schema definitions and migrations for QuickDep storage
//!
//! This module defines the database schema for storing:
//! - Symbols (functions, classes, structs, etc.)
//! - Dependencies (call, inherit, implement, type use)
//! - Imports (import statements)
//! - File state (hash, status, errors)

use rusqlite::Connection;

/// Current schema version
pub const SCHEMA_VERSION: i32 = 1;

/// SQL schema definition
pub const SCHEMA_SQL: &str = "
-- Symbols table: stores code symbols (functions, classes, etc.)
CREATE TABLE IF NOT EXISTS symbols (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    file_path TEXT NOT NULL,
    line INTEGER NOT NULL,
    column INTEGER NOT NULL,
    visibility TEXT NOT NULL DEFAULT 'private',
    signature TEXT,
    source TEXT NOT NULL DEFAULT 'local',
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

-- Dependencies table: stores relationships between symbols
CREATE TABLE IF NOT EXISTS dependencies (
    id TEXT PRIMARY KEY,
    from_symbol TEXT NOT NULL,
    to_symbol TEXT NOT NULL,
    from_file TEXT NOT NULL,
    from_line INTEGER NOT NULL,
    kind TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

-- Imports table: stores import statements
CREATE TABLE IF NOT EXISTS imports (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    alias TEXT,
    file_path TEXT NOT NULL,
    line INTEGER NOT NULL,
    kind TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

-- File state table: tracks file hashes and parsing status
CREATE TABLE IF NOT EXISTS file_state (
    path TEXT PRIMARY KEY,
    hash TEXT NOT NULL,
    last_modified INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'ok',
    error_message TEXT,
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

-- Scan state table: tracks scanning progress
CREATE TABLE IF NOT EXISTS scan_state (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    files_total INTEGER DEFAULT 0,
    files_scanned INTEGER DEFAULT 0,
    started_at INTEGER,
    updated_at INTEGER
);

-- Metadata table: stores key-value metadata
CREATE TABLE IF NOT EXISTS metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Indexes for symbols table
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_qualified ON symbols(qualified_name);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_path);
CREATE INDEX IF NOT EXISTS idx_symbols_kind ON symbols(kind);

-- Indexes for dependencies table
CREATE INDEX IF NOT EXISTS idx_deps_from ON dependencies(from_symbol);
CREATE INDEX IF NOT EXISTS idx_deps_to ON dependencies(to_symbol);
CREATE INDEX IF NOT EXISTS idx_deps_file ON dependencies(from_file);
CREATE INDEX IF NOT EXISTS idx_deps_kind ON dependencies(kind);

-- Indexes for imports table
CREATE INDEX IF NOT EXISTS idx_imports_file ON imports(file_path);

-- Index for file_state
CREATE INDEX IF NOT EXISTS idx_file_state_status ON file_state(status);
";

/// Initialize the database schema
///
/// Creates all tables and indexes if they don't exist.
/// Sets the schema version in the metadata table.
///
/// # Arguments
/// * `conn` - SQLite connection
///
/// # Errors
/// Returns error if schema creation fails
pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    // Execute schema creation
    conn.execute_batch(SCHEMA_SQL)?;

    // Set schema version if not already set
    conn.execute(
        "INSERT OR IGNORE INTO metadata (key, value) VALUES ('schema_version', ?1)",
        [SCHEMA_VERSION.to_string()],
    )?;

    Ok(())
}

/// Get the current schema version from the database
///
/// # Arguments
/// * `conn` - SQLite connection
///
/// # Returns
/// The schema version as an integer, or 0 if not set
pub fn get_schema_version(conn: &Connection) -> i32 {
    conn.query_row(
        "SELECT value FROM metadata WHERE key = 'schema_version'",
        [],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .and_then(|v| v.parse::<i32>().ok())
    .unwrap_or(0)
}

/// Check if schema needs to be rebuilt
///
/// Returns true if the stored schema version doesn't match the current version.
///
/// # Arguments
/// * `conn` - SQLite connection
///
/// # Returns
/// true if schema version mismatch, false otherwise
pub fn needs_rebuild(conn: &Connection) -> bool {
    get_schema_version(conn) != SCHEMA_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_init_schema() {
        let temp_file = NamedTempFile::new().unwrap();
        let conn = Connection::open(temp_file.path()).unwrap();

        init_schema(&conn).unwrap();

        // Check tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"symbols".to_string()));
        assert!(tables.contains(&"dependencies".to_string()));
        assert!(tables.contains(&"imports".to_string()));
        assert!(tables.contains(&"file_state".to_string()));
        assert!(tables.contains(&"metadata".to_string()));
    }

    #[test]
    fn test_schema_version() {
        let temp_file = NamedTempFile::new().unwrap();
        let conn = Connection::open(temp_file.path()).unwrap();

        init_schema(&conn).unwrap();
        let version = get_schema_version(&conn);

        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_needs_rebuild() {
        let temp_file = NamedTempFile::new().unwrap();
        let conn = Connection::open(temp_file.path()).unwrap();

        // Before init, needs rebuild (version is 0)
        assert!(needs_rebuild(&conn));

        init_schema(&conn).unwrap();

        // After init, no rebuild needed
        assert!(!needs_rebuild(&conn));
    }
}
