//! C parser using tree-sitter-c.

use std::path::Path;

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource, Visibility};
use crate::parser::{make_qualified_name, node_text, ParseResult, Parser};
use crate::resolver::{Import, ImportKind};

/// C parser using tree-sitter-c.
pub struct CParser {
    parser: tree_sitter::Parser,
}

impl CParser {
    /// Create a new C parser.
    ///
    /// # Panics
    /// Panics if the bundled tree-sitter grammar cannot be loaded.
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_c::LANGUAGE.into())
            .expect("Failed to set C language for tree-sitter parser");
        Self { parser }
    }

    fn extract_symbols(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        enclosing_symbol: Option<&str>,
    ) {
        if node.is_error() || node.is_missing() {
            result.error_count += 1;
            return;
        }

        match node.kind() {
            "function_definition" => {
                if let Some(qualified_name) =
                    self.extract_function_definition(node, source, file_path, result)
                {
                    if let Some(body) = node.child_by_field_name("body") {
                        self.extract_symbols(
                            &body,
                            source,
                            file_path,
                            result,
                            Some(&qualified_name),
                        );
                    }
                    return;
                }
            }
            "struct_specifier" => {
                if self.is_top_level_type_specifier(node) {
                    self.extract_struct(node, source, file_path, result);
                }
            }
            "enum_specifier" => {
                if self.is_top_level_type_specifier(node) {
                    self.extract_enum(node, source, file_path, result);
                }
            }
            "type_definition" => {
                self.extract_type_alias(node, source, file_path, result);
            }
            "declaration" => {
                if self.is_top_level_declaration(node) {
                    self.extract_global_declarations(node, source, file_path, result);
                }
            }
            "preproc_include" => {
                self.extract_include(node, source, file_path, result);
            }
            "call_expression" => {
                self.extract_call_expression(node, source, file_path, result, enclosing_symbol);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_symbols(&child, source, file_path, result, enclosing_symbol);
        }
    }

    fn extract_function_definition(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) -> Option<String> {
        let declarator = node.child_by_field_name("declarator")?;
        let name = self.declarator_identifier(&declarator, source)?;
        let qualified_name = make_qualified_name(file_path, &name, None);
        let (line, column) = self.node_start(node);

        let mut symbol = Symbol::new(
            name.clone(),
            qualified_name.clone(),
            SymbolKind::Function,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(self.visibility_for_node(node, source))
        .with_source(SymbolSource::Local);

        if let Some(signature) = self.function_signature(node, source) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some(qualified_name)
    }

    fn extract_struct(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Some(name) = node_text(&name_node, source) else {
            return;
        };

        let (line, column) = self.node_start(node);
        result.symbols.push(
            Symbol::new(
                name.clone(),
                make_qualified_name(file_path, &name, None),
                SymbolKind::Struct,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(Visibility::Public)
            .with_source(SymbolSource::Local),
        );
    }

    fn extract_enum(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Some(name) = node_text(&name_node, source) else {
            return;
        };

        let (line, column) = self.node_start(node);
        result.symbols.push(
            Symbol::new(
                name.clone(),
                make_qualified_name(file_path, &name, None),
                SymbolKind::Enum,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(Visibility::Public)
            .with_source(SymbolSource::Local),
        );

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "enumerator_list" {
                continue;
            }

            let mut enum_cursor = child.walk();
            for enumerator in child.children(&mut enum_cursor) {
                if enumerator.kind() != "enumerator" {
                    continue;
                }

                let Some(variant_name) =
                    self.first_named_text(&enumerator, source, &["identifier"])
                else {
                    continue;
                };
                let (line, column) = self.node_start(&enumerator);
                result.symbols.push(
                    Symbol::new(
                        variant_name.clone(),
                        make_qualified_name(file_path, &variant_name, Some(&name)),
                        SymbolKind::EnumVariant,
                        file_path.to_string(),
                        line,
                        column,
                    )
                    .with_visibility(Visibility::Public)
                    .with_source(SymbolSource::Local),
                );
            }
        }
    }

    fn extract_type_alias(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let Some(declarator) = node.child_by_field_name("declarator") else {
            return;
        };
        let Some(name) = self.declarator_identifier(&declarator, source) else {
            return;
        };
        let (line, column) = self.node_start(node);

        let mut symbol = Symbol::new(
            name.clone(),
            make_qualified_name(file_path, &name, None),
            SymbolKind::TypeAlias,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(Visibility::Public)
        .with_source(SymbolSource::Local);

        if let Some(signature) = node_text(node, source) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
    }

    fn extract_global_declarations(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let declaration_text = node_text(node, source).unwrap_or_default();
        if self.is_objective_c_declaration(&declaration_text)
            || self.contains_kind(node, "ERROR")
            || !self.has_c_declaration_specifier(node)
        {
            return;
        }
        let kind = if declaration_text.contains("const") {
            SymbolKind::Constant
        } else {
            SymbolKind::Variable
        };
        let visibility = self.visibility_for_text(&declaration_text);

        if let Some(first_identifier) = self.first_named_text(node, source, &["identifier"]) {
            if first_identifier == "typedef" {
                return;
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "init_declarator" && child.kind() != "identifier" {
                continue;
            }

            let Some(name) = self.declarator_identifier(&child, source) else {
                continue;
            };

            if self.contains_kind(&child, "function_declarator") {
                continue;
            }

            let (line, column) = self.node_start(&child);
            let mut symbol = Symbol::new(
                name.clone(),
                make_qualified_name(file_path, &name, None),
                kind.clone(),
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(visibility.clone())
            .with_source(SymbolSource::Local);

            if child.kind() == "init_declarator" {
                if let Some(signature) = node_text(&child, source) {
                    symbol = symbol.with_signature(signature);
                }
            }

            result.symbols.push(symbol);
        }
    }

    fn extract_include(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let Some(raw) = node_text(&child, source) else {
                continue;
            };
            let include = raw
                .trim()
                .trim_start_matches('"')
                .trim_end_matches('"')
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string();

            if include.is_empty() || include == "#include" {
                continue;
            }

            let (line, _) = self.node_start(node);
            result.imports.push(Import::new(
                include,
                file_path.to_string(),
                line,
                ImportKind::Named,
            ));
            return;
        }
    }

    fn extract_call_expression(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        enclosing_symbol: Option<&str>,
    ) {
        let Some(from_symbol) = enclosing_symbol else {
            return;
        };
        let function_node = node
            .child_by_field_name("function")
            .or_else(|| node.named_child(0));
        let Some(function_node) = function_node else {
            return;
        };
        let Some(target) = self.call_target_name(&function_node, source) else {
            return;
        };
        let (line, _) = self.node_start(node);

        result.dependencies.push(Dependency::new(
            from_symbol.to_string(),
            target,
            file_path.to_string(),
            line,
            DependencyKind::Call,
        ));
    }

    fn function_signature(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        let declarator = node.child_by_field_name("declarator")?;
        node_text(&declarator, source)
    }

    fn call_target_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "identifier" | "field_identifier" => node_text(node, source),
            "field_expression" => node
                .child_by_field_name("field")
                .and_then(|field| node_text(&field, source))
                .or_else(|| self.first_named_text(node, source, &["field_identifier"])),
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if let Some(name) = self.call_target_name(&child, source) {
                        return Some(name);
                    }
                }
                None
            }
        }
    }

    fn declarator_identifier(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "identifier" | "field_identifier" | "type_identifier" => node_text(node, source),
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if let Some(name) = self.declarator_identifier(&child, source) {
                        return Some(name);
                    }
                }
                None
            }
        }
    }

    fn first_named_text(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        kinds: &[&str],
    ) -> Option<String> {
        if kinds.contains(&node.kind()) {
            return node_text(node, source);
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(text) = self.first_named_text(&child, source, kinds) {
                return Some(text);
            }
        }

        None
    }

    fn contains_kind(&self, node: &tree_sitter::Node<'_>, needle: &str) -> bool {
        if node.kind() == needle {
            return true;
        }

        let mut cursor = node.walk();
        let contains = node
            .children(&mut cursor)
            .any(|child| self.contains_kind(&child, needle));
        contains
    }

    fn is_objective_c_declaration(&self, text: &str) -> bool {
        let trimmed = text.trim_start();
        trimmed.starts_with('@') || trimmed.starts_with('-') || trimmed.starts_with('+')
    }

    fn has_c_declaration_specifier(&self, node: &tree_sitter::Node<'_>) -> bool {
        const DECLARATION_SPECIFIERS: &[&str] = &[
            "primitive_type",
            "sized_type_specifier",
            "type_identifier",
            "struct_specifier",
            "union_specifier",
            "enum_specifier",
        ];

        DECLARATION_SPECIFIERS
            .iter()
            .any(|kind| self.contains_kind(node, kind))
    }

    fn is_top_level_declaration(&self, node: &tree_sitter::Node<'_>) -> bool {
        matches!(
            node.parent().map(|parent| parent.kind()),
            Some("translation_unit")
        )
    }

    fn is_top_level_type_specifier(&self, node: &tree_sitter::Node<'_>) -> bool {
        if self.has_ancestor_kind(node, "type_definition") {
            return false;
        }

        matches!(
            node.parent().map(|parent| parent.kind()),
            Some("declaration" | "translation_unit")
        )
    }

    fn has_ancestor_kind(&self, node: &tree_sitter::Node<'_>, needle: &str) -> bool {
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == needle {
                return true;
            }
            current = parent.parent();
        }
        false
    }

    fn visibility_for_node(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Visibility {
        let text = node_text(node, source).unwrap_or_default();
        self.visibility_for_text(&text)
    }

    fn visibility_for_text(&self, text: &str) -> Visibility {
        if text.contains("static") {
            Visibility::Private
        } else {
            Visibility::Public
        }
    }

    fn node_start(&self, node: &tree_sitter::Node<'_>) -> (u32, u32) {
        let position = node.start_position();
        ((position.row + 1) as u32, (position.column + 1) as u32)
    }
}

