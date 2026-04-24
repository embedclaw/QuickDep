//! Rust parser using tree-sitter-rust.
//!
//! Extracts symbols (functions, structs, enums, traits, macros) and
//! dependencies (function calls, references) from Rust source files.

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource, Visibility};
use crate::parser::{make_qualified_name, ParseResult, Parser};
use crate::resolver::{Import, ImportKind};
use std::path::Path;

/// Rust parser using tree-sitter-rust.
pub struct RustParser {
    parser: tree_sitter::Parser,
}

impl RustParser {
    /// Create a new Rust parser.
    ///
    /// # Panics
    /// Panics if tree-sitter-rust language cannot be loaded.
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        let language = tree_sitter_rust::LANGUAGE.into();
        parser
            .set_language(&language)
            .expect("Failed to set Rust language for tree-sitter parser");
        Self { parser }
    }

    /// Extract symbols from a tree-sitter node.
    fn extract_symbols(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
        enclosing_symbol: Option<&str>,
    ) {
        // Track error nodes
        if node.is_error() || node.is_missing() {
            result.error_count += 1;
            return;
        }

        match node.kind() {
            // Function items (top-level functions)
            "function_item" => {
                if let Some(function_qualified_name) =
                    self.extract_function(node, source, file_path, result, parent_name)
                {
                    // Recurse into function body with new enclosing function
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "block" {
                            self.extract_symbols(
                                &child,
                                source,
                                file_path,
                                result,
                                parent_name,
                                Some(&function_qualified_name),
                            );
                        }
                    }
                    return; // Don't recurse into this node again
                }
            }

            // Struct items
            "struct_item" => {
                self.extract_struct(node, source, file_path, result);
            }

            // Enum items
            "enum_item" => {
                self.extract_enum(node, source, file_path, result);
            }

            // Trait items
            "trait_item" => {
                self.extract_trait(node, source, file_path, result);
            }

            // Type alias items
            "type_item" => {
                self.extract_type_alias(node, source, file_path, result);
            }

            // Module items
            "mod_item" => {
                self.extract_module(node, source, file_path, result);
            }

            // Macro definitions
            "macro_definition" => {
                self.extract_macro(node, source, file_path, result);
            }

            // Impl items - contains methods
            "impl_item" => {
                self.extract_impl(node, source, file_path, result);
                return; // Already handled children
            }

            // Constant items
            "const_item" => {
                self.extract_const(node, source, file_path, result);
            }

            // Static items
            "static_item" => {
                self.extract_static(node, source, file_path, result);
            }

            // Use declarations (imports)
            "use_declaration" => {
                self.extract_use_declaration(node, source, file_path, result);
            }

            // Call expressions (dependencies)
            "call_expression" => {
                self.extract_call_expression(node, source, file_path, result, enclosing_symbol);
            }

            // Macro invocations (also considered calls)
            "macro_invocation" => {
                self.extract_macro_invocation(node, source, file_path, result, enclosing_symbol);
            }

            _ => {}
        }

        // Recursively process children
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

    /// Extract a function definition.
    fn extract_function(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
    ) -> Option<String> {
        let name = self.get_identifier_name(node, source)?;

        let kind = if parent_name.is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        let qualified_name = make_qualified_name(file_path, &name, parent_name);
        let visibility = self.get_visibility(node, source);
        let signature = self.get_function_signature(node, source);
        let (line, column) = self.node_start(node);

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

        if let Some(sig) = signature {
            symbol = symbol.with_signature(sig);
        }

        result.symbols.push(symbol);
        Some(qualified_name)
    }

    /// Extract a struct definition.
    fn extract_struct(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        if let Some(name) = self.get_type_identifier_name(node, source) {
            let qualified_name = make_qualified_name(file_path, &name, None);
            let visibility = self.get_visibility(node, source);
            let (line, column) = self.node_start(node);

            let symbol = Symbol::new(
                name.clone(),
                qualified_name,
                SymbolKind::Struct,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(visibility)
            .with_source(SymbolSource::Local);

            result.symbols.push(symbol);
            self.extract_struct_fields(node, source, file_path, &name, result);
        }
    }

    /// Extract named struct fields as property symbols with type signatures.
    fn extract_struct_fields(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        struct_name: &str,
        result: &mut ParseResult,
    ) {
        let Some(body) = node.child_by_field_name("body") else {
            return;
        };

        if body.kind() != "field_declaration_list" {
            return;
        }

        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "field_declaration" {
                self.extract_struct_field(&child, source, file_path, struct_name, result);
            }
        }
    }

    /// Extract a single named struct field.
    fn extract_struct_field(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        struct_name: &str,
        result: &mut ParseResult,
    ) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let Some(name) = self.get_node_text(&name_node, source) else {
            return;
        };

        let field_type = node
            .child_by_field_name("type")
            .and_then(|field_type| self.get_node_text(&field_type, source));
        let (line, column) = self.node_start(node);

        let mut symbol = Symbol::new(
            name.clone(),
            make_qualified_name(file_path, &name, Some(struct_name)),
            SymbolKind::Property,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(self.get_visibility(node, source))
        .with_source(SymbolSource::Local);

        if let Some(field_type) = field_type {
            symbol = symbol.with_signature(field_type);
        }

        result.symbols.push(symbol);
    }

    /// Extract an enum definition and its variants.
    fn extract_enum(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        if let Some(name) = self.get_type_identifier_name(node, source) {
            let qualified_name = make_qualified_name(file_path, &name, None);
            let visibility = self.get_visibility(node, source);
            let (line, column) = self.node_start(node);

            let symbol = Symbol::new(
                name.clone(),
                qualified_name,
                SymbolKind::Enum,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(visibility)
            .with_source(SymbolSource::Local);

            result.symbols.push(symbol);

            // Extract enum variants
            self.extract_enum_variants(node, source, file_path, &name, result);
        }
    }

    /// Extract enum variants.
    fn extract_enum_variants(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        enum_name: &str,
        result: &mut ParseResult,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "enum_variant_list" {
                let mut variant_cursor = child.walk();
                for variant in child.children(&mut variant_cursor) {
                    if variant.kind() == "enum_variant" {
                        if let Some(vname) = self.get_node_identifier(&variant, source) {
                            let qualified_name =
                                make_qualified_name(file_path, &vname, Some(enum_name));
                            let (line, column) = self.node_start(&variant);
                            let symbol = Symbol::new(
                                vname.clone(),
                                qualified_name,
                                SymbolKind::EnumVariant,
                                file_path.to_string(),
                                line,
                                column,
                            )
                            .with_visibility(Visibility::Public)
                            .with_source(SymbolSource::Local);

                            result.symbols.push(symbol);
                        }
                    }
                }
            }
        }
    }

    /// Extract a trait definition.
    fn extract_trait(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        if let Some(name) = self.get_type_identifier_name(node, source) {
            let qualified_name = make_qualified_name(file_path, &name, None);
            let visibility = self.get_visibility(node, source);
            let (line, column) = self.node_start(node);

            let symbol = Symbol::new(
                name.clone(),
                qualified_name,
                SymbolKind::Trait,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(visibility)
            .with_source(SymbolSource::Local);

            result.symbols.push(symbol);
        }
    }

    /// Extract a type alias.
    fn extract_type_alias(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        if let Some(name) = self.get_type_identifier_name(node, source) {
            let qualified_name = make_qualified_name(file_path, &name, None);
            let visibility = self.get_visibility(node, source);
            let (line, column) = self.node_start(node);

            let symbol = Symbol::new(
                name.clone(),
                qualified_name,
                SymbolKind::TypeAlias,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(visibility)
            .with_source(SymbolSource::Local);

            result.symbols.push(symbol);
        }
    }

    /// Extract a module definition.
    fn extract_module(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        if let Some(name) = self.get_identifier_name(node, source) {
            let qualified_name = make_qualified_name(file_path, &name, None);
            let visibility = self.get_visibility(node, source);
            let (line, column) = self.node_start(node);

            let symbol = Symbol::new(
                name.clone(),
                qualified_name,
                SymbolKind::Module,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(visibility)
            .with_source(SymbolSource::Local);

            result.symbols.push(symbol);
        }
    }

    /// Extract a macro definition.
    fn extract_macro(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        if let Some(name) = self.get_identifier_name(node, source) {
            let qualified_name = make_qualified_name(file_path, &name, None);
            let visibility = self.get_visibility(node, source);
            let (line, column) = self.node_start(node);

            let symbol = Symbol::new(
                name.clone(),
                qualified_name,
                SymbolKind::Macro,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(visibility)
            .with_source(SymbolSource::Local);

            result.symbols.push(symbol);
        }
    }

    /// Extract an impl block and its methods.
    fn extract_impl(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        // Get the type being implemented
        let impl_type = self.get_impl_type(node, source);
        if impl_type.is_none() {
            return;
        }
        let impl_type = impl_type.unwrap();

        // Extract methods from the impl block
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "declaration_list" || child.kind() == "block" {
                let mut method_cursor = child.walk();
                for method in child.children(&mut method_cursor) {
                    if method.kind() == "function_item" {
                        self.extract_function(&method, source, file_path, result, Some(&impl_type));

                        // Recurse into method body
                        let mut func_cursor = method.walk();
                        for func_child in method.children(&mut func_cursor) {
                            if func_child.kind() == "block" {
                                let method_name = self
                                    .get_identifier_name(&method, source)
                                    .unwrap_or_default();
                                let method_qualified_name =
                                    make_qualified_name(file_path, &method_name, Some(&impl_type));
                                self.extract_symbols(
                                    &func_child,
                                    source,
                                    file_path,
                                    result,
                                    Some(&impl_type),
                                    Some(&method_qualified_name),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Extract a constant definition.
    fn extract_const(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        if let Some(name) = self.get_identifier_name(node, source) {
            let qualified_name = make_qualified_name(file_path, &name, None);
            let visibility = self.get_visibility(node, source);
            let (line, column) = self.node_start(node);

            let symbol = Symbol::new(
                name.clone(),
                qualified_name,
                SymbolKind::Constant,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(visibility)
            .with_source(SymbolSource::Local);

            result.symbols.push(symbol);
        }
    }

    /// Extract a static definition.
    fn extract_static(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        if let Some(name) = self.get_identifier_name(node, source) {
            let qualified_name = make_qualified_name(file_path, &name, None);
            let visibility = self.get_visibility(node, source);
            let (line, column) = self.node_start(node);

            let symbol = Symbol::new(
                name.clone(),
                qualified_name,
                SymbolKind::Constant,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(visibility)
            .with_source(SymbolSource::Local);

            result.symbols.push(symbol);
        }
    }

    /// Extract a use declaration (import).
    fn extract_use_declaration(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        // Get the full use statement text
        let use_text = self.get_node_text(node, source);
        if use_text.is_none() {
            return;
        }

        // Parse the use declaration using the import parser logic
        let line = Self::one_based_index(node.start_position().row);
        let imports = self.parse_use_declaration(&use_text.unwrap(), file_path, line);
        result.imports.extend(imports);
    }

    /// Parse a use declaration string.
    fn parse_use_declaration(&self, use_text: &str, file_path: &str, line: u32) -> Vec<Import> {
        let trimmed = use_text.trim();

        // Must start with "use "
        if !trimmed.starts_with("use ") {
            return Vec::new();
        }

        // Remove "use " prefix and trailing semicolon
        let use_content = trimmed
            .strip_prefix("use ")
            .unwrap_or("")
            .trim()
            .trim_end_matches(';');

        if use_content.is_empty() {
            return Vec::new();
        }

        // Check for grouped imports: `use path::{Item1, Item2}`
        if use_content.ends_with('}') {
            return self.parse_grouped_imports(use_content, file_path, line);
        }

        // Check for alias: `use path::Item as Alias`
        if let Some(import) = self.parse_alias_import(use_content, file_path, line) {
            return vec![import];
        }

        // Check for glob: `use path::*`
        if use_content.ends_with("::*") {
            let source = use_content.trim_end_matches("::*");
            return vec![Import::new(
                source.to_string(),
                file_path.to_string(),
                line,
                ImportKind::Glob,
            )];
        }

        // Check for self: `use path::self`
        if use_content.ends_with("::self") {
            let source = use_content.trim_end_matches("::self");
            return vec![Import::new(
                source.to_string(),
                file_path.to_string(),
                line,
                ImportKind::SelfImport,
            )];
        }

        // Simple named import
        vec![Import::new(
            use_content.to_string(),
            file_path.to_string(),
            line,
            ImportKind::Named,
        )]
    }

    /// Parse grouped imports.
    fn parse_grouped_imports(&self, use_content: &str, file_path: &str, line: u32) -> Vec<Import> {
        let mut imports = Vec::new();

        let brace_pos = use_content.find('{');
        if brace_pos.is_none() {
            return imports;
        }

        let brace_pos = brace_pos.unwrap();
        let base_path = use_content[..brace_pos].trim();
        let base_path = if let Some(stripped) = base_path.strip_suffix("::") {
            stripped
        } else {
            base_path
        };

        let group_start = brace_pos + 1;
        let group_end = use_content.len() - 1;
        if group_start >= group_end {
            return imports;
        }

        let group_content = &use_content[group_start..group_end];

        for item in group_content.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }

            // Check for alias in item
            if let Some(alias_pos) = item.find(" as ") {
                let item_name = item[..alias_pos].trim();
                let alias_name = item[alias_pos + 4..].trim();

                if item_name == "*" {
                    imports.push(Import::new(
                        base_path.to_string(),
                        file_path.to_string(),
                        line,
                        ImportKind::Glob,
                    ));
                } else if item_name == "self" {
                    imports.push(Import::new(
                        base_path.to_string(),
                        file_path.to_string(),
                        line,
                        ImportKind::SelfImport,
                    ));
                } else {
                    let full_path = if base_path.is_empty() {
                        item_name.to_string()
                    } else {
                        format!("{}::{}", base_path, item_name)
                    };
                    imports.push(
                        Import::new(full_path, file_path.to_string(), line, ImportKind::Named)
                            .with_alias(alias_name.to_string()),
                    );
                }
            } else {
                if item == "*" {
                    imports.push(Import::new(
                        base_path.to_string(),
                        file_path.to_string(),
                        line,
                        ImportKind::Glob,
                    ));
                } else if item == "self" {
                    imports.push(Import::new(
                        base_path.to_string(),
                        file_path.to_string(),
                        line,
                        ImportKind::SelfImport,
                    ));
                } else {
                    let full_path = if base_path.is_empty() {
                        item.to_string()
                    } else {
                        format!("{}::{}", base_path, item)
                    };
                    imports.push(Import::new(
                        full_path,
                        file_path.to_string(),
                        line,
                        ImportKind::Named,
                    ));
                }
            }
        }

        imports
    }

    /// Parse alias import.
    fn parse_alias_import(&self, use_content: &str, file_path: &str, line: u32) -> Option<Import> {
        if let Some(alias_pos) = use_content.find(" as ") {
            let source = use_content[..alias_pos].trim();
            let alias = use_content[alias_pos + 4..].trim();

            if !source.is_empty() && !alias.is_empty() {
                return Some(
                    Import::new(
                        source.to_string(),
                        file_path.to_string(),
                        line,
                        ImportKind::Alias,
                    )
                    .with_alias(alias.to_string()),
                );
            }
        }
        None
    }

    /// Extract a call expression (function call dependency).
    fn extract_call_expression(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        enclosing_symbol: Option<&str>,
    ) {
        // Get the function being called
        let call_target = self.get_call_target(node, source);
        if call_target.is_none() {
            return;
        }
        let call_target = call_target.unwrap();

        // Only record dependency if we have an enclosing function
        if let Some(from_qualified) = enclosing_symbol {
            let dependency = Dependency::new(
                from_qualified.to_string(),
                call_target,
                file_path.to_string(),
                Self::one_based_index(node.start_position().row),
                DependencyKind::Call,
            );

            result.dependencies.push(dependency);
        }
    }

    /// Extract a macro invocation.
    fn extract_macro_invocation(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        enclosing_symbol: Option<&str>,
    ) {
        // Get the macro name
        if let Some(name) = self.get_macro_name(node, source) {
            // Only record dependency if we have an enclosing function
            if let Some(from_qualified) = enclosing_symbol {
                let dependency = Dependency::new(
                    from_qualified.to_string(),
                    format!("{}!", name), // Macro calls end with !
                    file_path.to_string(),
                    Self::one_based_index(node.start_position().row),
                    DependencyKind::Call,
                );

                result.dependencies.push(dependency);
            }
        }
    }

    // Helper methods

    /// Get the name from an identifier node.
    fn get_identifier_name(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return self.get_node_text(&child, source);
            }
        }
        None
    }

    /// Get the name from a type_identifier node.
    fn get_type_identifier_name(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_identifier" {
                return self.get_node_text(&child, source);
            }
        }
        None
    }

    /// Get identifier from any node.
    fn get_node_identifier(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return self.get_node_text(&child, source);
            }
        }
        None
    }

    /// Get the text content of a node.
    fn get_node_text(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        node.utf8_text(source).ok().map(|s| s.to_string())
    }

    /// Get the visibility modifier.
    fn get_visibility(&self, node: &tree_sitter::Node, source: &[u8]) -> Visibility {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "visibility_modifier" {
                if let Some(text) = self.get_node_text(&child, source) {
                    if text == "pub" {
                        return Visibility::Public;
                    }
                    // pub(crate), pub(super), pub(path) are crate-visible
                    return Visibility::Protected;
                }
            }
        }
        Visibility::Private
    }

    /// Get function signature (parameters and return type).
    fn get_function_signature(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let mut cursor = node.walk();
        let mut params_text: Option<String> = None;
        let mut return_text: Option<String> = None;

        for child in node.children(&mut cursor) {
            if child.kind() == "parameters" {
                params_text = self.get_node_text(&child, source);
            }
            if child.kind() == "return_type" {
                return_text = self.get_node_text(&child, source);
            }
        }

        match (params_text, return_text) {
            (Some(params), Some(ret)) => Some(format!("{} {}", params, ret)),
            (Some(params), None) => Some(params),
            (None, Some(ret)) => Some(ret),
            (None, None) => None,
        }
    }

    /// Get the type being implemented in an impl block.
    fn get_impl_type(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_identifier" {
                return self.get_node_text(&child, source);
            }
        }
        None
    }

    /// Get the call target from a call expression.
    fn get_call_target(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            // Direct function call: foo()
            if child.kind() == "identifier" {
                return self.get_node_text(&child, source);
            }
            // Method call: obj.method() or Type::method()
            if child.kind() == "field_expression" || child.kind() == "scoped_identifier" {
                return self.get_node_text(&child, source);
            }
        }
        None
    }

    /// Get macro name from a macro invocation.
    fn get_macro_name(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return self.get_node_text(&child, source);
            }
            if child.kind() == "scoped_identifier" {
                return self.get_node_text(&child, source);
            }
        }
        None
    }

    /// Convert a tree-sitter node start position into one-based line/column values.
    fn node_start(&self, node: &tree_sitter::Node) -> (u32, u32) {
        let position = node.start_position();
        (
            Self::one_based_index(position.row),
            Self::one_based_index(position.column),
        )
    }

    /// Convert a zero-based tree-sitter index into a one-based coordinate.
    fn one_based_index(value: usize) -> u32 {
        u32::try_from(value)
            .map(|value| value.saturating_add(1))
            .unwrap_or(u32::MAX)
    }
}

