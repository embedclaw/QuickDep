//! Ruby parser using tree-sitter-ruby.

use std::path::Path;

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource, Visibility};
use crate::parser::{make_qualified_name, node_text, ParseResult, Parser};
use crate::resolver::{Import, ImportKind};

/// Ruby parser using tree-sitter-ruby.
pub struct RubyParser {
    parser: tree_sitter::Parser,
}

impl RubyParser {
    /// Create a new Ruby parser.
    ///
    /// # Panics
    /// Panics if the bundled tree-sitter grammar cannot be loaded.
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_ruby::LANGUAGE.into())
            .expect("Failed to set Ruby language for tree-sitter parser");
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
            "class" => {
                if let Some((nested_name, qualified_name)) = self.extract_type_declaration(
                    node,
                    source,
                    file_path,
                    result,
                    parent_name,
                    SymbolKind::Class,
                ) {
                    self.extract_superclass(node, source, file_path, result, &qualified_name);
                    if let Some(body) = node.child_by_field_name("body") {
                        self.extract_symbols(
                            &body,
                            source,
                            file_path,
                            result,
                            Some(&nested_name),
                            Some(&qualified_name),
                        );
                    }
                    return;
                }
            }
            "module" => {
                if let Some((nested_name, qualified_name)) = self.extract_type_declaration(
                    node,
                    source,
                    file_path,
                    result,
                    parent_name,
                    SymbolKind::Module,
                ) {
                    if let Some(body) = node.child_by_field_name("body") {
                        self.extract_symbols(
                            &body,
                            source,
                            file_path,
                            result,
                            Some(&nested_name),
                            Some(&qualified_name),
                        );
                    }
                    return;
                }
            }
            "method" => {
                if let Some(qualified_name) =
                    self.extract_method(node, source, file_path, result, parent_name)
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
            "singleton_method" => {
                if let Some(qualified_name) =
                    self.extract_singleton_method(node, source, file_path, result, parent_name)
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
            "assignment" => {
                self.extract_constant_assignment(node, source, file_path, result, parent_name);
            }
            "call" => {
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
        let name = self.primary_name(&name_node, source)?;
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
            .with_visibility(Visibility::Public)
            .with_source(SymbolSource::Local),
        );

        Some((nested_name, qualified_name))
    }

    fn extract_method(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.primary_name(&name_node, source)?;
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
        .with_visibility(Visibility::Public)
        .with_source(SymbolSource::Local);

        if let Some(signature) = self.method_signature(node, source, &name) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some(qualified_name)
    }

    fn extract_singleton_method(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.primary_name(&name_node, source)?;
        let receiver_name = node
            .child_by_field_name("object")
            .and_then(|object| self.primary_name(&object, source))
            .or_else(|| parent_name.map(ToString::to_string));
        let qualified_name = make_qualified_name(file_path, &name, receiver_name.as_deref());
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

        if let Some(signature) = self.method_signature(node, source, &name) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some(qualified_name)
    }

    fn extract_constant_assignment(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
    ) {
        let Some(left) = node.child_by_field_name("left") else {
            return;
        };
        if !matches!(left.kind(), "constant" | "scope_resolution") {
            return;
        }
        let Some(name) = self.primary_name(&left, source) else {
            return;
        };
        let (line, column) = self.node_start(node);
        let mut symbol = Symbol::new(
            name.clone(),
            make_qualified_name(file_path, &name, parent_name),
            SymbolKind::Constant,
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

    fn extract_superclass(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        from_symbol: &str,
    ) {
        let Some(superclass) = node.child_by_field_name("superclass") else {
            return;
        };
        let Some(name) = self.reference_name(&superclass, source) else {
            return;
        };
        result.dependencies.push(Dependency::new(
            from_symbol.to_string(),
            name,
            file_path.to_string(),
            self.node_start(node).0,
            DependencyKind::Inherit,
        ));
    }

    fn extract_call(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        enclosing_symbol: Option<&str>,
    ) {
        let Some(method_node) = node.child_by_field_name("method") else {
            return;
        };
        let Some(method_name) = self.primary_name(&method_node, source) else {
            return;
        };

        if matches!(method_name.as_str(), "require" | "require_relative") {
            self.extract_require_import(&method_name, node, source, file_path, result);
            return;
        }

        if matches!(method_name.as_str(), "include" | "extend" | "prepend") {
            self.extract_mixin_dependency(node, source, file_path, result, enclosing_symbol);
            return;
        }

        let Some(from_symbol) = enclosing_symbol else {
            return;
        };

        if method_name == "new" {
            let receiver_node = node.child_by_field_name("receiver");
            let receiver_is_type = receiver_node
                .as_ref()
                .map(tree_sitter::Node::kind)
                .is_some_and(|kind| matches!(kind, "constant" | "scope_resolution"));
            if receiver_is_type {
                if let Some(receiver) =
                    receiver_node.and_then(|receiver| self.reference_name(&receiver, source))
                {
                    result.dependencies.push(Dependency::new(
                        from_symbol.to_string(),
                        receiver,
                        file_path.to_string(),
                        self.node_start(node).0,
                        DependencyKind::TypeUse,
                    ));
                }
                return;
            }

            result.dependencies.push(Dependency::new(
                from_symbol.to_string(),
                method_name,
                file_path.to_string(),
                self.node_start(node).0,
                DependencyKind::Call,
            ));
            return;
        }

        result.dependencies.push(Dependency::new(
            from_symbol.to_string(),
            method_name,
            file_path.to_string(),
            self.node_start(node).0,
            DependencyKind::Call,
        ));
    }

    fn extract_require_import(
        &self,
        method_name: &str,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let Some(arguments) = node.child_by_field_name("arguments") else {
            return;
        };
        let Some(argument) = arguments.named_child(0) else {
            return;
        };
        let Some(path) = self.string_literal_content(&argument, source) else {
            return;
        };
        let source_path = if method_name == "require_relative" && !path.starts_with('.') {
            format!("./{path}")
        } else {
            path
        };

        result.imports.push(Import::new(
            source_path,
            file_path.to_string(),
            self.node_start(node).0,
            ImportKind::Glob,
        ));
    }

    fn extract_mixin_dependency(
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
        let Some(arguments) = node.child_by_field_name("arguments") else {
            return;
        };
        let Some(argument) = arguments.named_child(0) else {
            return;
        };
        let Some(target) = self.reference_name(&argument, source) else {
            return;
        };

        result.dependencies.push(Dependency::new(
            from_symbol.to_string(),
            target,
            file_path.to_string(),
            self.node_start(node).0,
            DependencyKind::Implement,
        ));
    }

    fn method_signature(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        name: &str,
    ) -> Option<String> {
        let parameters = node
            .child_by_field_name("parameters")
            .and_then(|parameters| node_text(&parameters, source))
            .unwrap_or_else(|| "()".to_string());
        Some(format!("{name}{parameters}"))
    }

    fn string_literal_content(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
    ) -> Option<String> {
        match node.kind() {
            "string" | "bare_string" => {
                let mut parts = String::new();
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "string_content" {
                        parts.push_str(node_text(&child, source)?.as_str());
                    }
                }
                (!parts.is_empty()).then_some(parts)
            }
            _ => node_text(node, source)
                .map(|text| text.trim_matches('"').trim_matches('\'').to_string()),
        }
    }

    fn primary_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "identifier" | "constant" => node_text(node, source),
            "scope_resolution" => node_text(node, source).map(|text| self.last_path_segment(&text)),
            "superclass" => node
                .named_child(0)
                .and_then(|child| self.primary_name(&child, source)),
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

    fn reference_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "scope_resolution" => node_text(node, source),
            "superclass" => node
                .named_child(0)
                .and_then(|child| self.reference_name(&child, source)),
            _ => self.primary_name(node, source),
        }
    }

    fn last_path_segment(&self, path: &str) -> String {
        path.rsplit(':')
            .find(|segment| !segment.is_empty())
            .unwrap_or(path)
            .to_string()
    }

    fn node_start(&self, node: &tree_sitter::Node<'_>) -> (u32, u32) {
        let position = node.start_position();
        (position.row as u32 + 1, position.column as u32 + 1)
    }
}

