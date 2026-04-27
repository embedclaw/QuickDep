//! SQLite storage operations for QuickDep
//!
//! This module provides:
//! - SQLite connection management with WAL mode
//! - CRUD operations for symbols, dependencies, imports, file_state
//! - Recursive CTE queries for dependency chain traversal
//! - Batch insert optimization using transactions

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource, Visibility};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

use super::{
    fts::{init_symbols_fts, search_symbols as search_symbols_fts},
    schema::{init_schema, needs_rebuild, SCHEMA_VERSION},
};

/// Storage error types
#[derive(Debug, Error)]
pub enum StorageError {
    /// SQLite operation error
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Schema version mismatch
    #[error(
        "Schema version mismatch: expected {expected}, found {found}. Please rebuild database."
    )]
    SchemaMismatch { expected: i32, found: i32 },

    /// File system error
    #[error("File system error: {0}")]
    Io(#[from] std::io::Error),

    /// Invalid data error
    #[error("Invalid data: {0}")]
    InvalidData(String),

    /// Symbol not found
    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),

    /// Dependency not found
    #[error("Dependency not found: {0}")]
    DependencyNotFound(String),
}

/// Import kind (type of import statement)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    /// Named import: import { x } from 'y'
    Named,
    /// Glob import: import * as x from 'y'
    Glob,
    /// Self import (Rust): use self::module
    SelfImport,
    /// Alias import: import { x as y }
    Alias,
    /// Re-exported named import: export { x } from 'y'
    ReExportNamed,
    /// Re-exported glob import: export * from 'y'
    ReExportGlob,
    /// Re-exported aliased import: export { x as y } from 'y'
    ReExportAlias,
}

impl ImportKind {
    /// Convert to string for storage
    pub fn as_str(&self) -> &'static str {
        match self {
            ImportKind::Named => "named",
            ImportKind::Glob => "glob",
            ImportKind::SelfImport => "self",
            ImportKind::Alias => "alias",
            ImportKind::ReExportNamed => "reexport_named",
            ImportKind::ReExportGlob => "reexport_glob",
            ImportKind::ReExportAlias => "reexport_alias",
        }
    }
}

impl FromStr for ImportKind {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "named" => Ok(ImportKind::Named),
            "glob" => Ok(ImportKind::Glob),
            "self" => Ok(ImportKind::SelfImport),
            "alias" => Ok(ImportKind::Alias),
            "reexport_named" => Ok(ImportKind::ReExportNamed),
            "reexport_glob" => Ok(ImportKind::ReExportGlob),
            "reexport_alias" => Ok(ImportKind::ReExportAlias),
            _ => Err("unknown import kind"),
        }
    }
}

/// Import statement record
#[derive(Debug, Clone)]
pub struct Import {
    /// Unique identifier
    pub id: String,
    /// Import source (module path)
    pub source: String,
    /// Alias name (optional)
    pub alias: Option<String>,
    /// File path
    pub file_path: String,
    /// Line number
    pub line: u32,
    /// Import kind
    pub kind: ImportKind,
}

impl Import {
    /// Create a new import
    pub fn new(source: String, file_path: String, line: u32, kind: ImportKind) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            source,
            alias: None,
            file_path,
            line,
            kind,
        }
    }

    /// Set alias
    pub fn with_alias(mut self, alias: String) -> Self {
        self.alias = Some(alias);
        self
    }
}

/// File status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    /// File parsed successfully
    Ok,
    /// File parsing failed
    Failed,
    /// File pending parsing
    Pending,
}

impl FileStatus {
    /// Convert to string for storage
    pub fn as_str(&self) -> &'static str {
        match self {
            FileStatus::Ok => "ok",
            FileStatus::Failed => "failed",
            FileStatus::Pending => "pending",
        }
    }
}

impl FromStr for FileStatus {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ok" => Ok(FileStatus::Ok),
            "failed" => Ok(FileStatus::Failed),
            "pending" => Ok(FileStatus::Pending),
            _ => Err("unknown file status"),
        }
    }
}

/// File state record
#[derive(Debug, Clone)]
pub struct FileState {
    /// File path
    pub path: String,
    /// Content hash (blake3)
    pub hash: String,
    /// Last modified timestamp
    pub last_modified: u64,
    /// Parsing status
    pub status: FileStatus,
    /// Error message (if failed)
    pub error_message: Option<String>,
}

impl FileState {
    /// Create a new file state
    pub fn new(path: String, hash: String, last_modified: u64) -> Self {
        Self {
            path,
            hash,
            last_modified,
            status: FileStatus::Ok,
            error_message: None,
        }
    }

    /// Set status
    pub fn with_status(mut self, status: FileStatus) -> Self {
        self.status = status;
        self
    }

    /// Set error message
    pub fn with_error(mut self, error: String) -> Self {
        self.error_message = Some(error);
        self.status = FileStatus::Failed;
        self
    }
}

/// Dependency chain node (for recursive CTE queries)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyNode {
    /// Symbol ID
    pub symbol_id: String,
    /// Symbol name
    pub name: String,
    /// Qualified name
    pub qualified_name: String,
    /// File path
    pub file_path: String,
    /// Depth in chain (0 = starting point)
    pub depth: u32,
    /// Dependency kind
    pub dep_kind: Option<DependencyKind>,
}

/// SQLite storage backend
///
/// Provides persistent storage for symbols, dependencies, imports,
/// and file state using SQLite with WAL mode.
pub struct Storage {
    /// SQLite connection
    conn: Connection,
}

impl Storage {
    /// Create a new storage instance
    ///
    /// Opens or creates a SQLite database at the given path.
    /// Enables WAL mode for better concurrent access.
    /// Initializes schema if needed.
    ///
    /// # Arguments
    /// * `path` - Path to the SQLite database file
    ///
    /// # Errors
    /// Returns error if:
    /// - Cannot create/open database file
    /// - Cannot enable WAL mode
    /// - Schema initialization fails
    pub fn new(path: &Path) -> Result<Self, StorageError> {
        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Open connection
        let conn = Connection::open(path)?;

        // Enable WAL mode
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -64000;",
        )?;

        // Initialize schema
        init_schema(&conn)?;
        init_symbols_fts(&conn)?;

        // Check schema version
        let version = super::schema::get_schema_version(&conn);
        if version != SCHEMA_VERSION {
            return Err(StorageError::SchemaMismatch {
                expected: SCHEMA_VERSION,
                found: version,
            });
        }

