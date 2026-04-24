//! Python parser using tree-sitter-python.

use std::path::Path;

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource, Visibility};
use crate::parser::{make_qualified_name, node_text, ParseResult, Parser};
use crate::resolver::{Import, ImportKind};

/// Python parser using tree-sitter-python.
pub struct PythonParser {
    parser: tree_sitter::Parser,
}

impl PythonParser {
    /// Create a new Python parser.
    ///
    /// # Panics
    /// Panics if the bundled tree-sitter grammar cannot be loaded.
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("Failed to set Python language for tree-sitter parser");
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
            "decorated_definition" => {
                if let Some(definition) = node.child_by_field_name("definition") {
                    self.extract_symbols(
                        &definition,
                        source,
                        file_path,
                        result,
                        parent_name,
                        enclosing_symbol,
                    );
                    return;
                }
            }
            "function_definition" => {
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
            "class_definition" => {
                if let Some(class_name) = self.extract_class(node, source, file_path, result) {
                    self.extract_superclasses(node, source, file_path, result, &class_name);
                    if let Some(body) = node.child_by_field_name("body") {
                        self.extract_symbols(
                            &body,
                            source,
                            file_path,
                            result,
                            Some(&class_name),
                            enclosing_symbol,
                        );
                    }
                    return;
                }
            }
            "import_statement" => {
                self.extract_import_statement(node, source, file_path, result);
            }
            "import_from_statement" => {
                self.extract_import_from_statement(node, source, file_path, result);
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

    fn extract_function(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(&name_node, source)?;
        let qualified_name = make_qualified_name(file_path, &name, parent_name);
        let (line, column) = self.node_start(node);
        let kind = if parent_name.is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };
        let visibility = if name.starts_with('_') {
            Visibility::Private
        } else {
            Visibility::Public
        };

        let mut symbol = Symbol::new(
            name.clone(),
            qualified_name.clone(),
            kind,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(visibility)
        .with_source(SymbolSource::Local);

        if let Some(signature) = self.function_signature(node, source, &name) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some(qualified_name)
    }

    fn extract_class(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(&name_node, source)?;
        let qualified_name = make_qualified_name(file_path, &name, None);
        let (line, column) = self.node_start(node);

        result.symbols.push(
            Symbol::new(
                name.clone(),
                qualified_name,
                SymbolKind::Class,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(Visibility::Public)
            .with_source(SymbolSource::Local),
        );

        Some(name)
    }

    fn extract_superclasses(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        class_name: &str,
    ) {
        let Some(superclasses) = node.child_by_field_name("superclasses") else {
            return;
        };
        let from_symbol = make_qualified_name(file_path, class_name, None);

        let mut cursor = superclasses.walk();
        for child in superclasses.named_children(&mut cursor) {
            let Some(target) = self.reference_name(&child, source) else {
                continue;
            };
            result.dependencies.push(Dependency::new(
                from_symbol.clone(),
                target,
                file_path.to_string(),
                self.node_start(&superclasses).0,
                DependencyKind::Inherit,
            ));
        }
    }

    fn extract_import_statement(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let line = self.node_start(node).0;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "dotted_name" => {
                    if let Some(name) = node_text(&child, source) {
                        result.imports.push(Import::new(
                            name.replace('.', "::"),
                            file_path.to_string(),
                            line,
                            ImportKind::Named,
                        ));
                    }
                }
                "aliased_import" => {
                    let Some(name_node) = child.child_by_field_name("name") else {
                        continue;
                    };
                    let Some(alias_node) = child.child_by_field_name("alias") else {
                        continue;
                    };
                    let Some(name) = node_text(&name_node, source) else {
                        continue;
                    };
                    let Some(alias) = node_text(&alias_node, source) else {
                        continue;
                    };
                    result.imports.push(
                        Import::new(
                            name.replace('.', "::"),
                            file_path.to_string(),
                            line,
                            ImportKind::Alias,
                        )
                        .with_alias(alias),
                    );
                }
                _ => {}
            }
        }
    }

    fn extract_import_from_statement(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let line = self.node_start(node).0;
        let module_name = node
            .child_by_field_name("module_name")
            .and_then(|module_name| node_text(&module_name, source))
            .map(|name| name.replace('.', "::"))
            .unwrap_or_default();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "wildcard_import" => result.imports.push(Import::new(
                    module_name.clone(),
                    file_path.to_string(),
                    line,
                    ImportKind::Glob,
                )),
                "dotted_name" => {
                    if let Some(name) = node_text(&child, source) {
                        let source_name = name.replace('.', "::");
                        let source_name = if module_name.is_empty() {
                            source_name
                        } else {
                            format!("{module_name}::{source_name}")
                        };
                        result.imports.push(Import::new(
                            source_name,
                            file_path.to_string(),
                            line,
                            ImportKind::Named,
                        ));
                    }
                }
                "aliased_import" => {
                    let Some(name_node) = child.child_by_field_name("name") else {
                        continue;
                    };
                    let Some(alias_node) = child.child_by_field_name("alias") else {
                        continue;
                    };
                    let Some(name) = node_text(&name_node, source) else {
                        continue;
                    };
                    let Some(alias) = node_text(&alias_node, source) else {
                        continue;
                    };
                    let source_name = name.replace('.', "::");
                    let source_name = if module_name.is_empty() {
                        source_name
                    } else {
                        format!("{module_name}::{source_name}")
                    };
                    result.imports.push(
                        Import::new(source_name, file_path.to_string(), line, ImportKind::Alias)
                            .with_alias(alias),
                    );
                }
                _ => {}
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
        let Some(function_node) = node.child_by_field_name("function") else {
            return;
        };
        let Some(target) = self.reference_name(&function_node, source) else {
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

    fn reference_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "identifier" => node_text(node, source),
            "attribute" => node
                .child_by_field_name("attribute")
                .and_then(|attribute| node_text(&attribute, source)),
            _ => node_text(node, source)
                .map(|text| text.rsplit('.').next().unwrap_or_default().to_string())
                .filter(|text| !text.is_empty()),
        }
    }

    fn function_signature(
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
            .map(|value| format!(" -> {value}"))
            .unwrap_or_default();
        Some(format!("{name}{parameters}{return_type}"))
    }

    fn node_start(&self, node: &tree_sitter::Node<'_>) -> (u32, u32) {
        let position = node.start_position();
        (position.row as u32 + 1, position.column as u32 + 1)
    }
}

impl Default for PythonParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for PythonParser {
    fn language(&self) -> &'static str {
        "python"
    }

    fn parse_file(&mut self, path: &Path, content: &[u8], file_path: &str) -> ParseResult {
        let _ = path;
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
        &["py", "pyi"]
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_parse_python_fixture() {
        let mut parser = PythonParser::new();
        let fixture_path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/python/sample.py"
        ));
        let content = std::fs::read(&fixture_path).expect("Failed to read fixture");
        let result = parser.parse_file(&fixture_path, &content, "src/sample.py");

        assert_eq!(result.error_count, 0);
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Class && symbol.name == "Greeter"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Method && symbol.name == "greet"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Inherit));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "format_name"));
        assert!(result
            .imports
            .iter()
            .any(|import| import.alias.as_deref() == Some("helper")));
    }

    #[test]
    fn test_parse_decorated_function() {
        let mut parser = PythonParser::new();
        let content = br#"
class Service:
    @trace
    def run(self):
        return helper()

def helper():
    return "ok"
"#;
        let result = parser.parse_file(&PathBuf::from("service.py"), content, "src/service.py");

        assert_eq!(result.error_count, 0);
        assert!(result.symbols.iter().any(|symbol| symbol.name == "run"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "helper"));
    }
}
