//! C++ parser using tree-sitter-cpp.

use std::path::Path;

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource, Visibility};
use crate::parser::{make_qualified_name, node_text, ParseResult, Parser};
use crate::resolver::{Import, ImportKind};

/// C++ parser using tree-sitter-cpp.
pub struct CppParser {
    parser: tree_sitter::Parser,
}

#[derive(Clone, Copy, Default)]
struct TraversalContext<'a> {
    scope_prefix: Option<&'a str>,
    current_type_scope: Option<&'a str>,
    enclosing_symbol: Option<&'a str>,
}

impl CppParser {
    /// Create a new C++ parser.
    ///
    /// # Panics
    /// Panics if the bundled tree-sitter grammar cannot be loaded.
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_cpp::LANGUAGE.into())
            .expect("Failed to set C++ language for tree-sitter parser");
        Self { parser }
    }

    fn extract_symbols(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        context: TraversalContext<'_>,
    ) {
        if node.is_error() || node.is_missing() {
            result.error_count += 1;
            return;
        }

        match node.kind() {
            "namespace_definition" => {
                if let Some(namespace_scope) =
                    self.extract_namespace(node, source, file_path, result, context.scope_prefix)
                {
                    if let Some(body) = node.child_by_field_name("body") {
                        self.extract_symbols(
                            &body,
                            source,
                            file_path,
                            result,
                            TraversalContext {
                                scope_prefix: Some(&namespace_scope),
                                ..context
                            },
                        );
                    }
                    return;
                }
            }
            "class_specifier" | "struct_specifier" => {
                if let Some(type_scope) =
                    self.extract_type(node, source, file_path, result, context.scope_prefix)
                {
                    self.extract_base_classes(
                        node,
                        source,
                        file_path,
                        result,
                        context.scope_prefix,
                        &type_scope,
                    );
                    if let Some(body) = node.child_by_field_name("body") {
                        self.extract_symbols(
                            &body,
                            source,
                            file_path,
                            result,
                            TraversalContext {
                                scope_prefix: Some(&type_scope),
                                current_type_scope: Some(&type_scope),
                                ..context
                            },
                        );
                    }
                    return;
                }
            }
            "function_definition" => {
                if let Some(qualified_name) = self.extract_function(
                    node,
                    source,
                    file_path,
                    result,
                    context.scope_prefix,
                    context.current_type_scope,
                ) {
                    if let Some(body) = node.child_by_field_name("body") {
                        self.extract_symbols(
                            &body,
                            source,
                            file_path,
                            result,
                            TraversalContext {
                                enclosing_symbol: Some(&qualified_name),
                                ..context
                            },
                        );
                    }
                    return;
                }
            }
            "field_declaration" => {
                if context.current_type_scope.is_some() {
                    self.extract_method_declaration(
                        node,
                        source,
                        file_path,
                        result,
                        context.current_type_scope,
                    );
                } else if self.is_top_level_declaration(node) {
                    self.extract_global_declarations(
                        node,
                        source,
                        file_path,
                        result,
                        context.scope_prefix,
                    );
                }
            }
            "enum_specifier" => {
                self.extract_enum(node, source, file_path, result, context.scope_prefix);
            }
            "type_definition" => {
                self.extract_type_alias(node, source, file_path, result, context.scope_prefix);
            }
            "declaration" => {
                if context.current_type_scope.is_none() && self.is_top_level_declaration(node) {
                    self.extract_global_declarations(
                        node,
                        source,
                        file_path,
                        result,
                        context.scope_prefix,
                    );
                }
            }
            "preproc_include" => {
                self.extract_include(node, source, file_path, result);
            }
            "call_expression" => {
                self.extract_call_expression(
                    node,
                    source,
                    file_path,
                    result,
                    context.enclosing_symbol,
                );
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_symbols(&child, source, file_path, result, context);
        }
    }

    fn extract_namespace(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        scope_prefix: Option<&str>,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(&name_node, source)?;
        let qualified_name = make_qualified_name(file_path, &name, scope_prefix);
        let (line, column) = self.node_start(node);
        self.push_symbol(
            result,
            Symbol::new(
                name.clone(),
                qualified_name,
                SymbolKind::Module,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(Visibility::Public)
            .with_source(SymbolSource::Local),
        );
        Some(self.join_scope(scope_prefix, &name))
    }

    fn extract_type(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        scope_prefix: Option<&str>,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let full_name = node_text(&name_node, source)?;
        let (_, short_name) = self.split_scoped_name(&full_name);
        let qualified_name = make_qualified_name(file_path, short_name, scope_prefix);
        let (line, column) = self.node_start(node);
        let kind = if node.kind() == "class_specifier" {
            SymbolKind::Class
        } else {
            SymbolKind::Struct
        };

        self.push_symbol(
            result,
            Symbol::new(
                short_name.to_string(),
                qualified_name,
                kind,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(self.visibility_for_type(node.kind()))
            .with_source(SymbolSource::Local),
        );

        Some(self.join_scope(scope_prefix, short_name))
    }

    fn extract_base_classes(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        scope_prefix: Option<&str>,
        type_scope: &str,
    ) {
        let type_name = type_scope.rsplit("::").next().unwrap_or(type_scope);
        let from_symbol = make_qualified_name(file_path, type_name, scope_prefix);
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "base_class_clause" {
                continue;
            }

            let mut base_cursor = child.walk();
            for base in child.children(&mut base_cursor) {
                if !matches!(
                    base.kind(),
                    "type_identifier" | "qualified_identifier" | "template_type"
                ) {
                    continue;
                }

                let Some(target) = node_text(&base, source) else {
                    continue;
                };
                let (line, _) = self.node_start(&base);
                result.dependencies.push(Dependency::new(
                    from_symbol.clone(),
                    target,
                    file_path.to_string(),
                    line,
                    DependencyKind::Inherit,
                ));
            }
        }
    }

    fn extract_function(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        scope_prefix: Option<&str>,
        current_type_scope: Option<&str>,
    ) -> Option<String> {
        let declarator = node.child_by_field_name("declarator")?;
        let raw_name = self.declarator_name(&declarator, source)?;
        let explicit_scope = raw_name
            .contains("::")
            .then(|| self.split_scoped_name(&raw_name).0)
            .flatten();
        let short_name = self
            .split_scoped_name(&raw_name)
            .1
            .trim_start_matches('~')
            .to_string();
        let parent_scope = explicit_scope
            .as_deref()
            .or(current_type_scope)
            .or(scope_prefix);
        let qualified_name = make_qualified_name(file_path, &short_name, parent_scope);
        let kind = if current_type_scope.is_some()
            || self.scope_matches_known_type(result, file_path, explicit_scope.as_deref())
        {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };
        let (line, column) = self.node_start(node);

        let mut symbol = Symbol::new(
            short_name.clone(),
            qualified_name.clone(),
            kind,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(self.visibility_for_function(node, source, current_type_scope))
        .with_source(SymbolSource::Local);

        if let Some(signature) = node_text(&declarator, source) {
            symbol = symbol.with_signature(signature);
        }

        self.push_symbol(result, symbol);
        Some(qualified_name)
    }

    fn extract_method_declaration(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        current_type_scope: Option<&str>,
    ) {
        let Some(type_scope) = current_type_scope else {
            return;
        };
        if !self.contains_kind(node, "function_declarator") {
            return;
        }
        let Some(raw_name) = self.declarator_name(node, source) else {
            return;
        };
        let short_name = self
            .split_scoped_name(&raw_name)
            .1
            .trim_start_matches('~')
            .to_string();
        if short_name.is_empty() {
            return;
        }

        let (line, column) = self.node_start(node);
        let mut symbol = Symbol::new(
            short_name.clone(),
            make_qualified_name(file_path, &short_name, Some(type_scope)),
            SymbolKind::Method,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(self.visibility_for_member(node, source))
        .with_source(SymbolSource::Local);
        if let Some(signature) = node_text(node, source) {
            symbol = symbol.with_signature(signature);
        }
        self.push_symbol(result, symbol);
    }

    fn extract_enum(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        scope_prefix: Option<&str>,
    ) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Some(name) = node_text(&name_node, source) else {
            return;
        };
        let (line, column) = self.node_start(node);
        self.push_symbol(
            result,
            Symbol::new(
                name.clone(),
                make_qualified_name(file_path, &name, scope_prefix),
                SymbolKind::Enum,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(Visibility::Public)
            .with_source(SymbolSource::Local),
        );
    }

    fn extract_type_alias(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        scope_prefix: Option<&str>,
    ) {
        let Some(declarator) = node.child_by_field_name("declarator") else {
            return;
        };
        let Some(raw_name) = self.declarator_name(&declarator, source) else {
            return;
        };
        let short_name = self.split_scoped_name(&raw_name).1.to_string();
        let (line, column) = self.node_start(node);
        let mut symbol = Symbol::new(
            short_name.to_string(),
            make_qualified_name(file_path, &short_name, scope_prefix),
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
        self.push_symbol(result, symbol);
    }

    fn extract_global_declarations(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        scope_prefix: Option<&str>,
    ) {
        if self.contains_kind(node, "function_declarator") {
            return;
        }

        let declaration_text = node_text(node, source).unwrap_or_default();
        let kind = if declaration_text.contains("const") {
            SymbolKind::Constant
        } else {
            SymbolKind::Variable
        };

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "init_declarator" && child.kind() != "identifier" {
                continue;
            }

            let Some(name) = self.declarator_name(&child, source) else {
                continue;
            };
            let short_name = self.split_scoped_name(&name).1.to_string();
            let (line, column) = self.node_start(&child);
            let mut symbol = Symbol::new(
                short_name.clone(),
                make_qualified_name(file_path, &short_name, scope_prefix),
                kind.clone(),
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(self.visibility_for_member(node, source))
            .with_source(SymbolSource::Local);
            if let Some(signature) = node_text(&child, source) {
                symbol = symbol.with_signature(signature);
            }
            self.push_symbol(result, symbol);
        }
    }

    fn extract_include(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let Some(path_node) = node.child_by_field_name("path") else {
            return;
        };
        let Some(raw_path) = node_text(&path_node, source) else {
            return;
        };
        let include = raw_path
            .trim()
            .trim_start_matches('"')
            .trim_end_matches('"')
            .trim_start_matches('<')
            .trim_end_matches('>')
            .to_string();
        if include.is_empty() {
            return;
        }
        let (line, _) = self.node_start(node);
        result.imports.push(Import::new(
            include,
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

    fn call_target_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "identifier"
            | "field_identifier"
            | "qualified_identifier"
            | "destructor_name"
            | "type_identifier" => node_text(node, source).map(|name| {
                self.split_scoped_name(&name)
                    .1
                    .trim_start_matches('~')
                    .to_string()
            }),
            "field_expression" => node
                .child_by_field_name("field")
                .and_then(|field| node_text(&field, source)),
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

    fn declarator_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        match node.kind() {
            "identifier"
            | "field_identifier"
            | "qualified_identifier"
            | "destructor_name"
            | "type_identifier" => node_text(node, source),
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if let Some(name) = self.declarator_name(&child, source) {
                        return Some(name);
                    }
                }
                None
            }
        }
    }

    fn scope_matches_known_type(
        &self,
        result: &ParseResult,
        file_path: &str,
        scope: Option<&str>,
    ) -> bool {
        let Some(scope) = scope else {
            return false;
        };
        let scoped_name = format!("{file_path}::{scope}");
        result.symbols.iter().any(|symbol| {
            symbol.qualified_name == scoped_name
                && matches!(symbol.kind, SymbolKind::Class | SymbolKind::Struct)
        })
    }

    fn split_scoped_name<'a>(&self, raw_name: &'a str) -> (Option<String>, &'a str) {
        if let Some((scope, name)) = raw_name.rsplit_once("::") {
            (Some(scope.to_string()), name)
        } else {
            (None, raw_name)
        }
    }

    fn push_symbol(&self, result: &mut ParseResult, symbol: Symbol) {
        if result
            .symbols
            .iter()
            .any(|existing| existing.qualified_name == symbol.qualified_name)
        {
            return;
        }
        result.symbols.push(symbol);
    }

    fn join_scope(&self, scope_prefix: Option<&str>, name: &str) -> String {
        match scope_prefix {
            Some(scope) if !scope.is_empty() => format!("{scope}::{name}"),
            _ => name.to_string(),
        }
    }

    fn visibility_for_type(&self, kind: &str) -> Visibility {
        if kind == "class_specifier" {
            Visibility::Private
        } else {
            Visibility::Public
        }
    }

    fn visibility_for_function(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        current_type_scope: Option<&str>,
    ) -> Visibility {
        if current_type_scope.is_some() {
            self.visibility_for_member(node, source)
        } else if node_text(node, source)
            .unwrap_or_default()
            .contains("static")
        {
            Visibility::Private
        } else {
            Visibility::Public
        }
    }

    fn visibility_for_member(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Visibility {
        let text = node_text(node, source).unwrap_or_default();
        if text.contains("private:") || text.contains("private ") {
            Visibility::Private
        } else if text.contains("protected:") || text.contains("protected ") {
            Visibility::Protected
        } else {
            Visibility::Public
        }
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

    fn is_top_level_declaration(&self, node: &tree_sitter::Node<'_>) -> bool {
        matches!(
            node.parent().map(|parent| parent.kind()),
            Some("translation_unit" | "declaration_list" | "field_declaration_list")
        )
    }

    fn node_start(&self, node: &tree_sitter::Node<'_>) -> (u32, u32) {
        let position = node.start_position();
        ((position.row + 1) as u32, (position.column + 1) as u32)
    }
}

impl Default for CppParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for CppParser {
    fn language(&self) -> &'static str {
        "cpp"
    }

    fn parse_file(&mut self, _path: &Path, content: &[u8], file_path: &str) -> ParseResult {
        let Some(tree) = self.parser.parse(content, None) else {
            return ParseResult::default();
        };
        let mut result = ParseResult::default();
        self.extract_symbols(
            &tree.root_node(),
            content,
            file_path,
            &mut result,
            TraversalContext::default(),
        );
        if tree.root_node().has_error() && result.error_count == 0 {
            result.error_count = 1;
        }
        result
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["cc", "cpp", "cxx", "hh", "hpp", "hxx"]
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_parse_cpp_symbols_inheritance_and_calls() {
        let content = br#"
#include "shared.hpp"
#include <vector>

namespace app {

class Base {};

class UserService : public Base {
public:
    UserService() = default;
    ~UserService() = default;
    int run() { return helper(); }
};

}

int helper() { return 1; }

int app::UserService::build() {
    return helper();
}
"#;
        let mut parser = CppParser::new();
        let result = parser.parse_file(&PathBuf::from("sample.cpp"), content, "src/sample.cpp");

        assert_eq!(result.error_count, 0);
        assert!(result.symbols.iter().any(|symbol| symbol.name == "app"));
        assert!(result.symbols.iter().any(|symbol| symbol.name == "Base"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.name == "UserService"));
        assert!(result.symbols.iter().any(|symbol| symbol.name == "run"));
        assert!(result.symbols.iter().any(|symbol| symbol.name == "build"));
        assert_eq!(result.imports.len(), 2);
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Inherit
                && dependency.to_symbol == "Base"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "helper"));
    }
}