impl Default for CParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for CParser {
    fn language(&self) -> &'static str {
        "c"
    }

    fn parse_file(&mut self, _path: &Path, content: &[u8], file_path: &str) -> ParseResult {
        let Some(tree) = self.parser.parse(content, None) else {
            return ParseResult::default();
        };
        let mut result = ParseResult::default();
        self.extract_symbols(&tree.root_node(), content, file_path, &mut result, None);
        if tree.root_node().has_error() && result.error_count == 0 {
            result.error_count = 1;
        }
        result
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["c", "h"]
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_parse_c_symbols_and_calls() {
        let content = br#"
#include "utils.h"
#include <stdio.h>

typedef unsigned long size_type;

struct user {
    int age;
};

enum status {
    STATUS_OK,
    STATUS_ERR,
};

static int helper(void) { return 1; }

const int VERSION = 1;
int global_counter = 0;

int run(void) {
    printf("hi");
    return helper();
}
"#;
        let mut parser = CParser::new();
        let result = parser.parse_file(&PathBuf::from("sample.c"), content, "src/sample.c");

        assert_eq!(result.error_count, 0);
        assert!(result.symbols.iter().any(|symbol| symbol.name == "run"));
        assert!(result.symbols.iter().any(|symbol| symbol.name == "helper"));
        assert!(result.symbols.iter().any(|symbol| symbol.name == "user"));
        assert!(result.symbols.iter().any(|symbol| symbol.name == "status"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.name == "STATUS_OK"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.name == "size_type"));
        assert!(result.symbols.iter().any(|symbol| symbol.name == "VERSION"));
        assert_eq!(result.imports.len(), 2);
        assert!(result
            .imports
            .iter()
            .any(|import| import.source == "utils.h"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.from_symbol == "src/sample.c::run"
                && dependency.to_symbol == "helper"));
    }

    #[test]
    fn test_static_functions_are_private() {
        let content = b"static int helper(void) { return 1; }\n";
        let mut parser = CParser::new();
        let result = parser.parse_file(&PathBuf::from("sample.c"), content, "src/sample.c");

        let helper = result
            .symbols
            .iter()
            .find(|symbol| symbol.name == "helper")
            .expect("helper symbol");
        assert_eq!(helper.visibility, Visibility::Private);
    }

    #[test]
    fn test_skips_objective_c_header_fragments() {
        let content = br#"#import <Foundation/Foundation.h>

@interface Logger : NSObject
- (void)logMessage:(NSString *)message;
@end
        "#;
        let mut parser = CParser::new();
        let result = parser.parse_file(&PathBuf::from("Logger.h"), content, "src/Logger.h");

        assert!(!result.symbols.iter().any(|symbol| symbol.name == "end"));
    }
}
