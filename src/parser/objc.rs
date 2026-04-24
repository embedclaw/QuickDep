//! Objective-C parser using tree-sitter-objc.

use std::path::Path;

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource, Visibility};
use crate::parser::{make_qualified_name, node_text, ParseResult, Parser};
use crate::resolver::{Import, ImportKind};

/// Objective-C parser using tree-sitter-objc.
pub struct ObjcParser {
    parser: tree_sitter::Parser,
}

impl ObjcParser {
    /// Create a new Objective-C parser.
    ///
    /// # Panics
    /// Panics if the bundled tree-sitter grammar cannot be loaded.
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_objc::LANGUAGE.into())
            .expect("Failed to set Objective-C language for tree-sitter parser");
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
            "class_interface" => {
                if let Some((name, qualified_name)) =
                    self.extract_class_like(node, source, file_path, result, SymbolKind::Class)
                {
                    self.extract_inheritance(node, source, file_path, result, &qualified_name);
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        self.extract_symbols(
                            &child,
                            source,
                            file_path,
                            result,
                            Some(&name),
                            Some(&qualified_name),
                        );
                    }
                    return;
                }
            }
            "class_implementation" => {
                if let Some((name, qualified_name)) =
                    self.extract_class_like(node, source, file_path, result, SymbolKind::Class)
                {
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        self.extract_symbols(
                            &child,
                            source,
                            file_path,
                            result,
                            Some(&name),
                            Some(&qualified_name),
                        );
                    }
                    return;
                }
            }
            "protocol_declaration" => {
                if let Some((name, qualified_name)) =
                    self.extract_class_like(node, source, file_path, result, SymbolKind::Interface)
                {
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        self.extract_symbols(
                            &child,
                            source,
                            file_path,
                            result,
                            Some(&name),
                            Some(&qualified_name),
                        );
                    }
                    return;
                }
            }
            "method_definition" | "method_declaration" => {
                if let Some(parent_name) = parent_name {
                    if let Some(qualified_name) =
                        self.extract_method(node, source, file_path, result, parent_name)
                    {
                        if let Some(body) = self.method_body(node) {
                            self.extract_symbols(
                                &body,
                                source,
                                file_path,
                                result,
                                Some(parent_name),
                                Some(&qualified_name),
                            );
                        }
                    }
                    return;
                }
            }
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
                            None,
                            Some(&qualified_name),
                        );
                    }
                    return;
                }
            }
            "preproc_include" => {
                self.extract_include(node, source, file_path, result);
            }
            "message_expression" => {
                self.extract_message_expression(node, source, file_path, result, enclosing_symbol);
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

    fn extract_class_like(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        kind: SymbolKind,
    ) -> Option<(String, String)> {
        let name = self.direct_child_text(node, source, "identifier")?;
        let qualified_name = make_qualified_name(file_path, &name, None);

        if self.symbol_exists(result, &qualified_name) {
            return Some((name, qualified_name));
        }

        let (line, column) = self.node_start(node);
        let mut symbol = Symbol::new(
            name.clone(),
            qualified_name.clone(),
            kind,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(Visibility::Public)
        .with_source(SymbolSource::Local);

        if let Some(signature) = self.line_signature(node, source) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some((name, qualified_name))
    }

    fn extract_method(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: &str,
    ) -> Option<String> {
        let name = self.direct_child_text(node, source, "identifier")?;
        let qualified_name = make_qualified_name(file_path, &name, Some(parent_name));

        if self.symbol_exists(result, &qualified_name) {
            return Some(qualified_name);
        }

        let (line, column) = self.node_start(node);
        let mut symbol = Symbol::new(
            name.clone(),
            qualified_name.clone(),
            SymbolKind::Method,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(Visibility::Public)
        .with_source(SymbolSource::Local);

        if let Some(signature) = self.line_signature(node, source) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some(qualified_name)
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

        if self.symbol_exists(result, &qualified_name) {
            return Some(qualified_name);
        }

        let (line, column) = self.node_start(node);
        let mut symbol = Symbol::new(
            name.clone(),
            qualified_name.clone(),
            SymbolKind::Function,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(Visibility::Public)
        .with_source(SymbolSource::Local);

        if let Some(signature) = node_text(&declarator, source) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some(qualified_name)
    }

    fn extract_inheritance(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        from_symbol: &str,
    ) {
        let mut class_name_seen = false;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" => {
                    let Some(name) = node_text(&child, source) else {
                        continue;
                    };
                    if !class_name_seen {
                        class_name_seen = true;
                        continue;
                    }
                    result.dependencies.push(Dependency::new(
                        from_symbol.to_string(),
                        name,
                        file_path.to_string(),
                        self.node_start(node).0,
                        DependencyKind::Inherit,
                    ));
                }
                "parameterized_arguments" => {
                    let mut args_cursor = child.walk();
                    for arg in child.named_children(&mut args_cursor) {
                        let Some(name) = self.type_name_text(&arg, source) else {
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
                _ => {}
            }
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

            if include.is_empty() || include == "#import" || include == "#include" {
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

    fn extract_message_expression(
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
        let Some(target) = self.message_target_name(node, source) else {
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

    fn symbol_exists(&self, result: &ParseResult, qualified_name: &str) -> bool {
        result
            .symbols
            .iter()
            .any(|symbol| symbol.qualified_name == qualified_name)
    }

    fn direct_child_text(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        kind: &str,
    ) -> Option<String> {
        let mut cursor = node.walk();
        let text = node
            .children(&mut cursor)
            .find(|child| child.kind() == kind)
            .and_then(|child| node_text(&child, source));
        text
    }

    fn type_name_text(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "type_identifier" | "identifier" => node_text(node, source),
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if let Some(name) = self.type_name_text(&child, source) {
                        return Some(name);
                    }
                }
                None
            }
        }
    }

    fn method_body<'a>(&self, node: &tree_sitter::Node<'a>) -> Option<tree_sitter::Node<'a>> {
        let mut cursor = node.walk();
        let body = node
            .children(&mut cursor)
            .find(|child| child.kind() == "compound_statement");
        body
    }

    fn line_signature(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        node_text(node, source)
            .and_then(|text| text.lines().next().map(str::trim).map(ToString::to_string))
    }

    fn message_target_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        let mut receiver_skipped = false;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "[" || child.kind() == "]" {
                continue;
            }
            if !receiver_skipped {
                receiver_skipped = true;
                continue;
            }
            if child.kind() == "identifier" {
                return node_text(&child, source);
            }
        }
        None
    }

    fn call_target_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "identifier" | "field_identifier" => node_text(node, source),
            "field_expression" => node
                .child_by_field_name("field")
                .and_then(|field| node_text(&field, source))
                .or_else(|| self.direct_child_text(node, source, "field_identifier")),
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

    fn node_start(&self, node: &tree_sitter::Node<'_>) -> (u32, u32) {
        let position = node.start_position();
        ((position.row + 1) as u32, (position.column + 1) as u32)
    }
}

impl Default for ObjcParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for ObjcParser {
    fn language(&self) -> &'static str {
        "objc"
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
        &["m"]
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_parse_objc_fixture() {
        let mut parser = ObjcParser::new();
        let fixture_path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/objc/sample.m"
        ));
        let content = std::fs::read(&fixture_path).expect("Failed to read fixture");
        let result = parser.parse_file(&fixture_path, &content, "src/sample.m");

        assert_eq!(result.error_count, 0);
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Class && symbol.name == "Calculator"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Interface && symbol.name == "ResultLogging"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Method && symbol.name == "add"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Method && symbol.name == "sharedCalculator"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Function && symbol.name == "main"));
        assert_eq!(result.imports.len(), 2);
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Inherit
                && dependency.to_symbol == "NSObject"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Inherit
                && dependency.to_symbol == "ResultLogging"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "logResult"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "sharedCalculator"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "NSLog"));
    }
}
