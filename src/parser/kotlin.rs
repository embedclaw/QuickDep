//! Kotlin parser using tree-sitter-kotlin-ng.

use std::path::Path;

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource, Visibility};
use crate::parser::{make_qualified_name, node_text, ParseResult, Parser};
use crate::resolver::{Import, ImportKind};

/// Kotlin parser using tree-sitter-kotlin-ng.
pub struct KotlinParser {
    parser: tree_sitter::Parser,
}

impl KotlinParser {
    /// Create a new Kotlin parser.
    ///
    /// # Panics
    /// Panics if the bundled tree-sitter grammar cannot be loaded.
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_kotlin_ng::LANGUAGE.into())
            .expect("Failed to set Kotlin language for tree-sitter parser");
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
                let kind = self.class_symbol_kind(node, source);
                if let Some((nested_name, qualified_name)) = self.extract_type_declaration(
                    node,
                    source,
                    file_path,
                    result,
                    parent_name,
                    kind.clone(),
                ) {
                    self.extract_type_relationships(
                        node,
                        source,
                        file_path,
                        result,
                        &qualified_name,
                        kind,
                    );
                    if let Some(body) = self.class_body(node) {
                        self.extract_symbols(
                            &body,
                            source,
                            file_path,
                            result,
                            Some(&nested_name),
                            enclosing_symbol,
                        );
                    }
                    return;
                }
            }
            "object_declaration" => {
                if let Some((nested_name, qualified_name)) = self.extract_type_declaration(
                    node,
                    source,
                    file_path,
                    result,
                    parent_name,
                    SymbolKind::Class,
                ) {
                    self.extract_type_relationships(
                        node,
                        source,
                        file_path,
                        result,
                        &qualified_name,
                        SymbolKind::Class,
                    );
                    if let Some(body) = self.class_body(node) {
                        self.extract_symbols(
                            &body,
                            source,
                            file_path,
                            result,
                            Some(&nested_name),
                            enclosing_symbol,
                        );
                    }
                    return;
                }
            }
            "function_declaration" => {
                if let Some(qualified_name) =
                    self.extract_function_declaration(node, source, file_path, result, parent_name)
                {
                    if let Some(body) = node
                        .children(&mut node.walk())
                        .find(|child| child.kind() == "function_body")
                    {
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
            "property_declaration" if enclosing_symbol.is_none() => {
                self.extract_property_declaration(node, source, file_path, result, parent_name);
            }
            "import" => {
                self.extract_import(node, source, file_path, result);
            }
            "call_expression" => {
                self.extract_call_expression(node, source, file_path, result, enclosing_symbol);
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

    fn extract_type_declaration(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
        kind: SymbolKind,
    ) -> Option<(String, String)> {
        let name_node = node.child_by_field_name("name")?;
        let name = clean_kotlin_identifier(&node_text(&name_node, source)?);
        let nested_name = match parent_name {
            Some(parent) => format!("{parent}::{name}"),
            None => name.clone(),
        };
        let qualified_name = make_qualified_name(file_path, &name, parent_name);
        let (line, column) = self.node_start(node);

        result.symbols.push(
            Symbol::new(
                name,
                qualified_name.clone(),
                kind,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(self.visibility_for_node(node, source))
            .with_source(SymbolSource::Local),
        );

        Some((nested_name, qualified_name))
    }

    fn extract_function_declaration(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let name = clean_kotlin_identifier(&node_text(&name_node, source)?);
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
        .with_visibility(self.visibility_for_node(node, source))
        .with_source(SymbolSource::Local);

        if let Some(signature) = self.function_signature(node, source, &name) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some(qualified_name)
    }

    fn extract_property_declaration(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
    ) {
        let (line, column) = self.node_start(node);
        let kind = self.property_symbol_kind(node, source);
        let visibility = self.visibility_for_node(node, source);
        let signature = node_text(node, source);

        for name in self.variable_declaration_names(node, source) {
            let mut symbol = Symbol::new(
                name.clone(),
                make_qualified_name(file_path, &name, parent_name),
                kind.clone(),
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(visibility.clone())
            .with_source(SymbolSource::Local);

            if let Some(signature) = &signature {
                symbol = symbol.with_signature(signature.clone());
            }

            result.symbols.push(symbol);
        }
    }

    fn extract_type_relationships(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        from_symbol: &str,
        symbol_kind: SymbolKind,
    ) {
        let Some(delegation_specifiers) = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "delegation_specifiers")
        else {
            return;
        };
        let line = self.node_start(node).0;

        let mut cursor = delegation_specifiers.walk();
        for child in delegation_specifiers.named_children(&mut cursor) {
            let Some(name) = self.primary_type_name(&child, source) else {
                continue;
            };
            let child_text = node_text(&child, source).unwrap_or_default();
            let known_interface = result
                .symbols
                .iter()
                .any(|symbol| symbol.kind == SymbolKind::Interface && symbol.name == name);
            let dependency_kind = if symbol_kind == SymbolKind::Interface {
                DependencyKind::Inherit
            } else if known_interface || looks_like_kotlin_interface(&name) {
                DependencyKind::Implement
            } else if child_text.contains('(') {
                DependencyKind::Inherit
            } else {
                DependencyKind::Implement
            };

            result.dependencies.push(Dependency::new(
                from_symbol.to_string(),
                name,
                file_path.to_string(),
                line,
                dependency_kind,
            ));
        }
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
        let Some(content) = text
            .trim()
            .trim_end_matches(';')
            .trim()
            .strip_prefix("import ")
            .map(str::trim)
        else {
            return;
        };
        if content.is_empty() {
            return;
        }

        let line = self.node_start(node).0;
        if let Some(path) = content.strip_suffix(".*") {
            result.imports.push(Import::new(
                path.trim().to_string(),
                file_path.to_string(),
                line,
                ImportKind::Glob,
            ));
            return;
        }

        if let Some((source, alias)) = content.split_once(" as ") {
            let source = source.trim();
            let alias = alias.trim();
            if !source.is_empty() && !alias.is_empty() {
                result.imports.push(
                    Import::new(
                        source.to_string(),
                        file_path.to_string(),
                        line,
                        ImportKind::Alias,
                    )
                    .with_alias(clean_kotlin_identifier(alias)),
                );
            }
            return;
        }

        result.imports.push(Import::new(
            content.to_string(),
            file_path.to_string(),
            line,
            ImportKind::Named,
        ));
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
        let Some(function_node) = node.named_child(0) else {
            return;
        };
        let Some(target) = self.call_target_name(&function_node, source) else {
            return;
        };
        let kind = if is_constructor_like_call(&function_node, &target) {
            DependencyKind::TypeUse
        } else {
            DependencyKind::Call
        };

        result.dependencies.push(Dependency::new(
            from_symbol.to_string(),
            target,
            file_path.to_string(),
            self.node_start(node).0,
            kind,
        ));
    }

    fn class_body<'a>(&self, node: &tree_sitter::Node<'a>) -> Option<tree_sitter::Node<'a>> {
        node.children(&mut node.walk())
            .find(|child| matches!(child.kind(), "class_body" | "enum_class_body"))
    }

    fn class_symbol_kind(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> SymbolKind {
        let Some(name_node) = node.child_by_field_name("name") else {
            return SymbolKind::Class;
        };
        let prefix = std::str::from_utf8(&source[node.start_byte()..name_node.start_byte()])
            .unwrap_or_default();

        if prefix.split_whitespace().any(|part| part == "interface") {
            SymbolKind::Interface
        } else if prefix.split_whitespace().any(|part| part == "enum") {
            SymbolKind::Enum
        } else {
            SymbolKind::Class
        }
    }

    fn variable_declaration_names(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
    ) -> Vec<String> {
        let mut names = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "variable_declaration" {
                if let Some(name_node) = child.named_child(0) {
                    if let Some(name) = node_text(&name_node, source) {
                        names.push(clean_kotlin_identifier(&name));
                    }
                }
                continue;
            }

            names.extend(self.variable_declaration_names(&child, source));
        }
        names
    }

    fn primary_type_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "identifier" => node_text(node, source).map(|text| clean_kotlin_identifier(&text)),
            "user_type" | "qualified_identifier" | "navigation_expression" => {
                node_text(node, source)
                    .map(|text| clean_kotlin_identifier(&self.last_path_segment(&text)))
            }
            "type" | "nullable_type" | "non_nullable_type" | "parenthesized_type" => node
                .named_child(0)
                .and_then(|child| self.primary_type_name(&child, source)),
            "constructor_invocation" => node
                .named_children(&mut node.walk())
                .find(|child| child.kind() == "type")
                .or_else(|| node.named_child(0))
                .and_then(|child| self.primary_type_name(&child, source)),
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if let Some(name) = self.primary_type_name(&child, source) {
                        return Some(name);
                    }
                }
                None
            }
        }
    }

    fn call_target_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "identifier" => node_text(node, source).map(|text| clean_kotlin_identifier(&text)),
            "user_type" | "qualified_identifier" | "navigation_expression" => {
                node_text(node, source)
                    .map(|text| clean_kotlin_identifier(&self.last_path_segment(&text)))
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if let Some(name) = self.call_target_name(&child, source) {
                        return Some(name);
                    }
                }
                None
            }
        }
    }

    fn property_symbol_kind(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> SymbolKind {
        let text = node_text(node, source).unwrap_or_default();
        if text.split_whitespace().any(|part| part == "const") {
            SymbolKind::Constant
        } else {
            SymbolKind::Property
        }
    }

    fn function_signature(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        name: &str,
    ) -> Option<String> {
        let parameters = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "function_value_parameters")
            .and_then(|parameters| node_text(&parameters, source))?;
        let mut return_type = String::new();
        for child in node.children(&mut node.walk()) {
            if child.kind() == "type" {
                return_type = node_text(&child, source).unwrap_or_default();
            }
        }
        Some(format!("{name}{parameters}{return_type}"))
    }

    fn visibility_for_node(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Visibility {
        if self.has_modifier(node, source, "private") || self.has_modifier(node, source, "internal")
        {
            Visibility::Private
        } else if self.has_modifier(node, source, "protected") {
            Visibility::Protected
        } else {
            Visibility::Public
        }
    }

    fn has_modifier(&self, node: &tree_sitter::Node<'_>, source: &[u8], modifier: &str) -> bool {
        let Some(modifiers) = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "modifiers")
        else {
            return false;
        };

        modifiers
            .children(&mut modifiers.walk())
            .any(|child| node_text(&child, source).as_deref() == Some(modifier))
    }

    fn last_path_segment(&self, path: &str) -> String {
        path.rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or(path)
            .split('<')
            .next()
            .unwrap_or(path)
            .to_string()
    }

    fn node_start(&self, node: &tree_sitter::Node<'_>) -> (u32, u32) {
        let position = node.start_position();
        (position.row as u32 + 1, position.column as u32 + 1)
    }
}

