//! Import statement parsing and resolution
//!
//! Handles parsing of import statements from various languages,
//! extracting module paths, aliases, and import kinds.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Import kind (type of import statement)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportKind {
    /// Named import: `use crate::module::Item` or `import { Item } from 'module'`
    Named,
    /// Glob import: `use crate::module::*` or `import * from 'module'`
    Glob,
    /// Self import: `use crate::module::self` (imports the module itself)
    SelfImport,
    /// Alias import: `use crate::module::Item as Alias` or `import Item as Alias`
    Alias,
    /// Re-exported named import: `export { Item } from 'module'`
    ReExportNamed,
    /// Re-exported glob import: `export * from 'module'`
    ReExportGlob,
    /// Re-exported aliased import: `export { Item as Alias } from 'module'`
    ReExportAlias,
}

/// An import statement in source code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    /// Unique identifier (UUID)
    pub id: String,

    /// Import source path (e.g., "crate::module::Item", "lodash")
    pub source: String,

    /// Alias name if present (e.g., "Alias" in `use Item as Alias`)
    pub alias: Option<String>,

    /// File path where the import occurs (relative to project root)
    pub file_path: String,

    /// Line number (1-based)
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

    /// Check if this is a glob import
    pub fn is_glob(&self) -> bool {
        matches!(self.kind, ImportKind::Glob | ImportKind::ReExportGlob)
    }

    /// Check if this import has an alias
    pub fn has_alias(&self) -> bool {
        self.alias.is_some()
    }

    /// Get the effective name for this import
    /// Returns alias if present, otherwise the last segment of source
    pub fn effective_name(&self) -> Option<&str> {
        if let Some(ref alias) = self.alias {
            return Some(alias);
        }
        // For named imports and re-exports, extract the last segment.
        self.source.split("::").last()
    }

    /// Check if this import represents a re-export.
    pub fn is_reexport(&self) -> bool {
        matches!(
            self.kind,
            ImportKind::ReExportNamed | ImportKind::ReExportGlob | ImportKind::ReExportAlias
        )
    }
}

/// Trait for language-specific import parsers
///
/// Each language (Rust, TypeScript, Python, Go) should implement
/// this trait to parse import statements from source code.
pub trait ImportParser {
    /// Parse imports from source code content
    ///
    /// # Arguments
    /// * `content` - Source code content
    /// * `file_path` - Path to the file being parsed (relative to project root)
    ///
    /// # Returns
    /// Vector of Import structures found in the content
    fn parse_imports(content: &str, file_path: &Path) -> Vec<Import>;
}

/// Rust import parser (handles `use` statements)
///
/// Parses Rust `use` declarations:
/// - `use crate::module::Item` (Named)
/// - `use crate::module::*` (Glob)
/// - `use crate::module::self` (SelfImport)
/// - `use crate::module::Item as Alias` (Alias)
/// - `use crate::module::{Item1, Item2}` (multiple Named)
pub struct RustImportParser;

impl ImportParser for RustImportParser {
    fn parse_imports(content: &str, file_path: &Path) -> Vec<Import> {
        let mut imports = Vec::new();
        let file_path_str = file_path.to_string_lossy().to_string();

        for (line_num, line) in content.lines().enumerate() {
            let line_num = (line_num + 1) as u32; // 1-based line numbers

            // Skip comment lines
            if line.trim().starts_with("//") {
                continue;
            }

            // Look for use statements
            let parsed = parse_rust_use_line(line, &file_path_str, line_num);
            imports.extend(parsed);
        }

        imports
    }
}

/// Parse a single Rust `use` line
///
/// Handles various use statement formats:
/// - Simple: `use std::collections::HashMap;`
/// - Glob: `use std::collections::*;`
/// - Self: `use std::collections::self;`
/// - Alias: `use std::collections::HashMap as Map;`
/// - Multiple: `use std::collections::{HashMap, HashSet};`
fn parse_rust_use_line(line: &str, file_path: &str, line_num: u32) -> Vec<Import> {
    let trimmed = line.trim();

    // Must start with "use "
    if !trimmed.starts_with("use ") {
        return Vec::new();
    }

    // Remove "use " prefix and trailing semicolon
    let use_content = trimmed
        .strip_prefix("use ")
        .unwrap_or("")
        .trim()
        .trim_end_matches(';');

    if use_content.is_empty() {
        return Vec::new();
    }

    // Check for grouped imports: `use path::{Item1, Item2}`
    if use_content.ends_with('}') {
        return parse_rust_grouped_imports(use_content, file_path, line_num);
    }

    // Check for alias: `use path::Item as Alias`
    if let Some(import) = parse_rust_alias_import(use_content, file_path, line_num) {
        return vec![import];
    }

    // Check for glob: `use path::*`
    if use_content.ends_with("::*") {
        let source = use_content.trim_end_matches("::*");
        return vec![Import::new(
            source.to_string(),
            file_path.to_string(),
            line_num,
            ImportKind::Glob,
        )];
    }

    // Check for self: `use path::self`
    if use_content.ends_with("::self") {
        let source = use_content.trim_end_matches("::self");
        return vec![Import::new(
            source.to_string(),
            file_path.to_string(),
            line_num,
            ImportKind::SelfImport,
        )];
    }

    // Simple named import
    vec![Import::new(
        use_content.to_string(),
        file_path.to_string(),
        line_num,
        ImportKind::Named,
    )]
}