impl Default for RubyParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for RubyParser {
    fn language(&self) -> &'static str {
        "ruby"
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
        &["rb", "rake"]
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_parse_ruby_fixture() {
        let mut parser = RubyParser::new();
        let fixture_path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/ruby/sample.rb"
        ));
        let content = std::fs::read(&fixture_path).expect("Failed to read fixture");
        let result = parser.parse_file(&fixture_path, &content, "lib/sample.rb");

        assert_eq!(result.error_count, 0);
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Module && symbol.name == "Acme"));
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
                && dependency.to_symbol == "Acme::Shared::Formatter"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::TypeUse
                && dependency.to_symbol == "Helper"));
        assert_eq!(result.imports.len(), 1);
    }

    #[test]
    fn test_parse_ruby_calls_and_constant_assignment() {
        let mut parser = RubyParser::new();
        let content = br#"
VALUE = 1

class App
  def self.call(name)
    puts format(name)
  end
end
"#;

        let result = parser.parse_file(&PathBuf::from("app.rb"), content, "app.rb");

        assert_eq!(result.error_count, 0);
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Constant && symbol.name == "VALUE"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "puts"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "format"));
    }

    #[test]
    fn test_dynamic_new_call_stays_as_call_dependency() {
        let mut parser = RubyParser::new();
        let content = br#"
class App
  def run(factory)
    factory.new
  end
end
"#;

        let result = parser.parse_file(&PathBuf::from("app.rb"), content, "app.rb");

        assert_eq!(result.error_count, 0);
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "new"));
        assert!(!result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::TypeUse));
    }
}