fn is_constructor_like_call(node: &tree_sitter::Node<'_>, target: &str) -> bool {
    matches!(node.kind(), "identifier" | "user_type")
        && target
            .chars()
            .next()
            .is_some_and(|first| first.is_uppercase())
}

fn looks_like_kotlin_interface(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('I')) && matches!(chars.next(), Some(next) if next.is_uppercase())
}

fn clean_kotlin_identifier(identifier: &str) -> String {
    identifier.trim_matches('`').to_string()
}

impl Default for KotlinParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for KotlinParser {
    fn language(&self) -> &'static str {
        "kotlin"
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
        &["kt", "kts"]
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_parse_kotlin_fixture() {
        let mut parser = KotlinParser::new();
        let fixture_path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/kotlin/sample.kt"
        ));
        let content = std::fs::read(&fixture_path).expect("Failed to read fixture");
        let result = parser.parse_file(&fixture_path, &content, "src/sample.kt");

        assert_eq!(result.error_count, 0);
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Interface && symbol.name == "Greeter"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Class && symbol.name == "UserService"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Method && symbol.name == "greet"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Implement
                && dependency.to_symbol == "Greeter"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "format"));
        assert_eq!(result.imports.len(), 3);
    }

    #[test]
    fn test_parse_kotlin_inheritance_constructor_and_calls() {
        let mut parser = KotlinParser::new();
        let content = br#"
open class Base
interface Worker

class App : Base(), Worker {
    fun run() {
        val helper = Helper()
        helper.work()
    }
}

class Helper {
    fun work() {}
}
"#;

        let result = parser.parse_file(&PathBuf::from("App.kt"), content, "src/App.kt");

        assert_eq!(result.error_count, 0);
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Inherit
                && dependency.to_symbol == "Base"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Implement
                && dependency.to_symbol == "Worker"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::TypeUse
                && dependency.to_symbol == "Helper"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "work"));
    }
}