        Ok(Self { conn })
    }

    /// Get the underlying SQLite connection
    ///
    /// This is useful for advanced queries or batch operations.
    ///
    /// # Returns
    /// Reference to the SQLite connection
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    // =========================================================================
    // Symbols CRUD
    // =========================================================================

    /// Insert a symbol
    ///
    /// # Arguments
    /// * `symbol` - Symbol to insert
    ///
    /// # Errors
    /// Returns error if insert fails (e.g., duplicate qualified_name)
    pub fn insert_symbol(&self, symbol: &Symbol) -> Result<(), StorageError> {
        let now = current_timestamp();
        self.conn.execute(
            "INSERT INTO symbols (id, name, qualified_name, kind, file_path, line, column,
             visibility, signature, source, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(qualified_name) DO UPDATE SET
             id = excluded.id,
             name = excluded.name,
             kind = excluded.kind,
             file_path = excluded.file_path,
             line = excluded.line,
             column = excluded.column,
             visibility = excluded.visibility,
             signature = excluded.signature,
             source = excluded.source,
             updated_at = excluded.updated_at",
            params![
                symbol.id,
                symbol.name,
                symbol.qualified_name,
                symbol.kind.as_str(),
                symbol.file_path,
                symbol.line,
                symbol.column,
                symbol.visibility.as_str(),
                symbol.signature,
                symbol.source.as_str(),
                now,
                now,
            ],
        )?;
        Ok(())
    }

    /// Get a symbol by ID
    ///
    /// # Arguments
    /// * `id` - Symbol ID
    ///
    /// # Returns
    /// The symbol, or None if not found
    pub fn get_symbol(&self, id: &str) -> Result<Option<Symbol>, StorageError> {
        let result = self
            .conn
            .query_row(
                "SELECT id, name, qualified_name, kind, file_path, line, column,
                 visibility, signature, source FROM symbols WHERE id = ?1",
                [id],
                row_to_symbol,
            )
            .optional()?;
        Ok(result)
    }

    /// Get a symbol by qualified name
    ///
    /// # Arguments
    /// * `qualified_name` - Fully qualified name (e.g., "src/utils.rs::helper")
    ///
    /// # Returns
    /// The symbol, or None if not found
    pub fn get_symbol_by_qualified_name(
        &self,
        qualified_name: &str,
    ) -> Result<Option<Symbol>, StorageError> {
        let result = self
            .conn
            .query_row(
                "SELECT id, name, qualified_name, kind, file_path, line, column,
                 visibility, signature, source FROM symbols WHERE qualified_name = ?1",
                [qualified_name],
                row_to_symbol,
            )
            .optional()?;
        Ok(result)
    }

    /// Get every symbol currently stored in the database.
    ///
    /// # Returns
    /// A full list of symbols ordered by file path and source location.
    pub fn get_all_symbols(&self) -> Result<Vec<Symbol>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, qualified_name, kind, file_path, line, column,
             visibility, signature, source
             FROM symbols
             ORDER BY file_path, line, column, name",
        )?;
        let symbols = stmt
            .query_map([], row_to_symbol)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(symbols)
    }

    /// Get all symbols in a file
    ///
    /// # Arguments
    /// * `file_path` - File path (relative to project root)
    ///
    /// # Returns
    /// List of symbols in the file
    pub fn get_symbols_by_file(&self, file_path: &str) -> Result<Vec<Symbol>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, qualified_name, kind, file_path, line, column,
             visibility, signature, source FROM symbols WHERE file_path = ?1",
        )?;
        let symbols = stmt
            .query_map([file_path], row_to_symbol)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(symbols)
    }

    /// Search symbols by name using FTS5 ranking with a partial-match fallback.
    ///
    /// # Arguments
    /// * `name` - Symbol name or partial name
    /// * `limit` - Maximum results to return
    ///
    /// # Returns
    /// List of matching symbols
    pub fn search_symbols(&self, name: &str, limit: usize) -> Result<Vec<Symbol>, StorageError> {
        Ok(search_symbols_fts(&self.conn, name, limit)?)
    }

    /// Get symbols by kind
    ///
    /// # Arguments
    /// * `kind` - Symbol kind to filter
    ///
    /// # Returns
    /// List of symbols of the given kind
    pub fn get_symbols_by_kind(&self, kind: SymbolKind) -> Result<Vec<Symbol>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, qualified_name, kind, file_path, line, column,
             visibility, signature, source FROM symbols WHERE kind = ?1",
        )?;
        let symbols = stmt
            .query_map([kind.as_str()], row_to_symbol)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(symbols)
    }

    /// Update a symbol
    ///
    /// # Arguments
    /// * `symbol` - Symbol with updated values
    ///
    /// # Errors
    /// Returns error if symbol not found or update fails
    pub fn update_symbol(&self, symbol: &Symbol) -> Result<(), StorageError> {
        let now = current_timestamp();
        let rows = self.conn.execute(
            "UPDATE symbols SET name = ?1, qualified_name = ?2, kind = ?3,
             file_path = ?4, line = ?5, column = ?6, visibility = ?7,
             signature = ?8, source = ?9, updated_at = ?10 WHERE id = ?11",
            params![
                symbol.name,
                symbol.qualified_name,
                symbol.kind.as_str(),
                symbol.file_path,
                symbol.line,
                symbol.column,
                symbol.visibility.as_str(),
                symbol.signature,
                symbol.source.as_str(),
                now,
                symbol.id,
            ],
        )?;
        if rows == 0 {
            return Err(StorageError::SymbolNotFound(symbol.id.clone()));
        }
        Ok(())
    }

    /// Delete a symbol by ID
    ///
    /// Also deletes associated dependencies (cascade handled manually).
    ///
    /// # Arguments
    /// * `id` - Symbol ID
    ///
    /// # Errors
    /// Returns error if delete fails
    pub fn delete_symbol(&self, id: &str) -> Result<(), StorageError> {
        // Delete dependencies involving this symbol
        self.conn.execute(
            "DELETE FROM dependencies WHERE from_symbol = ?1 OR to_symbol = ?1",
            [id],
        )?;

        // Delete symbol
        self.conn
            .execute("DELETE FROM symbols WHERE id = ?1", [id])?;

        Ok(())
    }

    /// Delete all symbols in a file
    ///
    /// Also deletes associated dependencies.
    ///
    /// # Arguments
    /// * `file_path` - File path
    ///
    /// # Errors
    /// Returns error if delete fails
    pub fn delete_symbols_by_file(&self, file_path: &str) -> Result<(), StorageError> {
        // Get symbols in file
        let symbols = self.get_symbols_by_file(file_path)?;
        let symbol_ids: Vec<&str> = symbols.iter().map(|s| s.id.as_str()).collect();

        // Delete dependencies
        for id in &symbol_ids {
            self.conn.execute(
                "DELETE FROM dependencies WHERE from_symbol = ?1 OR to_symbol = ?1",
                [id],
            )?;
        }

        // Delete symbols
        self.conn
            .execute("DELETE FROM symbols WHERE file_path = ?1", [file_path])?;

        Ok(())
    }

    /// Delete all persisted data for a file.
    ///
    /// Removes symbols, imports, dependencies, and file-state records tied to
    /// the file. Dependencies targeting removed symbols are deleted as well.
    pub fn delete_file_data(&mut self, file_path: &str) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;

        let existing_symbols: Vec<String> = tx
            .prepare("SELECT id FROM symbols WHERE file_path = ?1")?
            .query_map([file_path], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;

        tx.execute("DELETE FROM dependencies WHERE from_file = ?1", [file_path])?;
        for id in &existing_symbols {
            tx.execute(
                "DELETE FROM dependencies WHERE from_symbol = ?1 OR to_symbol = ?1",
                [id],
            )?;
        }

        tx.execute("DELETE FROM symbols WHERE file_path = ?1", [file_path])?;
        tx.execute("DELETE FROM imports WHERE file_path = ?1", [file_path])?;
        tx.execute("DELETE FROM file_state WHERE path = ?1", [file_path])?;

        tx.commit()?;
        Ok(())
    }

    /// Count total symbols
    ///
    /// # Returns
    /// Total number of symbols in the database
    pub fn count_symbols(&self) -> Result<usize, StorageError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    // =========================================================================
    // Dependencies CRUD
    // =========================================================================

    /// Insert a dependency
    ///
    /// # Arguments
    /// * `dep` - Dependency to insert
    ///
    /// # Errors
    /// Returns error if insert fails
    pub fn insert_dependency(&self, dep: &Dependency) -> Result<(), StorageError> {
        let now = current_timestamp();
        self.conn.execute(
            "INSERT INTO dependencies (id, from_symbol, to_symbol, from_file, from_line, kind, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                dep.id,
                dep.from_symbol,
                dep.to_symbol,
                dep.from_file,
                dep.from_line,
                dep.kind.as_str(),
                now,
            ],
        )?;
        Ok(())
    }

    /// Get a dependency by ID
    ///
    /// # Arguments
    /// * `id` - Dependency ID
    ///
    /// # Returns
    /// The dependency, or None if not found
    pub fn get_dependency(&self, id: &str) -> Result<Option<Dependency>, StorageError> {
        let result = self
            .conn
            .query_row(
                "SELECT id, from_symbol, to_symbol, from_file, from_line, kind
                 FROM dependencies WHERE id = ?1",
                [id],
                row_to_dependency,
            )
            .optional()?;
        Ok(result)
    }

    /// Get outgoing dependencies (what a symbol depends on)
    ///
    /// # Arguments
    /// * `symbol_id` - Source symbol ID
    ///
    /// # Returns
    /// List of dependencies where this symbol is the source
    pub fn get_dependencies_from(&self, symbol_id: &str) -> Result<Vec<Dependency>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, from_symbol, to_symbol, from_file, from_line, kind
             FROM dependencies WHERE from_symbol = ?1",
        )?;
        let deps = stmt
            .query_map([symbol_id], row_to_dependency)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(deps)
    }

    /// Get incoming dependencies (what depends on a symbol)
    ///
    /// # Arguments
    /// * `symbol_id` - Target symbol ID
    ///
    /// # Returns
    /// List of dependencies where this symbol is the target
    pub fn get_dependencies_to(&self, symbol_id: &str) -> Result<Vec<Dependency>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, from_symbol, to_symbol, from_file, from_line, kind
             FROM dependencies WHERE to_symbol = ?1",
        )?;
        let deps = stmt
            .query_map([symbol_id], row_to_dependency)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(deps)
    }

    /// Get every dependency currently stored in the database.
    ///
    /// # Returns
    /// A full list of dependencies ordered by source file and line number.
    pub fn get_all_dependencies(&self) -> Result<Vec<Dependency>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, from_symbol, to_symbol, from_file, from_line, kind
             FROM dependencies
             ORDER BY from_file, from_line, from_symbol, to_symbol",
        )?;
        let deps = stmt
            .query_map([], row_to_dependency)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(deps)
    }

    /// Get all dependencies in a file
    ///
    /// # Arguments
    /// * `file_path` - File path
    ///
    /// # Returns
    /// List of dependencies originating from this file
    pub fn get_dependencies_by_file(
        &self,
        file_path: &str,
    ) -> Result<Vec<Dependency>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, from_symbol, to_symbol, from_file, from_line, kind
             FROM dependencies WHERE from_file = ?1",
        )?;
        let deps = stmt
            .query_map([file_path], row_to_dependency)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(deps)
    }

    /// Delete a dependency by ID
    ///
    /// # Arguments
    /// * `id` - Dependency ID
    ///
    /// # Errors
    /// Returns error if delete fails
    pub fn delete_dependency(&self, id: &str) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM dependencies WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Delete all dependencies involving a symbol
    ///
    /// # Arguments
    /// * `symbol_id` - Symbol ID
    ///
    /// # Errors
    /// Returns error if delete fails
    pub fn delete_dependencies_by_symbol(&self, symbol_id: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM dependencies WHERE from_symbol = ?1 OR to_symbol = ?1",
            [symbol_id],
        )?;
        Ok(())
    }

    /// Count total dependencies
    ///
    /// # Returns
    /// Total number of dependencies in the database
    pub fn count_dependencies(&self) -> Result<usize, StorageError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM dependencies", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    // =========================================================================
    // Imports CRUD
    // =========================================================================

    /// Insert an import
    ///
    /// # Arguments
    /// * `import` - Import to insert
    ///
    /// # Errors
    /// Returns error if insert fails
    pub fn insert_import(&self, import: &Import) -> Result<(), StorageError> {
        let now = current_timestamp();
        self.conn.execute(
            "INSERT INTO imports (id, source, alias, file_path, line, kind, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                import.id,
                import.source,
                import.alias,
                import.file_path,
                import.line,
                import.kind.as_str(),
                now,
            ],
        )?;
        Ok(())
    }

    /// Get imports by file
    ///
    /// # Arguments
    /// * `file_path` - File path
    ///
    /// # Returns
    /// List of imports in the file
    pub fn get_imports_by_file(&self, file_path: &str) -> Result<Vec<Import>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source, alias, file_path, line, kind FROM imports WHERE file_path = ?1",
        )?;
        let imports = stmt
            .query_map([file_path], row_to_import)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(imports)
    }

    /// Get every persisted import.
    pub fn get_all_imports(&self) -> Result<Vec<Import>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source, alias, file_path, line, kind FROM imports")?;
        let imports = stmt
            .query_map([], row_to_import)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(imports)
    }

    /// Delete all imports in a file
    ///
    /// # Arguments
    /// * `file_path` - File path
    ///
    /// # Errors
    /// Returns error if delete fails
    pub fn delete_imports_by_file(&self, file_path: &str) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM imports WHERE file_path = ?1", [file_path])?;
        Ok(())
    }

    /// Count total imports
    ///
    /// # Returns
    /// Total number of imports in the database
    pub fn count_imports(&self) -> Result<usize, StorageError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM imports", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    // =========================================================================
    // File State CRUD
    // =========================================================================

    /// Insert or update file state
    ///
    /// # Arguments
    /// * `state` - File state to insert/update
    ///
    /// # Errors
    /// Returns error if operation fails
    pub fn upsert_file_state(&self, state: &FileState) -> Result<(), StorageError> {
        let now = current_timestamp();
        self.conn.execute(
            "INSERT INTO file_state (path, hash, last_modified, status, error_message, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET
             hash = excluded.hash,
             last_modified = excluded.last_modified,
             status = excluded.status,
             error_message = excluded.error_message,
             updated_at = excluded.updated_at",
            params![
                state.path,
                state.hash,
                state.last_modified,
                state.status.as_str(),
                state.error_message,
                now,
            ],
        )?;
        Ok(())
    }

    /// Get file state by path
    ///
    /// # Arguments
    /// * `path` - File path
    ///
    /// # Returns
    /// File state, or None if not found
    pub fn get_file_state(&self, path: &str) -> Result<Option<FileState>, StorageError> {
        let result = self
            .conn
            .query_row(
                "SELECT path, hash, last_modified, status, error_message FROM file_state WHERE path = ?1",
                [path],
                row_to_file_state,
            )
            .optional()?;
        Ok(result)
    }

    /// Get all file states
    ///
    /// # Returns
    /// List of all file states in the database
    pub fn get_all_file_states(&self) -> Result<Vec<FileState>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, hash, last_modified, status, error_message FROM file_state")?;
        let states = stmt
            .query_map([], row_to_file_state)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(states)
    }

    /// Get file states by status
    ///
    /// # Arguments
    /// * `status` - File status to filter
    ///
    /// # Returns
    /// List of file states with the given status
    pub fn get_file_states_by_status(
        &self,
        status: FileStatus,
    ) -> Result<Vec<FileState>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT path, hash, last_modified, status, error_message FROM file_state WHERE status = ?1",
        )?;
        let states = stmt
            .query_map([status.as_str()], row_to_file_state)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(states)
    }

    /// Delete file state by path
    ///
    /// # Arguments
    /// * `path` - File path
    ///
    /// # Errors
    /// Returns error if delete fails
    pub fn delete_file_state(&self, path: &str) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM file_state WHERE path = ?1", [path])?;
        Ok(())
    }

    /// Count total files
    ///
    /// # Returns
    /// Total number of tracked files
    pub fn count_file_states(&self) -> Result<usize, StorageError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM file_state", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    // =========================================================================
    // Recursive CTE Graph Queries
    // =========================================================================

    /// Get dependency chain (forward: what does a symbol depend on)
    ///
    /// Uses recursive CTE to traverse the dependency graph.
    ///
    /// # Arguments
    /// * `symbol_id` - Starting symbol ID
    /// * `max_depth` - Maximum traversal depth
    ///
    /// # Returns
    /// List of symbols in the dependency chain, with depth info
    pub fn get_dependency_chain_forward(
        &self,
        symbol_id: &str,
        max_depth: u32,
    ) -> Result<Vec<DependencyNode>, StorageError> {
        // First get the starting symbol
        let _start_symbol = self
            .get_symbol(symbol_id)?
            .ok_or_else(|| StorageError::SymbolNotFound(symbol_id.to_string()))?;

        // Use recursive CTE to find all downstream dependencies
        let cte_sql = "
            WITH RECURSIVE dep_chain(symbol_id, depth, dep_kind) AS (
                -- Base case: start from the given symbol
                VALUES (?1, 0, NULL)
                UNION ALL
                -- Recursive case: find symbols this one depends on
                SELECT d.to_symbol, dc.depth + 1, d.kind
                FROM dep_chain dc
                JOIN dependencies d ON d.from_symbol = dc.symbol_id
                WHERE dc.depth < ?2
            )
            SELECT dc.symbol_id, dc.depth, s.name, s.qualified_name, s.file_path, dc.dep_kind
            FROM dep_chain dc
            JOIN symbols s ON s.id = dc.symbol_id
            ORDER BY dc.depth, s.name
        ";

        let mut stmt = self.conn.prepare(cte_sql)?;
        let nodes = stmt
            .query_map(params![symbol_id, max_depth], row_to_dependency_node)?
            .collect::<Result<Vec<_>, _>>()?;

        // Add the starting node with depth 0
        Ok(nodes)
    }

    /// Get dependency chain (backward: what depends on a symbol)
    ///
    /// Uses recursive CTE to traverse the dependency graph in reverse.
    ///
    /// # Arguments
    /// * `symbol_id` - Starting symbol ID
    /// * `max_depth` - Maximum traversal depth
    ///
    /// # Returns
    /// List of symbols that depend on the given symbol, with depth info
    pub fn get_dependency_chain_backward(
        &self,
        symbol_id: &str,
        max_depth: u32,
    ) -> Result<Vec<DependencyNode>, StorageError> {
        // First get the starting symbol
        let _start_symbol = self
            .get_symbol(symbol_id)?
            .ok_or_else(|| StorageError::SymbolNotFound(symbol_id.to_string()))?;

        // Use recursive CTE to find all upstream dependencies (dependents)
        let cte_sql = "
            WITH RECURSIVE dep_chain(symbol_id, depth, dep_kind) AS (
                -- Base case: start from the given symbol
                VALUES (?1, 0, NULL)
                UNION ALL
                -- Recursive case: find symbols that depend on this one
                SELECT d.from_symbol, dc.depth + 1, d.kind
                FROM dep_chain dc
                JOIN dependencies d ON d.to_symbol = dc.symbol_id
                WHERE dc.depth < ?2
            )
            SELECT dc.symbol_id, dc.depth, s.name, s.qualified_name, s.file_path, dc.dep_kind
            FROM dep_chain dc
            JOIN symbols s ON s.id = dc.symbol_id
            ORDER BY dc.depth, s.name
        ";

        let mut stmt = self.conn.prepare(cte_sql)?;
        let nodes = stmt
            .query_map(params![symbol_id, max_depth], row_to_dependency_node)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(nodes)
    }

    /// Get call chain path between two symbols
    ///
    /// Finds the shortest path from one symbol to another through dependencies.
    ///
    /// # Arguments
    /// * `from_symbol_id` - Source symbol ID
    /// * `to_symbol_id` - Target symbol ID
    /// * `max_depth` - Maximum search depth
    ///
    /// # Returns
    /// Path as list of symbols, or empty if no path found
    pub fn get_call_chain_path(
        &self,
        from_symbol_id: &str,
        to_symbol_id: &str,
        max_depth: u32,
    ) -> Result<Vec<DependencyNode>, StorageError> {
        let _from_symbol = self
            .get_symbol(from_symbol_id)?
            .ok_or_else(|| StorageError::SymbolNotFound(from_symbol_id.to_string()))?;
        let _to_symbol = self
            .get_symbol(to_symbol_id)?
            .ok_or_else(|| StorageError::SymbolNotFound(to_symbol_id.to_string()))?;

        use std::collections::{HashMap, VecDeque};

        #[derive(Debug, Clone)]
        struct Step {
            parent: Option<String>,
            depth: u32,
            dep_kind: Option<DependencyKind>,
        }

        let mut queue = VecDeque::from([from_symbol_id.to_string()]);
        let mut visited = HashMap::from([(
            from_symbol_id.to_string(),
            Step {
                parent: None,
                depth: 0,
                dep_kind: None,
            },
        )]);

        while let Some(current) = queue.pop_front() {
            let depth = visited
                .get(&current)
                .map(|step| step.depth)
                .unwrap_or_default();

            if depth >= max_depth {
                continue;
            }

            for dependency in self.get_dependencies_from(&current)? {
                if visited.contains_key(&dependency.to_symbol) {
                    continue;
                }

                visited.insert(
                    dependency.to_symbol.clone(),
                    Step {
                        parent: Some(current.clone()),
                        depth: depth + 1,
                        dep_kind: Some(dependency.kind.clone()),
                    },
                );

                if dependency.to_symbol == to_symbol_id {
                    queue.clear();
                    break;
                }

                queue.push_back(dependency.to_symbol);
            }
        }

        if !visited.contains_key(to_symbol_id) {
            return Ok(Vec::new());
        }

        let mut path = Vec::new();
        let mut current = to_symbol_id.to_string();

        loop {
            let step = visited
                .get(&current)
                .ok_or_else(|| StorageError::DependencyNotFound(current.clone()))?;
            path.push((current.clone(), step.depth, step.dep_kind.clone()));

            match &step.parent {
                Some(parent) => current = parent.clone(),
                None => break,
            }
        }

        path.reverse();

        let mut nodes = Vec::with_capacity(path.len());
        for (symbol_id, depth, dep_kind) in path {
            let symbol = self
                .get_symbol(&symbol_id)?
                .ok_or_else(|| StorageError::SymbolNotFound(symbol_id.clone()))?;
            nodes.push(DependencyNode {
                symbol_id,
                name: symbol.name,
                qualified_name: symbol.qualified_name,
                file_path: symbol.file_path,
                depth,
                dep_kind,
            });
        }

        Ok(nodes)
    }

    /// Get impact radius for changed files
    ///
    /// Finds all symbols affected by changes in the given files.
    /// Uses recursive CTE for efficient traversal.
    ///
    /// # Arguments
    /// * `file_paths` - List of changed file paths
    /// * `max_depth` - Maximum traversal depth
    ///
    /// # Returns
    /// List of potentially impacted symbols
    pub fn get_impact_radius(
        &self,
        file_paths: &[String],
        max_depth: u32,
    ) -> Result<Vec<DependencyNode>, StorageError> {
        if file_paths.is_empty() {
            return Ok(Vec::new());
        }

        // Get symbols in changed files
        let changed_symbols: Vec<String> = file_paths
            .iter()
            .flat_map(|fp| self.get_symbols_by_file(fp).unwrap_or_default())
            .map(|s| s.id)
            .collect();

        if changed_symbols.is_empty() {
            return Ok(Vec::new());
        }

        // Build temp table for seeds
        self.conn.execute(
            "CREATE TEMP TABLE IF NOT EXISTS _impact_seeds (symbol_id TEXT PRIMARY KEY)",
            [],
        )?;
        self.conn.execute("DELETE FROM _impact_seeds", [])?;

        // Insert seeds in batches
        let batch_size = 450;
        for chunk in changed_symbols.chunks(batch_size) {
            let placeholders = chunk.iter().map(|_| "(?)").collect::<Vec<_>>().join(",");
            self.conn.execute(
                &format!(
                    "INSERT OR IGNORE INTO _impact_seeds (symbol_id) VALUES {}",
                    placeholders
                ),
                rusqlite::params_from_iter(chunk),
            )?;
        }

        // Use recursive CTE to find impacted symbols
        let cte_sql = "
            WITH RECURSIVE impacted(symbol_id, depth) AS (
                SELECT symbol_id, 0 FROM _impact_seeds
                UNION ALL
                SELECT d.from_symbol, i.depth + 1
                FROM impacted i
                JOIN dependencies d ON d.to_symbol = i.symbol_id
                WHERE i.depth < ?
            )
            SELECT DISTINCT i.symbol_id, MIN(i.depth) as depth, s.name, s.qualified_name, s.file_path, d.kind
            FROM impacted i
            JOIN symbols s ON s.id = i.symbol_id
            LEFT JOIN dependencies d ON d.to_symbol = i.symbol_id AND i.depth > 0
            WHERE i.depth > 0
            GROUP BY i.symbol_id
            ORDER BY depth, s.name
        ";

        let mut stmt = self.conn.prepare(cte_sql)?;
        let nodes = stmt
            .query_map([max_depth], row_to_dependency_node)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(nodes)
    }

    // =========================================================================
    // Batch Operations (Transaction-based)
    // =========================================================================

    /// Batch insert symbols
    ///
    /// Uses a transaction for efficient bulk insert.
    ///
    /// # Arguments
    /// * `symbols` - List of symbols to insert
    ///
    /// # Errors
    /// Returns error if any insert fails; entire batch is rolled back
    pub fn batch_insert_symbols(&mut self, symbols: &[Symbol]) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        let now = current_timestamp();

        for symbol in symbols {
            tx.execute(
                "INSERT INTO symbols (id, name, qualified_name, kind, file_path, line, column,
                 visibility, signature, source, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(qualified_name) DO UPDATE SET
                 id = excluded.id,
                 name = excluded.name,
                 kind = excluded.kind,
                 file_path = excluded.file_path,
                 line = excluded.line,
                 column = excluded.column,
                 visibility = excluded.visibility,
                 signature = excluded.signature,
                 source = excluded.source,
                 updated_at = excluded.updated_at",
                params![
                    symbol.id,
                    symbol.name,
                    symbol.qualified_name,
                    symbol.kind.as_str(),
                    symbol.file_path,
                    symbol.line,
                    symbol.column,
                    symbol.visibility.as_str(),
                    symbol.signature,
                    symbol.source.as_str(),
                    now,
                    now,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Batch insert dependencies
    ///
    /// Uses a transaction for efficient bulk insert.
    ///
    /// # Arguments
    /// * `deps` - List of dependencies to insert
    ///
    /// # Errors
    /// Returns error if any insert fails; entire batch is rolled back
    pub fn batch_insert_dependencies(&mut self, deps: &[Dependency]) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        let now = current_timestamp();

        for dep in deps {
            tx.execute(
                "INSERT INTO dependencies (id, from_symbol, to_symbol, from_file, from_line, kind, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    dep.id,
                    dep.from_symbol,
                    dep.to_symbol,
                    dep.from_file,
                    dep.from_line,
                    dep.kind.as_str(),
                    now,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Batch insert imports
    ///
    /// Uses a transaction for efficient bulk insert.
    ///
    /// # Arguments
    /// * `imports` - List of imports to insert
    ///
    /// # Errors
    /// Returns error if any insert fails; entire batch is rolled back
    pub fn batch_insert_imports(&mut self, imports: &[Import]) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        let now = current_timestamp();

        for import in imports {
            tx.execute(
                "INSERT INTO imports (id, source, alias, file_path, line, kind, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    import.id,
                    import.source,
                    import.alias,
                    import.file_path,
                    import.line,
                    import.kind.as_str(),
                    now,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Replace all data for a file (symbols, dependencies, imports)
    ///
    /// Uses a transaction to atomically replace file data.
    ///
    /// # Arguments
    /// * `file_path` - File path
    /// * `symbols` - New symbols for the file
    /// * `deps` - New dependencies for the file
    /// * `imports` - New imports for the file
    /// * `file_state` - New file state
    ///
    /// # Errors
    /// Returns error if operation fails; entire replacement is rolled back
    pub fn replace_file_data(
        &mut self,
        file_path: &str,
        symbols: &[Symbol],
        deps: &[Dependency],
        imports: &[Import],
        file_state: &FileState,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        let now = current_timestamp();

        let existing_symbols: Vec<String> = tx
            .prepare("SELECT id FROM symbols WHERE file_path = ?1")?
            .query_map([file_path], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let new_symbol_ids = symbols
            .iter()
            .map(|symbol| symbol.id.as_str())
            .collect::<std::collections::HashSet<_>>();

        // Outgoing dependencies from this file are always rebuilt.
        tx.execute("DELETE FROM dependencies WHERE from_file = ?1", [file_path])?;

        // Only remove incoming dependencies for symbols that disappeared.
        for id in &existing_symbols {
            if !new_symbol_ids.contains(id.as_str()) {
                tx.execute(
                    "DELETE FROM dependencies WHERE from_symbol = ?1 OR to_symbol = ?1",
                    [id],
                )?;
            }
        }

        // Delete existing symbols, imports
        tx.execute("DELETE FROM symbols WHERE file_path = ?1", [file_path])?;
        tx.execute("DELETE FROM imports WHERE file_path = ?1", [file_path])?;

        // Insert new symbols
        for symbol in symbols {
            tx.execute(
                "INSERT INTO symbols (id, name, qualified_name, kind, file_path, line, column,
                 visibility, signature, source, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(qualified_name) DO UPDATE SET
                 id = excluded.id,
                 name = excluded.name,
                 kind = excluded.kind,
                 file_path = excluded.file_path,
                 line = excluded.line,
                 column = excluded.column,
                 visibility = excluded.visibility,
                 signature = excluded.signature,
                 source = excluded.source,
                 updated_at = excluded.updated_at",
                params![
                    symbol.id,
                    symbol.name,
                    symbol.qualified_name,
                    symbol.kind.as_str(),
                    symbol.file_path,
                    symbol.line,
                    symbol.column,
                    symbol.visibility.as_str(),
                    symbol.signature,
                    symbol.source.as_str(),
                    now,
                    now,
                ],
            )?;
        }

        // Insert new dependencies
        for dep in deps {
            tx.execute(
                "INSERT INTO dependencies (id, from_symbol, to_symbol, from_file, from_line, kind, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    dep.id,
                    dep.from_symbol,
                    dep.to_symbol,
                    dep.from_file,
                    dep.from_line,
                    dep.kind.as_str(),
                    now,
                ],
            )?;
        }

        // Insert new imports
        for import in imports {
            tx.execute(
                "INSERT INTO imports (id, source, alias, file_path, line, kind, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    import.id,
                    import.source,
                    import.alias,
                    import.file_path,
                    import.line,
                    import.kind.as_str(),
                    now,
                ],
            )?;
        }

        // Upsert file state
        tx.execute(
            "INSERT INTO file_state (path, hash, last_modified, status, error_message, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET
             hash = excluded.hash,
             last_modified = excluded.last_modified,
             status = excluded.status,
             error_message = excluded.error_message,
             updated_at = excluded.updated_at",
            params![
                file_state.path,
                file_state.hash,
                file_state.last_modified,
                file_state.status.as_str(),
                file_state.error_message,
                now,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    // =========================================================================
    // Metadata Operations
    // =========================================================================

    /// Set metadata key-value
    ///
    /// # Arguments
    /// * `key` - Metadata key
    /// * `value` - Metadata value
    pub fn set_metadata(&self, key: &str, value: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES (?1, ?2)",
            [key, value],
        )?;
        Ok(())
    }

    /// Get metadata value
    ///
    /// # Arguments
    /// * `key` - Metadata key
    ///
    /// # Returns
    /// Metadata value, or None if not found
    pub fn get_metadata(&self, key: &str) -> Result<Option<String>, StorageError> {
        let result = self
            .conn
            .query_row("SELECT value FROM metadata WHERE key = ?1", [key], |row| {
                row.get::<_, String>(0)
            })
            .optional()?;
        Ok(result)
    }

    // =========================================================================
    // Utility Operations
    // =========================================================================

    /// Clear all data (keep schema)
    ///
    /// Useful for rebuilding the database.
    ///
    /// # Errors
    /// Returns error if operation fails
    pub fn clear_all(&self) -> Result<(), StorageError> {
        self.conn.execute("DELETE FROM dependencies", [])?;
        self.conn.execute("DELETE FROM symbols", [])?;
        self.conn.execute("DELETE FROM imports", [])?;
        self.conn.execute("DELETE FROM file_state", [])?;
        self.conn.execute("DELETE FROM scan_state", [])?;
        Ok(())
    }

    /// Get storage statistics
    ///
    /// # Returns
    /// Stats as a map of key to value
    pub fn get_stats(&self) -> Result<std::collections::HashMap<String, usize>, StorageError> {
        let mut stats = std::collections::HashMap::new();
        stats.insert("symbols".to_string(), self.count_symbols()?);
        stats.insert("dependencies".to_string(), self.count_dependencies()?);
        stats.insert("imports".to_string(), self.count_imports()?);
        stats.insert("files".to_string(), self.count_file_states()?);
        Ok(stats)
    }

    /// Check if database needs rebuild
    ///
    /// # Returns
    /// true if schema version mismatch
    pub fn needs_rebuild(&self) -> bool {
        needs_rebuild(&self.conn)
    }
}

// Helper functions

/// Get current timestamp in seconds
fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Convert row to Symbol
fn row_to_symbol(row: &Row) -> rusqlite::Result<Symbol> {
    Ok(Symbol {
        id: row.get(0)?,
        name: row.get(1)?,
        qualified_name: row.get(2)?,
        kind: row
            .get::<_, String>(3)?
            .parse()
            .unwrap_or(SymbolKind::Function),
        file_path: row.get(4)?,
        line: row.get::<_, i64>(5)? as u32,
        column: row.get::<_, i64>(6)? as u32,
        visibility: row
            .get::<_, String>(7)?
            .parse()
            .unwrap_or(Visibility::Private),
        signature: row.get(8)?,
        source: row
            .get::<_, String>(9)?
            .parse()
            .unwrap_or(SymbolSource::Local),
    })
}

/// Convert row to Dependency
fn row_to_dependency(row: &Row) -> rusqlite::Result<Dependency> {
    Ok(Dependency {
        id: row.get(0)?,
        from_symbol: row.get(1)?,
        to_symbol: row.get(2)?,
        from_file: row.get(3)?,
        from_line: row.get::<_, i64>(4)? as u32,
        kind: row
            .get::<_, String>(5)?
            .parse()
            .unwrap_or(DependencyKind::Call),
    })
}

/// Convert row to Import
fn row_to_import(row: &Row) -> rusqlite::Result<Import> {
    Ok(Import {
        id: row.get(0)?,
        source: row.get(1)?,
        alias: row.get(2)?,
        file_path: row.get(3)?,
        line: row.get::<_, i64>(4)? as u32,
        kind: row
            .get::<_, String>(5)?
            .parse()
            .unwrap_or(ImportKind::Named),
    })
}

/// Convert row to FileState
fn row_to_file_state(row: &Row) -> rusqlite::Result<FileState> {
    Ok(FileState {
        path: row.get(0)?,
        hash: row.get(1)?,
        last_modified: row.get::<_, i64>(2)? as u64,
        status: row.get::<_, String>(3)?.parse().unwrap_or(FileStatus::Ok),
        error_message: row.get(4)?,
    })
}

/// Convert row to DependencyNode
fn row_to_dependency_node(row: &Row) -> rusqlite::Result<DependencyNode> {
    let kind_str: Option<String> = row.get(5)?;
    let dep_kind = kind_str.and_then(|s| s.parse().ok());

    Ok(DependencyNode {
        symbol_id: row.get(0)?,
        name: row.get(2)?,
        qualified_name: row.get(3)?,
        file_path: row.get(4)?,
        depth: row.get::<_, i64>(1)? as u32,
        dep_kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn create_test_storage() -> (Storage, NamedTempFile) {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = Storage::new(temp_file.path()).unwrap();
        (storage, temp_file)
    }

    fn create_test_storage_mut() -> (Storage, NamedTempFile) {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = Storage::new(temp_file.path()).unwrap();
        (storage, temp_file)
    }

    #[test]
    fn test_storage_new() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = Storage::new(temp_file.path()).unwrap();
        assert!(storage.count_symbols().unwrap() == 0);
    }

    #[test]
    fn test_insert_and_get_symbol() {
        let (storage, _) = create_test_storage();

        let symbol = Symbol::new(
            "test_func".to_string(),
            "src/main.rs::test_func".to_string(),
            SymbolKind::Function,
            "src/main.rs".to_string(),
            10,
            5,
        )
        .with_visibility(Visibility::Public);

        storage.insert_symbol(&symbol).unwrap();

        let retrieved = storage.get_symbol(&symbol.id).unwrap().unwrap();
        assert_eq!(retrieved.name, "test_func");
        assert_eq!(retrieved.kind, SymbolKind::Function);
    }

    #[test]
    fn test_get_symbol_by_qualified_name() {
        let (storage, _) = create_test_storage();

        let symbol = Symbol::new(
            "helper".to_string(),
            "src/utils.rs::helper".to_string(),
            SymbolKind::Function,
            "src/utils.rs".to_string(),
            15,
            1,
        );

        storage.insert_symbol(&symbol).unwrap();

        let retrieved = storage
            .get_symbol_by_qualified_name("src/utils.rs::helper")
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.name, "helper");
    }

    #[test]
    fn test_search_symbols() {
        let (storage, _) = create_test_storage();

        let symbol1 = Symbol::new(
            "process_data".to_string(),
            "src/lib.rs::process_data".to_string(),
            SymbolKind::Function,
            "src/lib.rs".to_string(),
            10,
            1,
        );

        let symbol2 = Symbol::new(
            "DataProcessor".to_string(),
            "src/lib.rs::DataProcessor".to_string(),
            SymbolKind::Class,
            "src/lib.rs".to_string(),
            20,
            1,
        );

        storage.insert_symbol(&symbol1).unwrap();
        storage.insert_symbol(&symbol2).unwrap();

        let results = storage.search_symbols("process", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_update_symbol() {
        let (storage, _) = create_test_storage();

        let mut symbol = Symbol::new(
            "old_name".to_string(),
            "src/main.rs::old_name".to_string(),
            SymbolKind::Function,
            "src/main.rs".to_string(),
            10,
            5,
        );

        storage.insert_symbol(&symbol).unwrap();

        symbol.name = "new_name".to_string();
        symbol.line = 20;
        storage.update_symbol(&symbol).unwrap();

        let retrieved = storage.get_symbol(&symbol.id).unwrap().unwrap();
        assert_eq!(retrieved.name, "new_name");
        assert_eq!(retrieved.line, 20);
    }

    #[test]
    fn test_delete_symbol() {
        let (storage, _) = create_test_storage();

        let symbol = Symbol::new(
            "test".to_string(),
            "src/main.rs::test".to_string(),
            SymbolKind::Function,
            "src/main.rs".to_string(),
            10,
            5,
        );

        storage.insert_symbol(&symbol).unwrap();
        assert!(storage.get_symbol(&symbol.id).unwrap().is_some());

        storage.delete_symbol(&symbol.id).unwrap();
        assert!(storage.get_symbol(&symbol.id).unwrap().is_none());
    }

    #[test]
    fn test_dependency_crud() {
        let (storage, _) = create_test_storage();

        let symbol1 = Symbol::new(
            "caller".to_string(),
            "src/main.rs::caller".to_string(),
            SymbolKind::Function,
            "src/main.rs".to_string(),
            10,
            1,
        );
        let symbol2 = Symbol::new(
            "callee".to_string(),
            "src/utils.rs::callee".to_string(),
            SymbolKind::Function,
            "src/utils.rs".to_string(),
            5,
            1,
        );

        storage.insert_symbol(&symbol1).unwrap();
        storage.insert_symbol(&symbol2).unwrap();

        let dep = Dependency::new(
            symbol1.id.clone(),
            symbol2.id.clone(),
            "src/main.rs".to_string(),
            15,
            DependencyKind::Call,
        );

        storage.insert_dependency(&dep).unwrap();

        let deps_from = storage.get_dependencies_from(&symbol1.id).unwrap();
        assert_eq!(deps_from.len(), 1);
        assert_eq!(deps_from[0].to_symbol, symbol2.id);

        let deps_to = storage.get_dependencies_to(&symbol2.id).unwrap();
        assert_eq!(deps_to.len(), 1);
        assert_eq!(deps_to[0].from_symbol, symbol1.id);
    }

    #[test]
    fn test_import_crud() {
        let (storage, _) = create_test_storage();

        let import = Import::new(
            "std::collections::HashMap".to_string(),
            "src/main.rs".to_string(),
            1,
            ImportKind::Named,
        );

        storage.insert_import(&import).unwrap();

        let imports = storage.get_imports_by_file("src/main.rs").unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "std::collections::HashMap");
    }

    #[test]
    fn test_file_state_crud() {
        let (storage, _) = create_test_storage();

        let state = FileState::new("src/main.rs".to_string(), "abc123".to_string(), 12345);

        storage.upsert_file_state(&state).unwrap();

        let retrieved = storage.get_file_state("src/main.rs").unwrap().unwrap();
        assert_eq!(retrieved.hash, "abc123");
        assert_eq!(retrieved.status, FileStatus::Ok);

        // Update with error
        let error_state = FileState::new("src/main.rs".to_string(), "def456".to_string(), 12346)
            .with_error("Parse error".to_string());

        storage.upsert_file_state(&error_state).unwrap();

        let retrieved = storage.get_file_state("src/main.rs").unwrap().unwrap();
        assert_eq!(retrieved.status, FileStatus::Failed);
        assert_eq!(retrieved.error_message, Some("Parse error".to_string()));
    }

    #[test]
    fn test_batch_insert_symbols() {
        let (mut storage, _) = create_test_storage_mut();

        let symbols: Vec<Symbol> = (0..10)
            .map(|i| {
                Symbol::new(
                    format!("func{}", i),
                    format!("src/lib.rs::func{}", i),
                    SymbolKind::Function,
                    "src/lib.rs".to_string(),
                    i * 10 + 1,
                    1,
                )
            })
            .collect();

        storage.batch_insert_symbols(&symbols).unwrap();

        assert_eq!(storage.count_symbols().unwrap(), 10);
    }

    #[test]
    fn test_batch_insert_symbols_upserts_duplicate_qualified_name() {
        let (mut storage, _) = create_test_storage_mut();

        let original = Symbol::new(
            "helper".into(),
            "src/main.cpp::helper".into(),
            SymbolKind::Function,
            "src/main.cpp".into(),
            1,
            1,
        );
        let updated = Symbol::new(
            "helper".into(),
            "src/main.cpp::helper".into(),
            SymbolKind::Function,
            "src/main.cpp".into(),
            20,
            4,
        )
        .with_signature("helper(int value)".into());

        storage
            .batch_insert_symbols(&[original, updated.clone()])
            .unwrap();

        let retrieved = storage
            .get_symbol_by_qualified_name("src/main.cpp::helper")
            .unwrap()
            .expect("symbol should exist");
        assert_eq!(
            storage.get_symbols_by_file("src/main.cpp").unwrap().len(),
            1
        );
        assert_eq!(retrieved.line, updated.line);
        assert_eq!(retrieved.signature, updated.signature);
    }

    #[test]
    fn test_dependency_chain_forward() {
        let (storage, _) = create_test_storage();

        // Create chain: func1 -> func2 -> func3
        let func1 = Symbol::new(
            "func1".into(),
            "src/a.rs::func1".into(),
            SymbolKind::Function,
            "src/a.rs".into(),
            1,
            1,
        );
        let func2 = Symbol::new(
            "func2".into(),
            "src/b.rs::func2".into(),
            SymbolKind::Function,
            "src/b.rs".into(),
            1,
            1,
        );
        let func3 = Symbol::new(
            "func3".into(),
            "src/c.rs::func3".into(),
            SymbolKind::Function,
            "src/c.rs".into(),
            1,
            1,
        );

        storage.insert_symbol(&func1).unwrap();
        storage.insert_symbol(&func2).unwrap();
        storage.insert_symbol(&func3).unwrap();

        let dep1 = Dependency::new(
            func1.id.clone(),
            func2.id.clone(),
            "src/a.rs".into(),
            5,
            DependencyKind::Call,
        );
        let dep2 = Dependency::new(
            func2.id.clone(),
            func3.id.clone(),
            "src/b.rs".into(),
            5,
            DependencyKind::Call,
        );

        storage.insert_dependency(&dep1).unwrap();
        storage.insert_dependency(&dep2).unwrap();

        let chain = storage.get_dependency_chain_forward(&func1.id, 5).unwrap();
        assert!(chain.len() >= 3);

        // Check depth ordering
        assert!(chain
            .iter()
            .any(|n| n.symbol_id == func1.id && n.depth == 0));
        assert!(chain
            .iter()
            .any(|n| n.symbol_id == func2.id && n.depth == 1));
        assert!(chain
            .iter()
            .any(|n| n.symbol_id == func3.id && n.depth == 2));
        assert_eq!(
            chain
                .iter()
                .find(|n| n.symbol_id == func2.id)
                .unwrap()
                .dep_kind,
            Some(DependencyKind::Call)
        );
        assert_eq!(
            chain
                .iter()
                .find(|n| n.symbol_id == func3.id)
                .unwrap()
                .dep_kind,
            Some(DependencyKind::Call)
        );
    }

    #[test]
    fn test_dependency_chain_backward() {
        let (storage, _) = create_test_storage();

        // Create chain: func1 -> func2 -> func3
        let func1 = Symbol::new(
            "func1".into(),
            "src/a.rs::func1".into(),
            SymbolKind::Function,
            "src/a.rs".into(),
            1,
            1,
        );
        let func2 = Symbol::new(
            "func2".into(),
            "src/b.rs::func2".into(),
            SymbolKind::Function,
            "src/b.rs".into(),
            1,
            1,
        );
        let func3 = Symbol::new(
            "func3".into(),
            "src/c.rs::func3".into(),
            SymbolKind::Function,
            "src/c.rs".into(),
            1,
            1,
        );

        storage.insert_symbol(&func1).unwrap();
        storage.insert_symbol(&func2).unwrap();
        storage.insert_symbol(&func3).unwrap();

        let dep1 = Dependency::new(
            func1.id.clone(),
            func2.id.clone(),
            "src/a.rs".into(),
            5,
            DependencyKind::Call,
        );
        let dep2 = Dependency::new(
            func2.id.clone(),
            func3.id.clone(),
            "src/b.rs".into(),
            5,
            DependencyKind::Call,
        );

        storage.insert_dependency(&dep1).unwrap();
        storage.insert_dependency(&dep2).unwrap();

        let chain = storage.get_dependency_chain_backward(&func3.id, 5).unwrap();
        assert!(chain.len() >= 3);

        // Check depth ordering (backward)
        assert!(chain
            .iter()
            .any(|n| n.symbol_id == func3.id && n.depth == 0));
        assert!(chain
            .iter()
            .any(|n| n.symbol_id == func2.id && n.depth == 1));
        assert!(chain
            .iter()
            .any(|n| n.symbol_id == func1.id && n.depth == 2));
        assert_eq!(
            chain
                .iter()
                .find(|n| n.symbol_id == func2.id)
                .unwrap()
                .dep_kind,
            Some(DependencyKind::Call)
        );
        assert_eq!(
            chain
                .iter()
                .find(|n| n.symbol_id == func1.id)
                .unwrap()
                .dep_kind,
            Some(DependencyKind::Call)
        );
    }

    #[test]
    fn test_dependency_chain_forward_does_not_duplicate_parent_for_branching_kinds() {
        let (storage, _) = create_test_storage();

        let root = Symbol::new(
            "root".into(),
            "src/a.rs::root".into(),
            SymbolKind::Function,
            "src/a.rs".into(),
            1,
            1,
        );
        let branch_a = Symbol::new(
            "branch_a".into(),
            "src/b.rs::branch_a".into(),
            SymbolKind::Function,
            "src/b.rs".into(),
            1,
            1,
        );
        let branch_b = Symbol::new(
            "branch_b".into(),
            "src/c.rs::branch_b".into(),
            SymbolKind::Function,
            "src/c.rs".into(),
            1,
            1,
        );

        storage.insert_symbol(&root).unwrap();
        storage.insert_symbol(&branch_a).unwrap();
        storage.insert_symbol(&branch_b).unwrap();
        storage
            .insert_dependency(&Dependency::new(
                root.id.clone(),
                branch_a.id.clone(),
                "src/a.rs".into(),
                10,
                DependencyKind::Call,
            ))
            .unwrap();
        storage
            .insert_dependency(&Dependency::new(
                root.id.clone(),
                branch_b.id.clone(),
                "src/a.rs".into(),
                11,
                DependencyKind::TypeUse,
            ))
            .unwrap();

        let chain = storage.get_dependency_chain_forward(&root.id, 1).unwrap();
        assert_eq!(chain.len(), 3);
        assert_eq!(
            chain
                .iter()
                .filter(|node| node.symbol_id == root.id)
                .count(),
            1
        );
        assert_eq!(
            chain
                .iter()
                .find(|n| n.symbol_id == branch_a.id)
                .unwrap()
                .dep_kind,
            Some(DependencyKind::Call)
        );
        assert_eq!(
            chain
                .iter()
                .find(|n| n.symbol_id == branch_b.id)
                .unwrap()
                .dep_kind,
            Some(DependencyKind::TypeUse)
        );
    }

    #[test]
    fn test_call_chain_path_returns_full_path() {
        let (storage, _) = create_test_storage();

        let func1 = Symbol::new(
            "func1".into(),
            "src/a.rs::func1".into(),
            SymbolKind::Function,
            "src/a.rs".into(),
            1,
            1,
        );
        let func2 = Symbol::new(
            "func2".into(),
            "src/b.rs::func2".into(),
            SymbolKind::Function,
            "src/b.rs".into(),
            1,
            1,
        );
        let func3 = Symbol::new(
            "func3".into(),
            "src/c.rs::func3".into(),
            SymbolKind::Function,
            "src/c.rs".into(),
            1,
            1,
        );

        storage.insert_symbol(&func1).unwrap();
        storage.insert_symbol(&func2).unwrap();
        storage.insert_symbol(&func3).unwrap();

        storage
            .insert_dependency(&Dependency::new(
                func1.id.clone(),
                func2.id.clone(),
                "src/a.rs".into(),
                5,
                DependencyKind::Call,
            ))
            .unwrap();
        storage
            .insert_dependency(&Dependency::new(
                func2.id.clone(),
                func3.id.clone(),
                "src/b.rs".into(),
                8,
                DependencyKind::Call,
            ))
            .unwrap();

        let path = storage
            .get_call_chain_path(&func1.id, &func3.id, 5)
            .unwrap();
        let ids = path
            .iter()
            .map(|node| node.symbol_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![func1.id.as_str(), func2.id.as_str(), func3.id.as_str()]
        );
        assert_eq!(path[0].depth, 0);
        assert_eq!(path[1].depth, 1);
        assert_eq!(path[2].depth, 2);
        assert_eq!(path[1].dep_kind, Some(DependencyKind::Call));
        assert_eq!(path[2].dep_kind, Some(DependencyKind::Call));
    }

    #[test]
    fn test_replace_file_data() {
        let (mut storage, _) = create_test_storage_mut();

        // Initial data
        let old_symbol = Symbol::new(
            "old".into(),
            "src/main.rs::old".into(),
            SymbolKind::Function,
            "src/main.rs".into(),
            1,
            1,
        );
        storage.insert_symbol(&old_symbol).unwrap();

        // New data
        let new_symbols: Vec<Symbol> = vec![
            Symbol::new(
                "new1".into(),
                "src/main.rs::new1".into(),
                SymbolKind::Function,
                "src/main.rs".into(),
                10,
                1,
            ),
            Symbol::new(
                "new2".into(),
                "src/main.rs::new2".into(),
                SymbolKind::Function,
                "src/main.rs".into(),
                20,
                1,
            ),
        ];

        let file_state = FileState::new("src/main.rs".into(), "new_hash".into(), 12345);

        storage
            .replace_file_data("src/main.rs", &new_symbols, &[], &[], &file_state)
            .unwrap();

        // Verify old symbol deleted
        assert!(storage.get_symbol(&old_symbol.id).unwrap().is_none());

        // Verify new symbols exist
        assert_eq!(storage.get_symbols_by_file("src/main.rs").unwrap().len(), 2);
    }

    #[test]
    fn test_replace_file_data_preserves_incoming_dependencies_for_stable_symbols() {
        let (mut storage, _) = create_test_storage_mut();

        let helper = Symbol::new(
            "helper".into(),
            "src/utils.rs::helper".into(),
            SymbolKind::Function,
            "src/utils.rs".into(),
            1,
            1,
        );
        let caller = Symbol::new(
            "caller".into(),
            "src/main.rs::caller".into(),
            SymbolKind::Function,
            "src/main.rs".into(),
            1,
            1,
        );

        storage.insert_symbol(&helper).unwrap();
        storage.insert_symbol(&caller).unwrap();
        storage
            .insert_dependency(&Dependency::new(
                caller.id.clone(),
                helper.id.clone(),
                "src/main.rs".into(),
                2,
                DependencyKind::Call,
            ))
            .unwrap();

        let refreshed_helper = Symbol::new(
            "helper".into(),
            "src/utils.rs::helper".into(),
            SymbolKind::Function,
            "src/utils.rs".into(),
            10,
            4,
        );
        let file_state = FileState::new("src/utils.rs".into(), "new_hash".into(), 12345);

        storage
            .replace_file_data(
                "src/utils.rs",
                std::slice::from_ref(&refreshed_helper),
                &[],
                &[],
                &file_state,
            )
            .unwrap();

        let incoming = storage.get_dependencies_to(&refreshed_helper.id).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].from_symbol, caller.id);
    }

    #[test]
    fn test_metadata() {
        let (storage, _) = create_test_storage();

        storage.set_metadata("last_scan", "2024-01-01").unwrap();
        let value = storage.get_metadata("last_scan").unwrap();
        assert_eq!(value, Some("2024-01-01".to_string()));
    }

    #[test]
    fn test_stats() {
        let (storage, _) = create_test_storage();

        let symbol = Symbol::new(
            "test".into(),
            "src/main.rs::test".into(),
            SymbolKind::Function,
            "src/main.rs".into(),
            1,
            1,
        );
        storage.insert_symbol(&symbol).unwrap();

        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.get("symbols"), Some(&1));
    }
}
