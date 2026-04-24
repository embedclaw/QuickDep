//! PHP parser using tree-sitter-php.

use std::path::Path;

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource, Visibility};
use crate::parser::{make_qualified_name, node_text, ParseResult, Parser};
use crate::resolver::{Import, ImportKind};

/// PHP parser using tree-sitter-php.
pub struct PhpParser {
    parser: tree_sitter::Parser,
}

impl PhpParser {
    /// Create a new PHP parser.
    ///
    /// # Panics
    /// Panics if the bundled tree-sitter grammar cannot be loaded.
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
            .expect("Failed to set PHP language for tree-sitter parser");
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
                if let Some((nested_name, qualified_name)) = self.extract_type_declaration(
                    node,
                    source,
                    file_path,
                    result,
                    parent_name,
                    SymbolKind::Class,
                ) {
                    self.extract_class_relationships(
                        node,
                        source,
                        file_path,
                        result,
                        &qualified_name,
                    );
                    if let Some(body) = node.child_by_field_name("body") {
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
            "interface_declaration" => {
                if let Some((nested_name, qualified_name)) = self.extract_type_declaration(
                    node,
                    source,
                    file_path,
                    result,
                    parent_name,
                    SymbolKind::Interface,
                ) {
                    self.extract_interface_relationships(
                        node,
                        source,
                        file_path,
                        result,
                        &qualified_name,
                    );
                    if let Some(body) = node.child_by_field_name("body") {
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
            "trait_declaration" => {
                if let Some((nested_name, _)) = self.extract_type_declaration(
                    node,
                    source,
                    file_path,
                    result,
                    parent_name,
                    SymbolKind::Trait,
                ) {
                    if let Some(body) = node.child_by_field_name("body") {
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
            "enum_declaration" => {
                if let Some((nested_name, _)) = self.extract_type_declaration(
                    node,
                    source,
                    file_path,
                    result,
                    parent_name,
                    SymbolKind::Enum,
                ) {
                    if let Some(body) = node.child_by_field_name("body") {
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
            "function_definition" => {
                if let Some(qualified_name) =
                    self.extract_function_definition(node, source, file_path, result, parent_name)
                {
                    if let Some(body) = node.child_by_field_name("body") {
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
            "method_declaration" => {
                if let Some(qualified_name) =
                    self.extract_method_declaration(node, source, file_path, result, parent_name)
                {
                    if let Some(body) = node.child_by_field_name("body") {
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
            "property_declaration" => {
                self.extract_property_declaration(node, source, file_path, result, parent_name);
            }
            "const_declaration" => {
                self.extract_const_declaration(node, source, file_path, result, parent_name);
            }
            "namespace_use_declaration" => {
                self.extract_namespace_use_declaration(node, source, file_path, result);
            }
            "function_call_expression" => {
                self.extract_function_call(node, source, file_path, result, enclosing_symbol);
            }
            "member_call_expression" | "scoped_call_expression" => {
                self.extract_named_call(node, source, file_path, result, enclosing_symbol);
            }
            "object_creation_expression" => {
                self.extract_object_creation(node, source, file_path, result, enclosing_symbol);
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
        let name = self.clean_identifier(&node_text(&name_node, source)?);
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

    fn extract_function_definition(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.clean_identifier(&node_text(&name_node, source)?);
        let qualified_name = make_qualified_name(file_path, &name, parent_name);
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

        if let Some(signature) = self.callable_signature(node, source, &name) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some(qualified_name)
    }

    fn extract_method_declaration(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.clean_identifier(&node_text(&name_node, source)?);
        let qualified_name = make_qualified_name(file_path, &name, parent_name);
        let (line, column) = self.node_start(node);

        let mut symbol = Symbol::new(
            name.clone(),
            qualified_name.clone(),
            SymbolKind::Method,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(self.visibility_for_node(node, source))
        .with_source(SymbolSource::Local);

        if let Some(signature) = self.callable_signature(node, source, &name) {
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
        let Some(parent_name) = parent_name else {
            return;
        };
        let (line, column) = self.node_start(node);
        let visibility = self.visibility_for_node(node, source);
        let signature = node_text(node, source);

        for name in self.property_names(node, source) {
            let mut symbol = Symbol::new(
                name.clone(),
                make_qualified_name(file_path, &name, Some(parent_name)),
                SymbolKind::Property,
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

    fn extract_const_declaration(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
    ) {
        let (line, column) = self.node_start(node);
        let visibility = self.visibility_for_node(node, source);
        let signature = node_text(node, source);

        for name in self.const_names(node, source) {
            let mut symbol = Symbol::new(
                name.clone(),
                make_qualified_name(file_path, &name, parent_name),
                SymbolKind::Constant,
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

    fn extract_class_relationships(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        from_symbol: &str,
    ) {
        let line = self.node_start(node).0;
        if let Some(base_clause) = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "base_clause")
        {
            for name in self.named_type_children(&base_clause, source) {
                result.dependencies.push(Dependency::new(
                    from_symbol.to_string(),
                    name,
                    file_path.to_string(),
                    line,
                    DependencyKind::Inherit,
                ));
            }
        }

        if let Some(interface_clause) = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "class_interface_clause")
        {
            for name in self.named_type_children(&interface_clause, source) {
                result.dependencies.push(Dependency::new(
                    from_symbol.to_string(),
                    name,
                    file_path.to_string(),
                    line,
                    DependencyKind::Implement,
                ));
            }
        }
    }

    fn extract_interface_relationships(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        from_symbol: &str,
    ) {
        let Some(base_clause) = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "base_clause")
        else {
            return;
        };
        let line = self.node_start(node).0;
        for name in self.named_type_children(&base_clause, source) {
            result.dependencies.push(Dependency::new(
                from_symbol.to_string(),
                name,
                file_path.to_string(),
                line,
                DependencyKind::Inherit,
            ));
        }
    }

    fn extract_namespace_use_declaration(
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
            .strip_prefix("use ")
            .map(str::trim)
        else {
            return;
        };
        let line = self.node_start(node).0;
        self.push_php_use_imports(content, file_path, line, result);
    }

    fn push_php_use_imports(
        &self,
        content: &str,
        file_path: &str,
        line: u32,
        result: &mut ParseResult,
    ) {
        let content = content
            .strip_prefix("function ")
            .or_else(|| content.strip_prefix("const "))
            .unwrap_or(content)
            .trim();

        if let Some((prefix, rest)) = content.split_once('{') {
            let prefix = prefix.trim().trim_end_matches('\\');
            let rest = rest.trim_end_matches('}').trim();
            for clause in rest
                .split(',')
                .map(str::trim)
                .filter(|clause| !clause.is_empty())
            {
                let source = if prefix.is_empty() {
                    clause.to_string()
                } else {
                    format!("{prefix}\\{clause}")
                };
                self.push_single_php_use_import(&source, file_path, line, result);
            }
            return;
        }

        self.push_single_php_use_import(content, file_path, line, result);
    }

    fn push_single_php_use_import(
        &self,
        content: &str,
        file_path: &str,
        line: u32,
        result: &mut ParseResult,
    ) {
        let (source, alias) = split_php_alias(content);
        if source.is_empty() {
            return;
        }

        let kind = if alias.is_some() {
            ImportKind::Alias
        } else {
            ImportKind::Named
        };
        let mut import = Import::new(source.to_string(), file_path.to_string(), line, kind);
        if let Some(alias) = alias {
            import = import.with_alias(alias.to_string());
        }
        result.imports.push(import);
    }

    fn extract_function_call(
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
        let Some(function_node) = node.child_by_field_name("function") else {
            return;
        };
        let Some(target) = self.primary_name(&function_node, source) else {
            return;
        };

        result.dependencies.push(Dependency::new(
            from_symbol.to_string(),
            target,
            file_path.to_string(),
            self.node_start(node).0,
            DependencyKind::Call,
        ));
    }

    fn extract_named_call(
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
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Some(target) = self.primary_name(&name_node, source) else {
            return;
        };

        result.dependencies.push(Dependency::new(
            from_symbol.to_string(),
            target,
            file_path.to_string(),
            self.node_start(node).0,
            DependencyKind::Call,
        ));
    }

    fn extract_object_creation(
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
        let Some(target) = node
            .named_child(0)
            .and_then(|child| self.primary_name(&child, source))
        else {
            return;
        };

        result.dependencies.push(Dependency::new(
            from_symbol.to_string(),
            target,
            file_path.to_string(),
            self.node_start(node).0,
            DependencyKind::TypeUse,
        ));
    }

    fn named_type_children(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Vec<String> {
        let mut names = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if matches!(child.kind(), "name" | "qualified_name" | "relative_name") {
                if let Some(name) = self.primary_name(&child, source) {
                    names.push(name);
                }
            }
        }
        names.sort();
        names.dedup();
        names
    }

    fn property_names(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Vec<String> {
        let mut names = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "property_element" {
                if let Some(name) = child
                    .named_children(&mut child.walk())
                    .find(|nested| nested.kind() == "variable_name")
                    .and_then(|nested| node_text(&nested, source))
                {
                    names.push(self.clean_identifier(&name));
                }
                continue;
            }

            names.extend(self.property_names(&child, source));
        }
        names
    }

    fn const_names(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Vec<String> {
        let mut names = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "const_element" {
                if let Some(name) = child
                    .named_child(0)
                    .and_then(|nested| node_text(&nested, source))
                {
                    names.push(self.clean_identifier(&name));
                }
                continue;
            }

            names.extend(self.const_names(&child, source));
        }
        names
    }

    fn primary_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "name" | "qualified_name" | "relative_name" | "namespace_name" | "variable_name" => {
                node_text(node, source)
                    .map(|text| self.clean_identifier(&self.last_path_segment(&text)))
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if let Some(name) = self.primary_name(&child, source) {
                        return Some(name);
                    }
                }
                None
            }
        }
    }

    fn callable_signature(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        name: &str,
    ) -> Option<String> {
        let parameters = node
            .child_by_field_name("parameters")
            .and_then(|parameters| node_text(&parameters, source))?;
        let return_type = node
            .child_by_field_name("return_type")
            .and_then(|return_type| node_text(&return_type, source))
            .unwrap_or_default();
        Some(format!("{name}{parameters}{return_type}"))
    }

    fn visibility_for_node(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Visibility {
        if self.has_modifier(node, source, "private") {
            Visibility::Private
        } else if self.has_modifier(node, source, "protected") {
            Visibility::Protected
        } else {
            Visibility::Public
        }
    }

    fn has_modifier(&self, node: &tree_sitter::Node<'_>, source: &[u8], modifier: &str) -> bool {
        node.children(&mut node.walk())
            .any(|child| node_text(&child, source).as_deref() == Some(modifier))
    }

    fn clean_identifier(&self, identifier: &str) -> String {
        identifier
            .trim()
            .trim_start_matches('$')
            .trim_start_matches('\\')
            .trim_matches('`')
            .to_string()
    }

    fn last_path_segment(&self, path: &str) -> String {
        path.rsplit(['\\', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or(path)
            .to_string()
    }

    fn node_start(&self, node: &tree_sitter::Node<'_>) -> (u32, u32) {
        let position = node.start_position();
        (position.row as u32 + 1, position.column as u32 + 1)
    }
}

fn split_php_alias(content: &str) -> (&str, Option<&str>) {
    if let Some((source, alias)) = content.split_once(" as ") {
        (source.trim(), Some(alias.trim()))
    } else {
        (content.trim(), None)
    }
}

impl Default for PhpParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for PhpParser {
    fn language(&self) -> &'static str {
        "php"
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
        &["php", "phtml"]
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_parse_php_fixture() {
        let mut parser = PhpParser::new();
        let fixture_path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/php/sample.php"
        ));
        let content = std::fs::read(&fixture_path).expect("Failed to read fixture");
        let result = parser.parse_file(&fixture_path, &content, "src/sample.php");

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
            .any(|dependency| dependency.kind == DependencyKind::Inherit
                && dependency.to_symbol == "BaseService"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Implement
                && dependency.to_symbol == "Greeter"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "format_name"));
        assert_eq!(result.imports.len(), 3);
    }

    #[test]
    fn test_parse_php_constructor_and_member_calls() {
        let mut parser = PhpParser::new();
        let content = br#"<?php
class App {
    public function run(): void {
        $helper = new Helper();
        $helper->work();
        Helper::staticWork();
    }
}

class Helper {
    public function work(): void {}
    public static function staticWork(): void {}
}
"#;

        let result = parser.parse_file(&PathBuf::from("App.php"), content, "src/App.php");

        assert_eq!(result.error_count, 0);
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
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "staticWork"));
    }
}
