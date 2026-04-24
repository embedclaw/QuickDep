//! Swift parser using tree-sitter-swift.

use std::path::Path;

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource, Visibility};
use crate::parser::{make_qualified_name, node_text, ParseResult, Parser};
use crate::resolver::{Import, ImportKind};

/// Swift parser using tree-sitter-swift.
pub struct SwiftParser {
    parser: tree_sitter::Parser,
}

impl SwiftParser {
    /// Create a new Swift parser.
    ///
    /// # Panics
    /// Panics if the bundled tree-sitter grammar cannot be loaded.
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_swift::LANGUAGE.into())
            .expect("Failed to set Swift language for tree-sitter parser");
        Self { parser }
    }

    fn extract_symbols(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
        enclosing_symbol: Option<&str>,
    ) {
        if node.is_error() || node.is_missing() {
            result.error_count += 1;
            return;
        }

        match node.kind() {
            "class_declaration" => {
                if let Some((body_parent_name, enclosing_name)) =
                    self.extract_class_like(node, source, file_path, result, parent_name)
                {
                    self.extract_inheritance(node, source, file_path, result, &enclosing_name);
                    if let Some(body) = self.body_node(node) {
                        self.extract_symbols(
                            &body,
                            source,
                            file_path,
                            result,
                            Some(&body_parent_name),
                            Some(&enclosing_name),
                        );
                    }
                    return;
                }
            }
            "protocol_declaration" => {
                if let Some((nested_name, enclosing_name)) =
                    self.extract_protocol(node, source, file_path, result, parent_name)
                {
                    if let Some(body) = self.body_node(node) {
                        self.extract_symbols(
                            &body,
                            source,
                            file_path,
                            result,
                            Some(&nested_name),
                            Some(&enclosing_name),
                        );
                    }
                    return;
                }
            }
            "function_declaration" | "protocol_function_declaration" => {
                if let Some(qualified_name) =
                    self.extract_function(node, source, file_path, result, parent_name)
                {
                    if let Some(body) = self.body_node(node) {
                        self.extract_symbols(
                            &body,
                            source,
                            file_path,
                            result,
                            parent_name,
                            Some(&qualified_name),
                        );
                    }
                    return;
                }
            }
            "import_declaration" => {
                self.extract_import(node, source, file_path, result);
            }
            "call_expression" => {
                self.extract_call(node, source, file_path, result, enclosing_symbol);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_symbols(
                &child,
                source,
                file_path,
                result,
                parent_name,
                enclosing_symbol,
            );
        }
    }

    fn extract_class_like(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
    ) -> Option<(String, String)> {
        let declaration_kind = self.declaration_keyword(node)?;
        let name_node = self
            .find_named_child(node, &["type_identifier", "user_type"])
            .or_else(|| node.child_by_field_name("name"))?;
        let raw_name = self.type_name_text(&name_node, source)?;
        let name = self.last_type_segment(&raw_name);
        let nested_name = match parent_name {
            Some(parent) => format!("{parent}::{name}"),
            None => name.clone(),
        };
        let body_parent_name = if declaration_kind == "extension" {
            format!("{nested_name}::extension")
        } else {
            nested_name.clone()
        };
        let qualified_name = if declaration_kind == "extension" {
            self.extension_qualified_name(file_path, &name, parent_name)
        } else {
            make_qualified_name(file_path, &name, parent_name)
        };
        let (line, column) = self.node_start(node);

        let mut symbol = Symbol::new(
            name,
            qualified_name.clone(),
            self.symbol_kind_for_class_like(declaration_kind)?,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(self.visibility_for(node, source))
        .with_source(SymbolSource::Local);

        if let Some(signature) = self.type_signature(node, source) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some((body_parent_name, qualified_name))
    }

    fn extract_protocol(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
    ) -> Option<(String, String)> {
        let name_node = self
            .find_named_child(node, &["type_identifier"])
            .or_else(|| node.child_by_field_name("name"))?;
        let name = self.type_name_text(&name_node, source)?;
        let nested_name = match parent_name {
            Some(parent) => format!("{parent}::{name}"),
            None => name.clone(),
        };
        let qualified_name = make_qualified_name(file_path, &name, parent_name);
        let (line, column) = self.node_start(node);

        let mut symbol = Symbol::new(
            name,
            qualified_name.clone(),
            SymbolKind::Interface,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(self.visibility_for(node, source))
        .with_source(SymbolSource::Local);

        if let Some(signature) = self.type_signature(node, source) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some((nested_name, qualified_name))
    }

    fn extract_function(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
    ) -> Option<String> {
        let name_node = self
            .find_named_child(node, &["simple_identifier"])
            .or_else(|| node.child_by_field_name("name"))?;
        let name = node_text(&name_node, source)?;
        let qualified_name = make_qualified_name(file_path, &name, parent_name);
        let (line, column) = self.node_start(node);
        let kind = if parent_name.is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        let mut symbol = Symbol::new(
            name.clone(),
            qualified_name.clone(),
            kind,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(self.visibility_for(node, source))
        .with_source(SymbolSource::Local);

        if let Some(signature) = self.function_signature(node, source) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some(qualified_name)
    }

    fn extract_import(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let Some(text) = node_text(node, source) else {
            return;
        };
        let Some(source_name) = text
            .strip_prefix("import")
            .map(str::trim)
            .and_then(|rest| rest.split_whitespace().last())
            .filter(|name| !name.is_empty())
        else {
            return;
        };

        result.imports.push(Import::new(
            source_name.to_string(),
            file_path.to_string(),
            self.node_start(node).0,
            ImportKind::Glob,
        ));
    }

    fn extract_inheritance(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        from_symbol: &str,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "inheritance_specifier" {
                continue;
            }

            let mut inheritance_cursor = child.walk();
            for target in child.named_children(&mut inheritance_cursor) {
                if target.kind() != "user_type" {
                    continue;
                }
                let Some(name) = self.type_name_text(&target, source) else {
                    continue;
                };
                result.dependencies.push(Dependency::new(
                    from_symbol.to_string(),
                    name,
                    file_path.to_string(),
                    self.node_start(node).0,
                    DependencyKind::Inherit,
                ));
            }
        }
    }

    fn extract_call(
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
        let Some(call_suffix) = self.find_named_child(node, &["call_suffix"]) else {
            return;
        };
        let Some(suffix_text) = node_text(&call_suffix, source) else {
            return;
        };
        if !suffix_text.starts_with('(') {
            return;
        }

        let Some(callee) = self.call_callee(node) else {
            return;
        };
        let Some(callee_text) = node_text(&callee, source) else {
            return;
        };

        let target_name = match callee.kind() {
            "simple_identifier" => callee_text.clone(),
            "navigation_expression" => {
                let Some(target_name) = self.navigation_target_name(&callee, source) else {
                    return;
                };
                target_name
            }
            _ => return,
        };

        let is_type_use = self.is_type_like_call(&callee, &callee_text, &target_name);
        let dependency_target = if is_type_use {
            callee_text
        } else {
            target_name
        };

        result.dependencies.push(Dependency::new(
            from_symbol.to_string(),
            dependency_target,
            file_path.to_string(),
            self.node_start(node).0,
            if is_type_use {
                DependencyKind::TypeUse
            } else {
                DependencyKind::Call
            },
        ));
    }

    fn declaration_keyword(&self, node: &tree_sitter::Node<'_>) -> Option<&'static str> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "class" => return Some("class"),
                "struct" => return Some("struct"),
                "enum" => return Some("enum"),
                "actor" => return Some("actor"),
                "extension" => return Some("extension"),
                _ => {}
            }
        }
        None
    }

    fn symbol_kind_for_class_like(&self, declaration_kind: &str) -> Option<SymbolKind> {
        match declaration_kind {
            "class" | "actor" | "extension" => Some(SymbolKind::Class),
            "struct" => Some(SymbolKind::Struct),
            "enum" => Some(SymbolKind::Enum),
            _ => None,
        }
    }

    fn type_name_text(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "type_identifier" | "user_type" | "simple_identifier" => node_text(node, source),
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if let Some(name) = self.type_name_text(&child, source) {
                        return Some(name);
                    }
                }
                None
            }
        }
    }

    fn last_type_segment(&self, type_name: &str) -> String {
        type_name
            .rsplit('.')
            .find(|segment| !segment.is_empty())
            .unwrap_or(type_name)
            .to_string()
    }

    fn extension_qualified_name(
        &self,
        file_path: &str,
        name: &str,
        parent_name: Option<&str>,
    ) -> String {
        match parent_name {
            Some(parent) => format!("{file_path}::{parent}::{name}::extension"),
            None => format!("{file_path}::{name}::extension"),
        }
    }

    fn visibility_for(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Visibility {
        let Some(modifiers) = self.find_named_child(node, &["modifiers"]) else {
            return Visibility::Public;
        };
        let Some(text) = node_text(&modifiers, source) else {
            return Visibility::Public;
        };

        if text.contains("private") || text.contains("fileprivate") {
            Visibility::Private
        } else if text.contains("protected") {
            Visibility::Protected
        } else {
            Visibility::Public
        }
    }

    fn function_signature(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        node_text(node, source)
            .and_then(|text| text.lines().next().map(str::trim).map(ToString::to_string))
    }

    fn type_signature(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        node_text(node, source)
            .and_then(|text| text.lines().next().map(str::trim).map(ToString::to_string))
    }

    fn call_callee<'a>(&self, node: &tree_sitter::Node<'a>) -> Option<tree_sitter::Node<'a>> {
        let mut cursor = node.walk();
        let callee = node
            .named_children(&mut cursor)
            .find(|child| child.kind() != "call_suffix");
        callee
    }

    fn navigation_target_name(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
    ) -> Option<String> {
        self.rightmost_simple_identifier(node, source)
    }

    fn is_type_like_call(
        &self,
        callee: &tree_sitter::Node<'_>,
        callee_text: &str,
        target_name: &str,
    ) -> bool {
        let starts_with_uppercase = target_name
            .chars()
            .next()
            .map(char::is_uppercase)
            .unwrap_or(false);
        if !starts_with_uppercase {
            return false;
        }

        callee.kind() == "simple_identifier" || callee_text.contains('.')
    }

    fn body_node<'a>(&self, node: &tree_sitter::Node<'a>) -> Option<tree_sitter::Node<'a>> {
        self.find_named_child(
            node,
            &[
                "class_body",
                "enum_class_body",
                "protocol_body",
                "function_body",
            ],
        )
    }

    fn find_named_child<'a>(
        &self,
        node: &tree_sitter::Node<'a>,
        kinds: &[&str],
    ) -> Option<tree_sitter::Node<'a>> {
        let mut cursor = node.walk();
        let child = node
            .named_children(&mut cursor)
            .find(|child| kinds.contains(&child.kind()));
        child
    }

    fn rightmost_simple_identifier(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
    ) -> Option<String> {
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();

        for child in children.iter().rev() {
            if child.kind() == "simple_identifier" {
                return node_text(child, source);
            }
            if let Some(identifier) = self.rightmost_simple_identifier(child, source) {
                return Some(identifier);
            }
        }

        None
    }

    fn node_start(&self, node: &tree_sitter::Node<'_>) -> (u32, u32) {
        let position = node.start_position();
        (position.row as u32 + 1, position.column as u32 + 1)
    }
}

