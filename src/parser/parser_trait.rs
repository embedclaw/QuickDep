//! Parser trait and result types.
//!
//! Defines the interface for language-specific parsers that extract
//! symbols and dependencies from source code.

use crate::core::{Dependency, Symbol};
use crate::resolver::Import;
use std::path::Path;

/// Result of parsing a source file.
#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    /// Extracted symbols (functions, structs, enums, etc.)
    pub symbols: Vec<Symbol>,

    /// Extracted dependencies (calls, references)
    pub dependencies: Vec<Dependency>,

    /// Extracted imports (use statements, import declarations)
    pub imports: Vec<Import>,

    /// Number of error nodes encountered during parsing
    pub error_count: usize,
}

/// Parser trait for extracting symbols and dependencies from source code.
///
/// Each language (Rust, TypeScript, Python, Go) implements this trait
/// using Tree-sitter to parse the source code.
pub trait Parser {
    /// Returns the language this parser handles.
    fn language(&self) -> &'static str;

    /// Parse a file and extract symbols, dependencies, and imports.
    ///
    /// # Arguments
    /// * `path` - Path to the source file
    /// * `content` - File content as bytes
    /// * `file_path` - Relative file path for qualified names
    ///
    /// # Returns
    /// ParseResult containing extracted symbols, dependencies, imports,
    /// and the count of error nodes encountered.
    fn parse_file(&mut self, path: &Path, content: &[u8], file_path: &str) -> ParseResult;

    /// Get supported file extensions for this parser.
    fn extensions(&self) -> &'static [&'static str];
}

/// Helper function to generate a qualified name.
///
/// Format: `file_path::SymbolName` (relative path format)
/// For methods inside a struct/trait: `file_path::StructName::MethodName`
pub fn make_qualified_name(
    file_path: &str,
    symbol_name: &str,
    parent_name: Option<&str>,
) -> String {
    match parent_name {
        Some(parent) => format!("{}::{}::{}", file_path, parent, symbol_name),
        None => format!("{}::{}", file_path, symbol_name),
    }
}

/// Helper function to extract text from a tree-sitter node.
pub fn node_text(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    node.utf8_text(source).ok().map(|s| s.to_string())
}