/// Parse grouped imports: `use path::{Item1, Item2, Item3 as Alias}`
fn parse_rust_grouped_imports(use_content: &str, file_path: &str, line_num: u32) -> Vec<Import> {
    let mut imports = Vec::new();

    // Find the base path and the group content
    // Format: `path::{items}`
    let brace_pos = use_content.find('{');
    if brace_pos.is_none() {
        return imports;
    }

    let brace_pos = brace_pos.unwrap();
    let base_path = use_content[..brace_pos].trim();

    // Check if base path ends with ::, remove it
    let base_path = base_path.strip_suffix("::").unwrap_or(base_path);

    // Extract the group content
    let group_start = brace_pos + 1;
    let group_end = use_content.len() - 1; // Remove trailing }
    if group_start >= group_end {
        return imports;
    }

    let group_content = &use_content[group_start..group_end];

    // Split by comma and parse each item
    for item in group_content.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }

        // Check for alias in item
        if let Some(alias_pos) = item.find(" as ") {
            let item_name = item[..alias_pos].trim();
            let alias_name = item[alias_pos + 4..].trim();

            // Check for glob in item
            if item_name == "*" {
                imports.push(Import::new(
                    base_path.to_string(),
                    file_path.to_string(),
                    line_num,
                    ImportKind::Glob,
                ));
            } else if item_name == "self" {
                imports.push(
                    Import::new(
                        base_path.to_string(),
                        file_path.to_string(),
                        line_num,
                        ImportKind::SelfImport,
                    )
                    .with_alias(alias_name.to_string()),
                );
            } else {
                let full_path = if base_path.is_empty() {
                    item_name.to_string()
                } else {
                    format!("{}::{}", base_path, item_name)
                };
                imports.push(
                    Import::new(
                        full_path,
                        file_path.to_string(),
                        line_num,
                        ImportKind::Alias,
                    )
                    .with_alias(alias_name.to_string()),
                );
            }
        } else {
            // No alias
            // Check for glob
            if item == "*" {
                imports.push(Import::new(
                    base_path.to_string(),
                    file_path.to_string(),
                    line_num,
                    ImportKind::Glob,
                ));
            } else if item == "self" {
                imports.push(Import::new(
                    base_path.to_string(),
                    file_path.to_string(),
                    line_num,
                    ImportKind::SelfImport,
                ));
            } else {
                let full_path = if base_path.is_empty() {
                    item.to_string()
                } else {
                    format!("{}::{}", base_path, item)
                };
                imports.push(Import::new(
                    full_path,
                    file_path.to_string(),
                    line_num,
                    ImportKind::Named,
                ));
            }
        }
    }

    imports
}