impl Default for SwiftParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for SwiftParser {
    fn language(&self) -> &'static str {
        "swift"
    }

    fn parse_file(&mut self, _path: &Path, content: &[u8], file_path: &str) -> ParseResult {
        let Some(tree) = self.parser.parse(content, None) else {
            return ParseResult {
                error_count: 1,
                ..ParseResult::default()
            };
        };

        let mut result = ParseResult::default();
        self.extract_symbols(
            &tree.root_node(),
            content,
            file_path,
            &mut result,
            None,
            None,
        );

        if tree.root_node().has_error() && result.error_count == 0 {
            result.error_count = 1;
        }

        result
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["swift"]
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_parse_swift_fixture() {
        let mut parser = SwiftParser::new();
        let fixture_path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/swift/sample.swift"
        ));
        let content = std::fs::read(&fixture_path).expect("Failed to read fixture");
        let result = parser.parse_file(&fixture_path, &content, "Sources/App/sample.swift");

        assert_eq!(result.error_count, 0);
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Interface && symbol.name == "UserRepository"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Struct && symbol.name == "User"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Class && symbol.name == "InMemoryRepo"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Enum && symbol.name == "Direction"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Method && symbol.name == "clear"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.qualified_name
                == "Sources/App/sample.swift::InMemoryRepo::extension"));
        assert!(result.symbols.iter().any(|symbol| symbol.qualified_name
            == "Sources/App/sample.swift::InMemoryRepo::extension::clear"));
        assert_eq!(result.imports.len(), 1);
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Inherit
                && dependency.to_symbol == "UserRepository"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Inherit
                && dependency.to_symbol == "CustomStringConvertible"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::TypeUse
                && dependency.to_symbol == "User"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "save"));
    }

    #[test]
    fn test_parse_swift_namespaced_type_use_and_subscript() {
        let mut parser = SwiftParser::new();
        let content = br#"
import Foundation

func makeDate(store: CacheStore) {
    let date = Foundation.Date()
    let value = store[date]
    print(value as Any)
}
"#;

        let result = parser.parse_file(&PathBuf::from("main.swift"), content, "main.swift");

        assert_eq!(result.error_count, 0);
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::TypeUse
                && dependency.to_symbol == "Foundation.Date"));
        assert!(!result
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "store"));
    }
}
