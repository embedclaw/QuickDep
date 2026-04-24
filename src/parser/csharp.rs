//! C# parser using tree-sitter-c-sharp.

use std::path::Path;

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource, Visibility};
use crate::parser::{make_qualified_name, node_text, ParseResult, Parser};
use crate::resolver::{Import, ImportKind};

/// C# parser using tree-sitter-c-sharp.
pub struct CSharpParser {
    parser: tree_sitter::Parser,
}

impl CSharpParser {
    /// Create a new C# parser.
    ///
    /// # Panics
    /// Panics if the bundled tree-sitter grammar cannot be loaded.
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
            .expect("Failed to set C# language for tree-sitter parser");
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
                    self.extract_base_list(node, source, file_path, result, &qualified_name, true);
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
                    self.extract_base_list(node, source, file_path, result, &qualified_name, false);
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
            "struct_declaration" => {
                if let Some((nested_name, qualified_name)) = self.extract_type_declaration(
                    node,
                    source,
                    file_path,
                    result,
                    parent_name,
                    SymbolKind::Struct,
                ) {
                    self.extract_base_list(node, source, file_path, result, &qualified_name, true);
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
            "property_declaration" => {
                self.extract_property_declaration(node, source, file_path, result, parent_name);
            }
            "field_declaration" => {
                self.extract_field_declaration(node, source, file_path, result, parent_name);
            }
            "using_directive" => {
                self.extract_using_directive(node, source, file_path, result);
            }
            "invocation_expression" => {
                self.extract_invocation_expression(
                    node,
                    source,
                    file_path,
                    result,
                    enclosing_symbol,
                );
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
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Some(name) = node_text(&name_node, source) else {
            return;
        };
        let (line, column) = self.node_start(node);

        let mut symbol = Symbol::new(
            name.clone(),
            make_qualified_name(file_path, &name, Some(parent_name)),
            SymbolKind::Property,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(self.visibility_for_node(node, source))
        .with_source(SymbolSource::Local);

        if let Some(signature) = node_text(node, source) {
            symbol = symbol.with_signature(signature);
        }

        result.symbols.push(symbol);
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
        let visibility = self.visibility_for_node(node, source);
        let kind = self.field_symbol_kind(node, source);
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

    fn extract_base_list(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        from_symbol: &str,
        first_base_is_inherit: bool,
    ) {
        let Some(base_list) = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "base_list")
        else {
            return;
        };

        let mut index = 0_usize;
        let mut cursor = base_list.walk();
        for child in base_list.named_children(&mut cursor) {
            let Some(name) = self.primary_type_name(&child, source) else {
                continue;
            };
            let kind = if first_base_is_inherit && index == 0 && !looks_like_csharp_interface(&name)
            {
                DependencyKind::Inherit
            } else {
                DependencyKind::Implement
            };
            index += 1;

            result.dependencies.push(Dependency::new(
                from_symbol.to_string(),
                name,
                file_path.to_string(),
                self.node_start(node).0,
                kind,
            ));
        }
    }

    fn extract_using_directive(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let Some(text) = node_text(node, source) else {
            return;
        };
        let Some(mut content) = text
            .trim()
            .trim_end_matches(';')
            .trim()
            .strip_prefix("using ")
            .map(str::trim)
        else {
            return;
        };
        let line = self.node_start(node).0;

        if let Some(static_source) = content.strip_prefix("static ") {
            result.imports.push(Import::new(
                static_source.trim().to_string(),
                file_path.to_string(),
                line,
                ImportKind::Glob,
            ));
            return;
        }

        if let Some((alias, source)) = content.split_once('=') {
            let alias = alias.trim();
            content = source.trim();
            if !alias.is_empty() && !content.is_empty() {
                result.imports.push(
                    Import::new(
                        content.to_string(),
                        file_path.to_string(),
                        line,
                        ImportKind::Alias,
                    )
                    .with_alias(alias.to_string()),
                );
            }
            return;
        }

        if !content.is_empty() {
            result.imports.push(Import::new(
                content.to_string(),
                file_path.to_string(),
                line,
                ImportKind::Glob,
            ));
        }
    }

    fn extract_invocation_expression(
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
        let Some(target) = self.call_target_name(&function_node, source) else {
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
        let type_node = node
            .child_by_field_name("type")
            .or_else(|| node.child_by_field_name("name"))
            .or_else(|| node.named_child(0));
        let Some(type_node) = type_node else {
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

    fn call_target_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "identifier" => node_text(node, source),
            "member_access_expression" => node
                .child_by_field_name("name")
                .and_then(|name| node_text(&name, source))
                .or_else(|| node_text(node, source).map(|text| self.last_path_segment(&text))),
            "generic_name" => node
                .child_by_field_name("name")
                .and_then(|name| node_text(&name, source))
                .or_else(|| node_text(node, source).map(|text| self.last_path_segment(&text))),
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

    fn variable_declarator_names(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
    ) -> Vec<String> {
        let mut names = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "variable_declarator" {
                if let Some(name_node) = child
                    .child_by_field_name("name")
                    .or_else(|| child.named_child(0))
                {
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
            "identifier" | "predefined_type" => node_text(node, source),
            "qualified_name" | "alias_qualified_name" => {
                node_text(node, source).map(|text| self.last_path_segment(&text))
            }
            "generic_name" => node
                .child_by_field_name("name")
                .and_then(|name| node_text(&name, source))
                .or_else(|| node_text(node, source).map(|text| self.last_path_segment(&text))),
            "nullable_type" | "array_type" => node
                .child_by_field_name("type")
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
        let text = node_text(node, source).unwrap_or_default();
        if text.contains(" const ") || text.trim_start().starts_with("const ") {
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
            .child_by_field_name("returns")
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
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "modifier" {
                continue;
            }
            if node_text(&child, source).as_deref() == Some(modifier) {
                return true;
            }
        }
        false
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

fn looks_like_csharp_interface(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('I')) && matches!(chars.next(), Some(next) if next.is_uppercase())
}

impl Default for CSharpParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for CSharpParser {
    fn language(&self) -> &'static str {
        "csharp"
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
        &["cs"]
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_parse_csharp_fixture() {
        let mut parser = CSharpParser::new();
        let fixture_path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/csharp/Sample.cs"
        ));
        let content = std::fs::read(&fixture_path).expect("Failed to read fixture");
        let result = parser.parse_file(&fixture_path, &content, "src/Sample.cs");

        assert_eq!(result.error_count, 0);
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Interface && symbol.name == "IGreeter"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Class && symbol.name == "UserService"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Method && symbol.name == "Greet"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Implement
                && dependency.to_symbol == "IGreeter"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "Format"));
        assert_eq!(result.imports.len(), 2);
    }

    #[test]
    fn test_parse_csharp_extends_and_new_expression() {
        let mut parser = CSharpParser::new();
        let content = br#"
class Base {}
interface IWorker {}

class App : Base, IWorker {
    public App() {}

    public void Run() {
        var helper = new Helper();
        helper.Work();
    }
}

class Helper {
    public void Work() {}
}
"#;

        let result = parser.parse_file(&PathBuf::from("App.cs"), content, "src/App.cs");

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
                && dependency.to_symbol == "IWorker"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::TypeUse
                && dependency.to_symbol == "Helper"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "Work"));
    }
}