impl Default for RustParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for RustParser {
    fn language(&self) -> &'static str {
        "rust"
    }

    fn parse_file(&mut self, _path: &Path, content: &[u8], file_path: &str) -> ParseResult {
        let tree = self.parser.parse(content, None);

        if tree.is_none() {
            return ParseResult {
                symbols: Vec::new(),
                dependencies: Vec::new(),
                imports: Vec::new(),
                error_count: 1, // Failed to parse
            };
        }

        let tree = tree.unwrap();
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
        &["rs"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_simple_function() {
        let mut parser = RustParser::new();
        let content = b"fn hello() { println!(\"Hello\"); }";
        let result = parser.parse_file(&PathBuf::from("test.rs"), content, "test.rs");

        assert_eq!(result.error_count, 0);
        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].name, "hello");
        assert_eq!(result.symbols[0].kind, SymbolKind::Function);
    }

    #[test]
    fn test_parse_struct() {
        let mut parser = RustParser::new();
        let content = b"struct User { name: String, age: u32 }";
        let result = parser.parse_file(&PathBuf::from("test.rs"), content, "test.rs");

        assert_eq!(result.error_count, 0);
        let user = result.symbols.iter().find(|symbol| symbol.name == "User");
        assert!(user.is_some());
        assert_eq!(user.unwrap().kind, SymbolKind::Struct);

        let name = result.symbols.iter().find(|symbol| symbol.name == "name");
        assert!(name.is_some());
        assert_eq!(name.unwrap().kind, SymbolKind::Property);

        let age = result.symbols.iter().find(|symbol| symbol.name == "age");
        assert!(age.is_some());
        assert_eq!(age.unwrap().kind, SymbolKind::Property);
    }

    #[test]
    fn test_parse_enum() {
        let mut parser = RustParser::new();
        let content = b"enum Status { Active, Inactive }";
        let result = parser.parse_file(&PathBuf::from("test.rs"), content, "test.rs");

        assert_eq!(result.error_count, 0);
        assert!(!result.symbols.is_empty());

        let enum_symbol = result.symbols.iter().find(|s| s.name == "Status");
        assert!(enum_symbol.is_some());
        assert_eq!(enum_symbol.unwrap().kind, SymbolKind::Enum);

        // Check variants
        let active = result.symbols.iter().find(|s| s.name == "Active");
        assert!(active.is_some());
        assert_eq!(active.unwrap().kind, SymbolKind::EnumVariant);
    }

    #[test]
    fn test_parse_use_declaration() {
        let mut parser = RustParser::new();
        let content = b"use std::collections::HashMap;";
        let result = parser.parse_file(&PathBuf::from("test.rs"), content, "test.rs");

        assert_eq!(result.error_count, 0);
        assert!(!result.imports.is_empty());
    }

    #[test]
    fn test_parse_impl_with_methods() {
        let mut parser = RustParser::new();
        let content =
            b"impl User { fn new(name: &str) -> Self { Self { name: name.to_string() } } }";
        let result = parser.parse_file(&PathBuf::from("test.rs"), content, "test.rs");

        assert_eq!(result.error_count, 0);
        // Should have the method
        let method = result.symbols.iter().find(|s| s.name == "new");
        assert!(method.is_some());
        assert_eq!(method.unwrap().kind, SymbolKind::Method);
    }

    #[test]
    fn test_visibility() {
        let mut parser = RustParser::new();
        let content = b"pub fn public() {} fn private() {}";
        let result = parser.parse_file(&PathBuf::from("test.rs"), content, "test.rs");

        assert_eq!(result.symbols.len(), 2);

        let pub_func = result.symbols.iter().find(|s| s.name == "public");
        assert!(pub_func.is_some());
        assert_eq!(pub_func.unwrap().visibility, Visibility::Public);

        let priv_func = result.symbols.iter().find(|s| s.name == "private");
        assert!(priv_func.is_some());
        assert_eq!(priv_func.unwrap().visibility, Visibility::Private);
    }

    #[test]
    fn test_qualified_name_format() {
        let mut parser = RustParser::new();
        let content = b"fn helper() {}";
        let result = parser.parse_file(&PathBuf::from("src/utils.rs"), content, "src/utils.rs");

        assert_eq!(result.symbols.len(), 1);
        assert_eq!(result.symbols[0].qualified_name, "src/utils.rs::helper");
    }

    #[test]
    fn test_error_tolerance() {
        let mut parser = RustParser::new();
        // Invalid Rust syntax
        let content = b"fn broken( { }";
        let result = parser.parse_file(&PathBuf::from("test.rs"), content, "test.rs");

        // Should still return a result, with error count > 0
        assert!(result.error_count > 0);
    }

    #[test]
    fn test_fixture_main_rs() {
        let fixture_content = include_str!("../../tests/fixtures/rust/src/main.rs");
        let mut parser = RustParser::new();
        let result = parser.parse_file(
            &PathBuf::from("tests/fixtures/rust/src/main.rs"),
            fixture_content.as_bytes(),
            "src/main.rs",
        );

        // Should find main function
        let main_func = result.symbols.iter().find(|s| s.name == "main");
        assert!(main_func.is_some());
        assert_eq!(main_func.unwrap().kind, SymbolKind::Function);
    }

    #[test]
    fn test_fixture_models_rs() {
        let fixture_content = include_str!("../../tests/fixtures/rust/src/models.rs");
        let mut parser = RustParser::new();
        let result = parser.parse_file(
            &PathBuf::from("tests/fixtures/rust/src/models.rs"),
            fixture_content.as_bytes(),
            "src/models.rs",
        );

        // Should find User struct
        let user_struct = result.symbols.iter().find(|s| s.name == "User");
        assert!(user_struct.is_some());
        assert_eq!(user_struct.unwrap().kind, SymbolKind::Struct);

        // Should find new method
        let new_method = result.symbols.iter().find(|s| s.name == "new");
        assert!(new_method.is_some());
        assert_eq!(new_method.unwrap().kind, SymbolKind::Method);

        // Should find Status enum
        let status_enum = result.symbols.iter().find(|s| s.name == "Status");
        assert!(status_enum.is_some());
        assert_eq!(status_enum.unwrap().kind, SymbolKind::Enum);
    }

    #[test]
    fn test_fixture_utils_rs() {
        let fixture_content = include_str!("../../tests/fixtures/rust/src/utils.rs");
        let mut parser = RustParser::new();
        let result = parser.parse_file(
            &PathBuf::from("tests/fixtures/rust/src/utils.rs"),
            fixture_content.as_bytes(),
            "src/utils.rs",
        );

        // Should find calculate function (pub)
        let calc_func = result.symbols.iter().find(|s| s.name == "calculate");
        assert!(calc_func.is_some());
        assert_eq!(calc_func.unwrap().kind, SymbolKind::Function);
        assert_eq!(calc_func.unwrap().visibility, Visibility::Public);

        // Should find helper function (private)
        let helper_func = result.symbols.iter().find(|s| s.name == "helper");
        assert!(helper_func.is_some());
        assert_eq!(helper_func.unwrap().visibility, Visibility::Private);
    }

    #[test]
    fn test_method_dependencies_keep_parent_qualified_name() {
        let content = br#"
        struct User;

        impl User {
            fn save(&self) {
                log::info!("saving");
                helper();
            }
        }

        fn helper() {}
        "#;

        let mut parser = RustParser::new();
        let result = parser.parse_file(&PathBuf::from("test.rs"), content, "test.rs");

        assert!(!result.dependencies.is_empty());
        assert!(result
            .dependencies
            .iter()
            .all(|dependency| dependency.from_symbol == "test.rs::User::save"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "helper"));
    }

    #[test]
    fn test_parse_struct_fields_as_properties_with_type_signatures() {
        let mut parser = RustParser::new();
        let content = br#"
        pub struct RuntimeCore {
            pub verifications: VerificationService,
            executions: ExecutionService,
        }
        "#;

        let result = parser.parse_file(&PathBuf::from("test.rs"), content, "test.rs");

        let verification_field = result
            .symbols
            .iter()
            .find(|symbol| symbol.qualified_name == "test.rs::RuntimeCore::verifications")
            .expect("expected verifications field");
        assert_eq!(verification_field.kind, SymbolKind::Property);
        assert_eq!(
            verification_field.signature.as_deref(),
            Some("VerificationService")
        );
        assert_eq!(verification_field.visibility, Visibility::Public);

        let executions_field = result
            .symbols
            .iter()
            .find(|symbol| symbol.qualified_name == "test.rs::RuntimeCore::executions")
            .expect("expected executions field");
        assert_eq!(executions_field.kind, SymbolKind::Property);
        assert_eq!(
            executions_field.signature.as_deref(),
            Some("ExecutionService")
        );
        assert_eq!(executions_field.visibility, Visibility::Private);
    }
}
