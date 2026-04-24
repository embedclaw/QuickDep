//! Java parser using tree-sitter-java.

use std::path::Path;

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource, Visibility};
use crate::parser::{make_qualified_name, node_text, ParseResult, Parser};
use crate::resolver::{Import, ImportKind};

/// Java parser using tree-sitter-java.
pub struct JavaParser {
    parser: tree_sitter::Parser,
}

impl JavaParser {
    /// Create a new Java parser.
    ///
    /// # Panics
    /// Panics if the bundled tree-sitter grammar cannot be loaded.
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .expect("Failed to set Java language for tree-sitter parser");
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
            "class_declaration" | "record_declaration" => {
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
            "interface_declaration" | "annotation_type_declaration" => {
                if let Some((nested_name, qualified_name)) = self.extract_type_declaration(
                    node,
                    source,
                    file_path,
                    result,
                    parent_name,
                    SymbolKind::Interface,
                ) {
                    self.extract_type_relationships(
                        node,
                        source,
                        file_path,
                        result,
                        &qualified_name,
                        SymbolKind::Interface,
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
            "enum_declaration" => {
                if let Some((nested_name, qualified_name)) = self.extract_type_declaration(
                    node,
                    source,
                    file_path,
                    result,
                    parent_name,
                    SymbolKind::Enum,
                ) {
                    self.extract_type_relationships(
                        node,
                        source,
                        file_path,
                        result,
                        &qualified_name,
                        SymbolKind::Enum,
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
            "constructor_declaration" => {
                if let Some(qualified_name) = self.extract_constructor_declaration(
                    node,
                    source,
                    file_path,
                    result,
                    parent_name,
                ) {
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
            "field_declaration" | "constant_declaration" => {
                self.extract_field_declaration(node, source, file_path, result, parent_name);
            }
            "import_declaration" => {
                self.extract_import_declaration(node, source, file_path, result);
            }
            "method_invocation" => {
                self.extract_method_invocation(node, source, file_path, result, enclosing_symbol);
            }
            "object_creation_expression" => {
                self.extract_object_creation_expression(
                    node,
                    source,
                    file_path,
                    result,
                    enclosing_symbol,
                );
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
        let name = node_text(&name_node, source)?;
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

    fn extract_method_declaration(
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

        if let Some(signature) = self.method_signature(node, source, &name) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some(qualified_name)
    }

    fn extract_constructor_declaration(
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

        if let Some(signature) = self.constructor_signature(node, source, &name) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
        Some(qualified_name)
    }

    fn extract_field_declaration(
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
        let kind = self.field_symbol_kind(node, source);
        let visibility = self.visibility_for_node(node, source);
        let signature = node_text(node, source);

        for name in self.variable_declarator_names(node, source) {
            let mut symbol = Symbol::new(
                name.clone(),
                make_qualified_name(file_path, &name, Some(parent_name)),
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
        let line = self.node_start(node).0;

        if let Some(superclass) = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "superclass")
            .and_then(|child| self.primary_type_name(&child, source))
        {
            result.dependencies.push(Dependency::new(
                from_symbol.to_string(),
                superclass,
                file_path.to_string(),
                line,
                DependencyKind::Inherit,
            ));
        }

        if let Some(super_interfaces) = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "super_interfaces")
        {
            let dependency_kind = if symbol_kind == SymbolKind::Class {
                DependencyKind::Implement
            } else {
                DependencyKind::Inherit
            };
            let mut names = Vec::new();
            let mut cursor = super_interfaces.walk();
            for child in super_interfaces.named_children(&mut cursor) {
                if let Some(name) = self.primary_type_name(&child, source) {
                    names.push(name);
                }
            }
            names.sort();
            names.dedup();

            for name in names {
                result.dependencies.push(Dependency::new(
                    from_symbol.to_string(),
                    name,
                    file_path.to_string(),
                    line,
                    dependency_kind.clone(),
                ));
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
        let Some(text) = node_text(node, source) else {
            return;
        };
        let trimmed = text
            .trim()
            .trim_end_matches(';')
            .trim()
            .strip_prefix("import ")
            .map(str::trim);
        let Some(trimmed) = trimmed else {
            return;
        };

        let (is_static, import_path) = match trimmed.strip_prefix("static ") {
            Some(path) => (true, path.trim()),
            None => (false, trimmed),
        };
        let line = self.node_start(node).0;

        if let Some(path) = import_path.strip_suffix(".*") {
            result.imports.push(Import::new(
                path.trim().to_string(),
                file_path.to_string(),
                line,
                ImportKind::Glob,
            ));
            return;
        }

        if is_static {
            if let Some((class_path, member_name)) = import_path.rsplit_once('.') {
                result.imports.push(Import::new(
                    format!("{}::{}", class_path.trim(), member_name.trim()),
                    file_path.to_string(),
                    line,
                    ImportKind::Named,
                ));
            }
            return;
        }

        result.imports.push(Import::new(
            import_path.to_string(),
            file_path.to_string(),
            line,
            ImportKind::Named,
        ));
    }

    fn extract_method_invocation(
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
        let Some(target) = node_text(&name_node, source) else {
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

    fn extract_object_creation_expression(
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
        let Some(type_node) = node.child_by_field_name("type") else {
            return;
        };
        let Some(target) = self.primary_type_name(&type_node, source) else {
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

    fn variable_declarator_names(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
    ) -> Vec<String> {
        let mut names = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "variable_declarator" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    if let Some(name) = node_text(&name_node, source) {
                        names.push(name);
                    }
                }
                continue;
            }

            names.extend(self.variable_declarator_names(&child, source));
        }
        names
    }

    fn primary_type_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "type_identifier" | "identifier" => node_text(node, source),
            "scoped_type_identifier" | "scoped_identifier" => node
                .child_by_field_name("name")
                .and_then(|name| node_text(&name, source))
                .or_else(|| node_text(node, source).map(|text| self.last_path_segment(&text))),
            "generic_type" | "array_type" | "annotated_type" => node
                .child_by_field_name("type")
                .or_else(|| node.child_by_field_name("name"))
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

    fn field_symbol_kind(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> SymbolKind {
        if node.kind() == "constant_declaration" {
            return SymbolKind::Constant;
        }

        let text = node_text(node, source).unwrap_or_default();
        if text.contains(" static ") && text.contains(" final ") {
            SymbolKind::Constant
        } else {
            SymbolKind::Property
        }
    }

    fn method_signature(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        name: &str,
    ) -> Option<String> {
        let parameters = node
            .child_by_field_name("parameters")
            .and_then(|parameters| node_text(&parameters, source))?;
        let return_type = node
            .child_by_field_name("type")
            .and_then(|return_type| node_text(&return_type, source))
            .unwrap_or_default();
        Some(format!("{name}{parameters}{return_type}"))
    }

    fn constructor_signature(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        name: &str,
    ) -> Option<String> {
        let parameters = node
            .child_by_field_name("parameters")
            .and_then(|parameters| node_text(&parameters, source))?;
        Some(format!("{name}{parameters}"))
    }

    fn visibility_for_node(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Visibility {
        if self.has_modifier(node, source, "public") {
            Visibility::Public
        } else if self.has_modifier(node, source, "protected") {
            Visibility::Protected
        } else {
            Visibility::Private
        }
    }

    fn has_modifier(&self, node: &tree_sitter::Node<'_>, source: &[u8], modifier: &str) -> bool {
        let Some(modifiers) = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "modifiers")
        else {
            return false;
        };

        let mut cursor = modifiers.walk();
        let has_modifier = modifiers
            .children(&mut cursor)
            .any(|child| node_text(&child, source).as_deref() == Some(modifier));
        has_modifier
    }

    fn last_path_segment(&self, path: &str) -> String {
        path.rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or(path)
            .to_string()
    }

    fn node_start(&self, node: &tree_sitter::Node<'_>) -> (u32, u32) {
        let position = node.start_position();
        (position.row as u32 + 1, position.column as u32 + 1)
    }
}

impl Default for JavaParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for JavaParser {
    fn language(&self) -> &'static str {
        "java"
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
        &["java"]
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_parse_java_fixture() {
        let mut parser = JavaParser::new();
        let fixture_path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/java/sample.java"
        ));
        let content = std::fs::read(&fixture_path).expect("Failed to read fixture");
        let result = parser.parse_file(&fixture_path, &content, "src/sample.java");

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
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "max"));
        assert_eq!(result.imports.len(), 2);
        assert!(result
            .imports
            .iter()
            .any(|import| import.source == "java.lang.Math::max"));
    }

    #[test]
    fn test_parse_java_extends_and_new_expression() {
        let mut parser = JavaParser::new();
        let content = br#"
class Base {}
interface Worker {}

class App extends Base implements Worker {
    App() {}

    void run() {
        Helper helper = new Helper();
        helper.work();
    }
}

class Helper {
    void work() {}
}
"#;

        let result = parser.parse_file(&PathBuf::from("App.java"), content, "src/App.java");

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