/// Parse alias import: `use path::Item as Alias`
fn parse_rust_alias_import(use_content: &str, file_path: &str, line_num: u32) -> Option<Import> {
    if let Some(alias_pos) = use_content.find(" as ") {
        let source = use_content[..alias_pos].trim();
        let alias = use_content[alias_pos + 4..].trim();

        if !source.is_empty() && !alias.is_empty() {
            return Some(
                Import::new(
                    source.to_string(),
                    file_path.to_string(),
                    line_num,
                    ImportKind::Alias,
                )
                .with_alias(alias.to_string()),
            );
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_file_path() -> PathBuf {
        PathBuf::from("src/test.rs")
    }

    #[test]
    fn test_import_kind_enum() {
        assert_ne!(ImportKind::Named, ImportKind::Glob);
        assert_ne!(ImportKind::Named, ImportKind::SelfImport);
        assert_ne!(ImportKind::Named, ImportKind::Alias);
        assert!(matches!(ImportKind::Named, ImportKind::Named));
    }

    #[test]
    fn test_import_creation() {
        let import = Import::new(
            "std::collections::HashMap".to_string(),
            "src/test.rs".to_string(),
            1,
            ImportKind::Named,
        );

        assert_eq!(import.source, "std::collections::HashMap");
        assert_eq!(import.file_path, "src/test.rs");
        assert_eq!(import.line, 1);
        assert_eq!(import.kind, ImportKind::Named);
        assert!(import.alias.is_none());
        assert!(!import.is_glob());
        assert!(!import.has_alias());
    }

    #[test]
    fn test_import_with_alias() {
        let import = Import::new(
            "std::collections::HashMap".to_string(),
            "src/test.rs".to_string(),
            1,
            ImportKind::Alias,
        )
        .with_alias("Map".to_string());

        assert_eq!(import.alias, Some("Map".to_string()));
        assert!(import.has_alias());
        assert_eq!(import.effective_name(), Some("Map"));
    }

    #[test]
    fn test_import_effective_name() {
        let import = Import::new(
            "std::collections::HashMap".to_string(),
            "src/test.rs".to_string(),
            1,
            ImportKind::Named,
        );
        assert_eq!(import.effective_name(), Some("HashMap"));

        let import_with_alias = Import::new(
            "std::collections::HashMap".to_string(),
            "src/test.rs".to_string(),
            1,
            ImportKind::Alias,
        )
        .with_alias("Map".to_string());
        assert_eq!(import_with_alias.effective_name(), Some("Map"));
    }

    #[test]
    fn test_parse_simple_named_import() {
        let content = "use std::collections::HashMap;";
        let imports = RustImportParser::parse_imports(content, &test_file_path());

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "std::collections::HashMap");
        assert_eq!(imports[0].kind, ImportKind::Named);
    }

    #[test]
    fn test_parse_glob_import() {
        let content = "use std::collections::*;";
        let imports = RustImportParser::parse_imports(content, &test_file_path());

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "std::collections");
        assert_eq!(imports[0].kind, ImportKind::Glob);
        assert!(imports[0].is_glob());
    }

    #[test]
    fn test_parse_self_import() {
        let content = "use std::collections::self;";
        let imports = RustImportParser::parse_imports(content, &test_file_path());

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "std::collections");
        assert_eq!(imports[0].kind, ImportKind::SelfImport);
    }

    #[test]
    fn test_parse_alias_import() {
        let content = "use std::collections::HashMap as Map;";
        let imports = RustImportParser::parse_imports(content, &test_file_path());

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "std::collections::HashMap");
        assert_eq!(imports[0].kind, ImportKind::Alias);
        assert_eq!(imports[0].alias, Some("Map".to_string()));
    }

    #[test]
    fn test_parse_grouped_imports() {
        let content = "use std::collections::{HashMap, HashSet};";
        let imports = RustImportParser::parse_imports(content, &test_file_path());

        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].source, "std::collections::HashMap");
        assert_eq!(imports[0].kind, ImportKind::Named);
        assert_eq!(imports[1].source, "std::collections::HashSet");
        assert_eq!(imports[1].kind, ImportKind::Named);
    }

    #[test]
    fn test_parse_grouped_imports_with_alias() {
        let content = "use std::collections::{HashMap as Map, HashSet};";
        let imports = RustImportParser::parse_imports(content, &test_file_path());

        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].source, "std::collections::HashMap");
        assert_eq!(imports[0].kind, ImportKind::Alias);
        assert_eq!(imports[0].alias, Some("Map".to_string()));
        assert_eq!(imports[1].source, "std::collections::HashSet");
        assert_eq!(imports[1].alias, None);
    }

    #[test]
    fn test_parse_grouped_self_import_with_alias() {
        let content = "use crate::models::{self as model_types, User};";
        let imports = RustImportParser::parse_imports(content, &test_file_path());

        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].source, "crate::models");
        assert_eq!(imports[0].kind, ImportKind::SelfImport);
        assert_eq!(imports[0].alias, Some("model_types".to_string()));
        assert_eq!(imports[0].effective_name(), Some("model_types"));
        assert_eq!(imports[1].source, "crate::models::User");
        assert_eq!(imports[1].kind, ImportKind::Named);
    }

    #[test]
    fn test_parse_grouped_imports_with_glob() {
        let content = "use std::collections::{*};";
        let imports = RustImportParser::parse_imports(content, &test_file_path());

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "std::collections");
        assert_eq!(imports[0].kind, ImportKind::Glob);
    }

    #[test]
    fn test_parse_multiple_lines() {
        let content = r#"
use std::collections::HashMap;
use std::io::Read;
use serde::Deserialize;
"#;
        let imports = RustImportParser::parse_imports(content, &test_file_path());

        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0].source, "std::collections::HashMap");
        assert_eq!(imports[1].source, "std::io::Read");
        assert_eq!(imports[2].source, "serde::Deserialize");
    }

    #[test]
    fn test_skip_comments() {
        let content = r#"
// This is a comment
use std::collections::HashMap;
// Another comment
use std::io::Read;
"#;
        let imports = RustImportParser::parse_imports(content, &test_file_path());

        assert_eq!(imports.len(), 2);
    }

    #[test]
    fn test_parse_crate_import() {
        let content = "use crate::models::User;";
        let imports = RustImportParser::parse_imports(content, &test_file_path());

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "crate::models::User");
    }

    #[test]
    fn test_parse_super_import() {
        let content = "use super::utils;";
        let imports = RustImportParser::parse_imports(content, &test_file_path());

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "super::utils");
    }
}
