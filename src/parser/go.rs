//! Go parser using tree-sitter-go.

use std::path::Path;

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource, Visibility};
use crate::parser::{make_qualified_name, node_text, ParseResult, Parser};
use crate::resolver::{Import, ImportKind};

/// Go parser using tree-sitter-go.
pub struct GoParser {
    parser: tree_sitter::Parser,
}

impl GoParser {
    /// Create a new Go parser.
    ///
    /// # Panics
    /// Panics if the bundled tree-sitter grammar cannot be loaded.
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_go::LANGUAGE.into())
            .expect("Failed to set Go language for tree-sitter parser");
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
            "function_declaration" => {
                if let Some(qualified_name) =
                    self.extract_function(node, source, file_path, result, None)
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
                    self.extract_function(node, source, file_path, result, parent_name)
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
            "type_spec" => {
                if let Some(type_name) = self.extract_type_spec(node, source, file_path, result) {
                    if let Some(type_node) = node.child_by_field_name("type") {
                        self.extract_symbols(
                            &type_node,
                            source,
                            file_path,
                            result,
                            Some(&type_name),
                            enclosing_symbol,
                        );
                    }
                    return;
                }
            }
            "type_alias" => {
                self.extract_type_alias(node, source, file_path, result);
            }
            "method_elem" => {
                self.extract_interface_method(node, source, file_path, result, parent_name);
                return;
            }
            "const_declaration" => {
                self.extract_value_declaration(
                    node,
                    source,
                    file_path,
                    result,
                    "const_spec",
                    SymbolKind::Constant,
                );
            }
            "var_declaration" => {
                self.extract_value_declaration(
                    node,
                    source,
                    file_path,
                    result,
                    "var_spec",
                    SymbolKind::Variable,
                );
            }
            "import_declaration" => {
                self.extract_import_declaration(node, source, file_path, result);
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

    fn extract_function(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        interface_parent: Option<&str>,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(&name_node, source)?;
        let parent_name = if node.kind() == "method_declaration" {
            self.receiver_type_name(node, source)
                .or_else(|| interface_parent.map(ToOwned::to_owned))
        } else {
            interface_parent.map(ToOwned::to_owned)
        };
        let qualified_name = make_qualified_name(file_path, &name, parent_name.as_deref());
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
        .with_visibility(self.visibility_for_name(&name))
        .with_source(SymbolSource::Local);

        if let Some(signature) = self.function_signature(node, source, &name) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some(qualified_name)
    }

    fn extract_type_spec(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let type_node = node.child_by_field_name("type")?;
        let name = node_text(&name_node, source)?;
        let kind = match type_node.kind() {
            "struct_type" => SymbolKind::Struct,
            "interface_type" => SymbolKind::Interface,
            _ => SymbolKind::TypeAlias,
        };
        let is_type_alias = matches!(kind, SymbolKind::TypeAlias);
        let qualified_name = make_qualified_name(file_path, &name, None);
        let (line, column) = self.node_start(node);

        let mut symbol = Symbol::new(
            name.clone(),
            qualified_name,
            kind,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(self.visibility_for_name(&name))
        .with_source(SymbolSource::Local);

        if is_type_alias {
            if let Some(signature) = self.type_signature(&type_node, source, &name) {
                symbol = symbol.with_signature(signature);
            }
        }

        result.symbols.push(symbol);
        Some(name)
    }

    fn extract_type_alias(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Some(type_node) = node.child_by_field_name("type") else {
            return;
        };
        let Some(name) = node_text(&name_node, source) else {
            return;
        };
        let qualified_name = make_qualified_name(file_path, &name, None);
        let (line, column) = self.node_start(node);

        let mut symbol = Symbol::new(
            name.clone(),
            qualified_name,
            SymbolKind::TypeAlias,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(self.visibility_for_name(&name))
        .with_source(SymbolSource::Local);

        if let Some(signature) = self.type_signature(&type_node, source, &name) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
    }

    fn extract_interface_method(
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
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Some(name) = node_text(&name_node, source) else {
            return;
        };
        let qualified_name = make_qualified_name(file_path, &name, Some(parent_name));
        let (line, column) = self.node_start(node);

        let mut symbol = Symbol::new(
            name.clone(),
            qualified_name,
            SymbolKind::Method,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(self.visibility_for_name(&name))
        .with_source(SymbolSource::Local);

        if let Some(signature) = self.function_signature(node, source, &name) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
    }

    fn extract_value_declaration(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        spec_kind: &str,
        kind: SymbolKind,
    ) {
        let mut cursor = node.walk();
        for spec in node.named_children(&mut cursor) {
            if spec.kind() != spec_kind {
                continue;
            }

            let (line, column) = self.node_start(&spec);
            let mut spec_cursor = spec.walk();
            for identifier in spec.children_by_field_name("name", &mut spec_cursor) {
                let Some(name) = node_text(&identifier, source) else {
                    continue;
                };
                let qualified_name = make_qualified_name(file_path, &name, None);
                result.symbols.push(
                    Symbol::new(
                        name.clone(),
                        qualified_name,
                        kind.clone(),
                        file_path.to_string(),
                        line,
                        column,
                    )
                    .with_visibility(self.visibility_for_name(&name))
                    .with_source(SymbolSource::Local),
                );
            }
        }
    }

    fn extract_import_declaration(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let line = self.node_start(node).0;
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "import_spec" {
                self.extract_import_spec(&child, source, file_path, line, result);
                continue;
            }

            if child.kind() == "import_spec_list" {
                let mut spec_cursor = child.walk();
                for spec in child.named_children(&mut spec_cursor) {
                    if spec.kind() == "import_spec" {
                        self.extract_import_spec(&spec, source, file_path, line, result);
                    }
                }
            }
        }
    }

    fn extract_import_spec(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        line: u32,
        result: &mut ParseResult,
    ) {
        let Some(path_node) = node.child_by_field_name("path") else {
            return;
        };
        let Some(path_text) = node_text(&path_node, source) else {
            return;
        };
        let import_path = self.strip_quotes(&path_text).replace('/', "::");
        let name_node = node.child_by_field_name("name");

        match name_node.and_then(|name| node_text(&name, source)) {
            Some(name) if name == "_" => {}
            Some(name) if name == "." => result.imports.push(Import::new(
                import_path,
                file_path.to_string(),
                line,
                ImportKind::Glob,
            )),
            Some(alias) => result.imports.push(
                Import::new(import_path, file_path.to_string(), line, ImportKind::Alias)
                    .with_alias(alias),
            ),
            None => result.imports.push(Import::new(
                import_path,
                file_path.to_string(),
                line,
                ImportKind::Named,
            )),
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
        let Some(function_node) = node.child_by_field_name("function") else {
            return;
        };
        let Some(target) = self.call_target(&function_node, source) else {
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

    fn call_target(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "identifier" | "field_identifier" => node_text(node, source),
            "selector_expression" => node
                .child_by_field_name("field")
                .and_then(|field| node_text(&field, source)),
            "parenthesized_expression" => {
                let mut cursor = node.walk();
                let target = node
                    .named_children(&mut cursor)
                    .find_map(|child| self.call_target(&child, source));
                target
            }
            _ => None,
        }
    }

    fn receiver_type_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        let receiver = node.child_by_field_name("receiver")?;
        self.first_type_identifier(&receiver, source)
    }

    fn first_type_identifier(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "type_identifier" => node_text(node, source),
            "qualified_type" => node
                .child_by_field_name("name")
                .and_then(|name| node_text(&name, source)),
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if let Some(name) = self.first_type_identifier(&child, source) {
                        return Some(name);
                    }
                }
                None
            }
        }
    }

    fn function_signature(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        name: &str,
    ) -> Option<String> {
        let type_parameters = node
            .child_by_field_name("type_parameters")
            .and_then(|type_parameters| node_text(&type_parameters, source))
            .unwrap_or_default();
        let parameters = node
            .child_by_field_name("parameters")
            .and_then(|parameters| node_text(&parameters, source))
            .unwrap_or_else(|| "()".to_string());
        let result = node
            .child_by_field_name("result")
            .and_then(|result| node_text(&result, source))
            .map(|result| format!(" {result}"))
            .unwrap_or_default();
        Some(format!("{name}{type_parameters}{parameters}{result}"))
    }

    fn type_signature(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        name: &str,
    ) -> Option<String> {
        node_text(node, source).map(|type_name| format!("{name} {type_name}"))
    }

    fn visibility_for_name(&self, name: &str) -> Visibility {
        match name.chars().next() {
            Some(first) if first.is_ascii_uppercase() => Visibility::Public,
            _ => Visibility::Private,
        }
    }

    fn strip_quotes(&self, value: &str) -> String {
        value.trim().trim_matches('"').trim_matches('`').to_string()
    }

    fn node_start(&self, node: &tree_sitter::Node<'_>) -> (u32, u32) {
        let position = node.start_position();
        (position.row as u32 + 1, position.column as u32 + 1)
    }
}

impl Default for GoParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for GoParser {
    fn language(&self) -> &'static str {
        "go"
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
        &["go"]
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_parse_go_fixture() {
        let mut parser = GoParser::new();
        let fixture_path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/go/sample.go"
        ));
        let content = std::fs::read(&fixture_path).expect("Failed to read fixture");
        let result = parser.parse_file(&fixture_path, &content, "src/sample.go");

        assert_eq!(result.error_count, 0);
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Interface && symbol.name == "Greeter"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Struct && symbol.name == "UserService"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Method
                && symbol.qualified_name == "src/sample.go::UserService::Greet"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Constant && symbol.name == "Version"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Variable && symbol.name == "defaultService"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::TypeAlias && symbol.name == "Transformer"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "FormatName"));
        assert!(result
            .imports
            .iter()
            .any(|import| import.alias.as_deref() == Some("helper")));
        assert!(result
            .imports
            .iter()
            .any(|import| import.kind == ImportKind::Glob));
    }

    #[test]
    fn test_parse_go_method_receiver_and_calls() {
        let mut parser = GoParser::new();
        let content = br#"
type service struct{}

func (s *service) run() {
    helper()
}

func helper() {}
"#;
        let result = parser.parse_file(&PathBuf::from("service.go"), content, "src/service.go");

        assert_eq!(result.error_count, 0);
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.qualified_name == "src/service.go::service::run"));
        assert!(result
            .dependencies
            .iter()
            .any(
                |dependency| dependency.from_symbol == "src/service.go::service::run"
                    && dependency.to_symbol == "helper"
            ));
    }

    #[test]
    fn test_parse_go_generic_function_signature() {
        let mut parser = GoParser::new();
        let content = br#"
func Zero[T any]() (v T) {
    return
}
"#;
        let result = parser.parse_file(&PathBuf::from("generic.go"), content, "src/generic.go");

        assert_eq!(result.error_count, 0);
        let zero = result
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Zero")
            .expect("generic function should be parsed");
        assert_eq!(zero.signature.as_deref(), Some("Zero[T any]() (v T)"));
    }
}
