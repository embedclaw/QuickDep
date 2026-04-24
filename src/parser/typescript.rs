//! TypeScript and TSX parser using tree-sitter-typescript.

use std::path::Path;

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource, Visibility};
use crate::parser::{make_qualified_name, node_text, ParseResult, Parser};
use crate::resolver::{Import, ImportKind};

/// TypeScript and JavaScript parser using tree-sitter-typescript.
pub struct TypeScriptParser {
    parser: tree_sitter::Parser,
}

impl TypeScriptParser {
    /// Create a new TypeScript/JavaScript parser.
    ///
    /// # Panics
    /// Panics if the bundled tree-sitter grammar cannot be loaded.
    pub fn new() -> Self {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .expect("Failed to set TypeScript language for tree-sitter parser");
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
            "export_statement" => {
                if let Some(declaration) = node.child_by_field_name("declaration") {
                    self.extract_symbols(
                        &declaration,
                        source,
                        file_path,
                        result,
                        parent_name,
                        enclosing_symbol,
                    );
                    return;
                }

                self.extract_reexport_statement(node, source, file_path, result);
                return;
            }
            "function_declaration" => {
                if let Some(qualified_name) = self.extract_function_like(
                    node,
                    source,
                    file_path,
                    result,
                    parent_name,
                    SymbolKind::Function,
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
            "class_declaration" | "abstract_class_declaration" => {
                if let Some(class_name) = self.extract_class(node, source, file_path, result) {
                    self.extract_class_relationships(node, source, file_path, result, &class_name);
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
            "interface_declaration" => {
                if let Some(interface_name) =
                    self.extract_interface(node, source, file_path, result)
                {
                    self.extract_interface_extends(
                        node,
                        source,
                        file_path,
                        result,
                        &interface_name,
                    );
                    if let Some(body) = node.child_by_field_name("body") {
                        self.extract_symbols(
                            &body,
                            source,
                            file_path,
                            result,
                            Some(&interface_name),
                            enclosing_symbol,
                        );
                    }
                    return;
                }
            }
            "method_definition" | "method_signature" | "abstract_method_signature" => {
                if let Some(qualified_name) = self.extract_function_like(
                    node,
                    source,
                    file_path,
                    result,
                    parent_name,
                    SymbolKind::Method,
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
            "public_field_definition" => {
                if let Some(qualified_name) =
                    self.extract_public_field(node, source, file_path, result, parent_name)
                {
                    if let Some(value) = node.child_by_field_name("value") {
                        self.extract_symbols(
                            &value,
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
            "variable_declarator" => {
                if let Some(qualified_name) =
                    self.extract_top_level_binding(node, source, file_path, result)
                {
                    if let Some(value) = node.child_by_field_name("value") {
                        self.extract_symbols(
                            &value,
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
            "type_alias_declaration" => {
                self.extract_type_alias(node, source, file_path, result);
            }
            "import_statement" => {
                self.extract_import_statement(node, source, file_path, result);
            }
            "call_expression" => {
                self.extract_call_like(node, source, file_path, result, enclosing_symbol);
            }
            "new_expression" => {
                self.extract_new_expression(node, source, file_path, result, enclosing_symbol);
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

    fn extract_function_like(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
        default_kind: SymbolKind,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.property_name(&name_node, source)?;
        let qualified_name = make_qualified_name(file_path, &name, parent_name);
        let visibility = self.visibility_for_node(node, source, parent_name, &name);
        let (line, column) = self.node_start(node);
        let signature = self.function_signature(node, source, &name);
        let kind = if parent_name.is_some() {
            SymbolKind::Method
        } else {
            default_kind
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

        if let Some(signature) = signature {
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
        let name = self.property_name(&name_node, source)?;
        let qualified_name = make_qualified_name(file_path, &name, None);
        let (line, column) = self.node_start(node);

        result.symbols.push(
            Symbol::new(
                name.clone(),
                qualified_name.clone(),
                SymbolKind::Class,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(self.visibility_for_node(node, source, None, &name))
            .with_source(SymbolSource::Local),
        );

        Some(name)
    }

    fn extract_interface(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.property_name(&name_node, source)?;
        let qualified_name = make_qualified_name(file_path, &name, None);
        let (line, column) = self.node_start(node);

        result.symbols.push(
            Symbol::new(
                name.clone(),
                qualified_name,
                SymbolKind::Interface,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(self.visibility_for_node(node, source, None, &name))
            .with_source(SymbolSource::Local),
        );

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
        let Some(name) = self.property_name(&name_node, source) else {
            return;
        };
        let qualified_name = make_qualified_name(file_path, &name, None);
        let (line, column) = self.node_start(node);

        result.symbols.push(
            Symbol::new(
                name.clone(),
                qualified_name,
                SymbolKind::TypeAlias,
                file_path.to_string(),
                line,
                column,
            )
            .with_visibility(self.visibility_for_node(node, source, None, &name))
            .with_source(SymbolSource::Local),
        );
    }

    fn extract_public_field(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        parent_name: Option<&str>,
    ) -> Option<String> {
        let name_node = node.child_by_field_name("name")?;
        let name = self.property_name(&name_node, source)?;
        let value = node.child_by_field_name("value");
        let is_method = matches!(
            value.as_ref().map(|node| node.kind()),
            Some("arrow_function")
        );
        let kind = if is_method {
            SymbolKind::Method
        } else {
            SymbolKind::Property
        };
        let qualified_name = make_qualified_name(file_path, &name, parent_name);
        let (line, column) = self.node_start(node);

        let mut symbol = Symbol::new(
            name.clone(),
            qualified_name.clone(),
            kind,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(self.visibility_for_node(node, source, parent_name, &name))
        .with_source(SymbolSource::Local);

        if is_method {
            if let Some(value) = value {
                if let Some(signature) = self.arrow_function_signature(&value, source, &name) {
                    symbol = symbol.with_signature(signature);
                }
            }
        }

        result.symbols.push(symbol);
        Some(qualified_name)
    }

    fn extract_top_level_binding(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) -> Option<String> {
        if !self.is_top_level(node) {
            return None;
        }

        let name_node = node.child_by_field_name("name")?;
        let name = self.property_name(&name_node, source)?;
        let qualified_name = make_qualified_name(file_path, &name, None);
        let (line, column) = self.node_start(node);
        let value = node.child_by_field_name("value");
        let kind = match value.as_ref().map(|value| value.kind()) {
            Some("arrow_function" | "function") => SymbolKind::Function,
            _ if self.is_const_binding(node, source) => SymbolKind::Constant,
            _ => SymbolKind::Variable,
        };

        let mut symbol = Symbol::new(
            name.clone(),
            qualified_name.clone(),
            kind,
            file_path.to_string(),
            line,
            column,
        )
        .with_visibility(self.visibility_for_node(node, source, None, &name))
        .with_source(SymbolSource::Local);

        if let Some(value) = value {
            if let Some(signature) = self.arrow_function_signature(&value, source, &name) {
                symbol = symbol.with_signature(signature);
            }
        }

        result.symbols.push(symbol);
        Some(qualified_name)
    }

    fn is_const_binding(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> bool {
        let mut current = node.parent();
        while let Some(parent) = current {
            match parent.kind() {
                "lexical_declaration" => {
                    let mut cursor = parent.walk();
                    for child in parent.children(&mut cursor) {
                        if child.kind() == "const" {
                            return true;
                        }
                        if child.kind() == "let" || child.kind() == "var" {
                            return false;
                        }
                    }

                    return node_text(&parent, source)
                        .map(|text| text.trim_start().starts_with("const "))
                        .unwrap_or(false);
                }
                "variable_statement" => {
                    let mut cursor = parent.walk();
                    for child in parent.children(&mut cursor) {
                        if child.kind() == "var" || child.kind() == "let" {
                            return false;
                        }
                    }
                }
                "program" | "statement_block" | "class_body" => return false,
                _ => current = parent.parent(),
            }
        }

        false
    }

    fn extract_class_relationships(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        class_name: &str,
    ) {
        let from_symbol = make_qualified_name(file_path, class_name, None);
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "class_heritage" {
                continue;
            }

            let mut heritage_cursor = child.walk();
            for clause in child.children(&mut heritage_cursor) {
                match clause.kind() {
                    "extends_clause" => {
                        for target in self.dependency_targets(&clause, source) {
                            result.dependencies.push(Dependency::new(
                                from_symbol.clone(),
                                target,
                                file_path.to_string(),
                                self.node_start(&clause).0,
                                DependencyKind::Inherit,
                            ));
                        }
                    }
                    "implements_clause" => {
                        for target in self.dependency_targets(&clause, source) {
                            result.dependencies.push(Dependency::new(
                                from_symbol.clone(),
                                target,
                                file_path.to_string(),
                                self.node_start(&clause).0,
                                DependencyKind::Implement,
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn extract_interface_extends(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
        interface_name: &str,
    ) {
        let from_symbol = make_qualified_name(file_path, interface_name, None);
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "extends_type_clause" {
                continue;
            }

            for target in self.dependency_targets(&child, source) {
                result.dependencies.push(Dependency::new(
                    from_symbol.clone(),
                    target,
                    file_path.to_string(),
                    self.node_start(&child).0,
                    DependencyKind::Inherit,
                ));
            }
        }
    }

    fn extract_import_statement(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let module = node
            .child_by_field_name("source")
            .and_then(|source_node| node_text(&source_node, source))
            .map(|value| self.strip_quotes(&value))
            .unwrap_or_default();
        let line = self.node_start(node).0;
        let mut saw_clause = false;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "import_clause" {
                saw_clause = true;
                self.extract_import_clause(&child, source, &module, file_path, line, result);
            }
        }

        if !saw_clause && !module.is_empty() {
            result.imports.push(Import::new(
                module,
                file_path.to_string(),
                line,
                ImportKind::Named,
            ));
        }
    }

    fn extract_reexport_statement(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        file_path: &str,
        result: &mut ParseResult,
    ) {
        let module = node
            .child_by_field_name("source")
            .and_then(|source_node| node_text(&source_node, source))
            .map(|value| self.strip_quotes(&value))
            .unwrap_or_default();
        if module.is_empty() {
            return;
        }

        let line = self.node_start(node).0;
        let mut saw_clause = false;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "export_clause" {
                continue;
            }

            saw_clause = true;
            self.extract_reexport_clause(&child, source, &module, file_path, line, result);
        }

        if !saw_clause {
            result.imports.push(Import::new(
                module,
                file_path.to_string(),
                line,
                ImportKind::ReExportGlob,
            ));
        }
    }

    fn extract_import_clause(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        module: &str,
        file_path: &str,
        line: u32,
        result: &mut ParseResult,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" => {
                    let alias = node_text(&child, source).unwrap_or_default();
                    result.imports.push(
                        Import::new(
                            format!("{module}::default"),
                            file_path.to_string(),
                            line,
                            ImportKind::Alias,
                        )
                        .with_alias(alias),
                    );
                }
                "namespace_import" => {
                    let mut namespace_cursor = child.walk();
                    for identifier in child.children(&mut namespace_cursor) {
                        if identifier.kind() == "identifier" {
                            let alias = node_text(&identifier, source).unwrap_or_default();
                            result.imports.push(
                                Import::new(
                                    module.to_string(),
                                    file_path.to_string(),
                                    line,
                                    ImportKind::Alias,
                                )
                                .with_alias(alias),
                            );
                        }
                    }
                }
                "named_imports" => {
                    let mut imports_cursor = child.walk();
                    for specifier in child.children(&mut imports_cursor) {
                        if specifier.kind() != "import_specifier" {
                            continue;
                        }

                        let name = specifier
                            .child_by_field_name("name")
                            .and_then(|name_node| node_text(&name_node, source))
                            .unwrap_or_default();
                        let alias = specifier
                            .child_by_field_name("alias")
                            .and_then(|alias_node| node_text(&alias_node, source));
                        let source = format!("{module}::{name}");

                        result.imports.push(match alias {
                            Some(alias) => {
                                Import::new(source, file_path.to_string(), line, ImportKind::Alias)
                                    .with_alias(alias)
                            }
                            None => {
                                Import::new(source, file_path.to_string(), line, ImportKind::Named)
                            }
                        });
                    }
                }
                _ => {}
            }
        }
    }

    fn extract_reexport_clause(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        module: &str,
        file_path: &str,
        line: u32,
        result: &mut ParseResult,
    ) {
        let mut cursor = node.walk();
        for specifier in node.children(&mut cursor) {
            if specifier.kind() != "export_specifier" {
                continue;
            }

            let name = specifier
                .child_by_field_name("name")
                .and_then(|name_node| self.property_name(&name_node, source))
                .or_else(|| {
                    let mut specifier_cursor = specifier.walk();
                    let fallback = specifier
                        .children(&mut specifier_cursor)
                        .find(|child| child.kind() == "identifier")
                        .and_then(|child| self.property_name(&child, source));
                    fallback
                })
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }

            let alias = specifier
                .child_by_field_name("alias")
                .and_then(|alias_node| self.property_name(&alias_node, source));
            let import_source = format!("{module}::{name}");

            result.imports.push(match alias {
                Some(alias) => Import::new(
                    import_source,
                    file_path.to_string(),
                    line,
                    ImportKind::ReExportAlias,
                )
                .with_alias(alias),
                None => Import::new(
                    import_source,
                    file_path.to_string(),
                    line,
                    ImportKind::ReExportNamed,
                ),
            });
        }
    }

    fn extract_call_like(
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
        let Some(function) = node.child_by_field_name("function") else {
            return;
        };
        let Some(target) = self.reference_name(&function, source) else {
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

    fn extract_new_expression(
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
        let Some(constructor) = node.child_by_field_name("constructor") else {
            return;
        };
        let Some(target) = self.reference_name(&constructor, source) else {
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
            .unwrap_or_default();
        Some(format!("{name}{parameters}{return_type}"))
    }

    fn arrow_function_signature(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        name: &str,
    ) -> Option<String> {
        let parameters = node
            .child_by_field_name("parameters")
            .or_else(|| node.child_by_field_name("parameter"))
            .and_then(|parameters| node_text(&parameters, source))?;
        let return_type = node
            .child_by_field_name("return_type")
            .and_then(|return_type| node_text(&return_type, source))
            .unwrap_or_default();
        Some(format!("{name}{parameters}{return_type}"))
    }

    fn dependency_targets(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Vec<String> {
        let mut targets = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if let Some(target) = self.reference_name(&child, source) {
                targets.push(target);
            }
        }
        targets.sort();
        targets.dedup();
        targets
    }

    fn property_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        let text = node_text(node, source)?;
        let text = self.strip_quotes(&text);
        let text = text.trim_start_matches('#').to_string();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    fn reference_name(&self, node: &tree_sitter::Node<'_>, source: &[u8]) -> Option<String> {
        if matches!(
            node.kind(),
            "identifier"
                | "property_identifier"
                | "private_property_identifier"
                | "type_identifier"
        ) {
            return self.property_name(node, source);
        }

        if node.kind() == "member_expression" {
            return node
                .child_by_field_name("property")
                .and_then(|property| self.property_name(&property, source));
        }

        node_text(node, source)
            .map(|text| self.simplify_reference(&text))
            .filter(|text| !text.is_empty())
    }

    fn simplify_reference(&self, text: &str) -> String {
        let mut simplified = self.strip_quotes(text);
        if let Some((head, _)) = simplified.split_once('<') {
            simplified = head.to_string();
        }
        simplified = simplified.trim_end_matches('?').to_string();

        simplified
            .rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default()
            .to_string()
    }

    fn visibility_for_node(
        &self,
        node: &tree_sitter::Node<'_>,
        source: &[u8],
        parent_name: Option<&str>,
        _name: &str,
    ) -> Visibility {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "accessibility_modifier" {
                continue;
            }

            return match node_text(&child, source).as_deref() {
                Some("private") => Visibility::Private,
                Some("protected") => Visibility::Protected,
                _ => Visibility::Public,
            };
        }

        if parent_name.is_some() {
            return Visibility::Public;
        }

        if self.is_exported(node) {
            Visibility::Public
        } else {
            Visibility::Private
        }
    }

    fn is_exported(&self, node: &tree_sitter::Node<'_>) -> bool {
        let mut current = node.parent();
        while let Some(parent) = current {
            match parent.kind() {
                "export_statement" => return true,
                "program" | "statement_block" | "class_body" => return false,
                _ => current = parent.parent(),
            }
        }
        false
    }

    fn is_top_level(&self, node: &tree_sitter::Node<'_>) -> bool {
        let mut current = node.parent();
        while let Some(parent) = current {
            match parent.kind() {
                "program" => return true,
                "statement_block" | "class_body" => return false,
                _ => current = parent.parent(),
            }
        }
        false
    }

    fn strip_quotes(&self, value: &str) -> String {
        value
            .trim()
            .trim_matches('\'')
            .trim_matches('"')
            .to_string()
    }

    fn node_start(&self, node: &tree_sitter::Node<'_>) -> (u32, u32) {
        let position = node.start_position();
        (position.row as u32 + 1, position.column as u32 + 1)
    }
}

impl Default for TypeScriptParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser for TypeScriptParser {
    fn language(&self) -> &'static str {
        "typescript"
    }

    fn parse_file(&mut self, path: &Path, content: &[u8], file_path: &str) -> ParseResult {
        let language = match path.extension().and_then(|extension| extension.to_str()) {
            Some("tsx") | Some("TSX") | Some("jsx") | Some("JSX") => {
                tree_sitter_typescript::LANGUAGE_TSX
            }
            _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        };
        self.parser
            .set_language(&language.into())
            .expect("Failed to set TypeScript language for tree-sitter parser");

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
        &["ts", "tsx", "js", "jsx", "mjs", "cjs"]
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_parse_typescript_fixture() {
        let mut parser = TypeScriptParser::new();
        let fixture_path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/typescript/sample.ts"
        ));
        let content = std::fs::read(&fixture_path).expect("Failed to read fixture");
        let result = parser.parse_file(&fixture_path, &content, "src/sample.ts");

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
            .any(|symbol| symbol.kind == SymbolKind::Function && symbol.name == "run"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Implement));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "formatName"));
        assert!(result
            .imports
            .iter()
            .any(|import| import.alias.as_deref() == Some("format")));
    }

    #[test]
    fn test_parse_javascript_fixture() {
        let mut parser = TypeScriptParser::new();
        let fixture_path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/javascript/sample.js"
        ));
        let content = std::fs::read(&fixture_path).expect("Failed to read fixture");
        let result = parser.parse_file(&fixture_path, &content, "src/sample.js");

        assert_eq!(result.error_count, 0);
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Class && symbol.name == "UserService"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Function && symbol.name == "run"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Call
                && dependency.to_symbol == "format"));
        assert!(result
            .imports
            .iter()
            .any(|import| import.alias.as_deref() == Some("format")));
    }

    #[test]
    fn test_parse_tsx_component() {
        let mut parser = TypeScriptParser::new();
        let content = br#"
export const Card = ({ title }: { title: string }) => {
    renderTitle(title);
    return <div>{title}</div>;
};

function renderTitle(value: string) {
    return value.trim();
}
"#;
        let result = parser.parse_file(&PathBuf::from("Card.tsx"), content, "src/Card.tsx");

        assert_eq!(result.error_count, 0);
        assert!(result.symbols.iter().any(|symbol| symbol.name == "Card"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "renderTitle"));
    }

    #[test]
    fn test_parse_jsx_component() {
        let mut parser = TypeScriptParser::new();
        let content = br#"
export const Card = ({ title }) => {
    renderTitle(title);
    return <div>{title}</div>;
};

function renderTitle(value) {
    return value.trim();
}
"#;
        let result = parser.parse_file(&PathBuf::from("Card.jsx"), content, "src/Card.jsx");

        assert_eq!(result.error_count, 0);
        assert!(result.symbols.iter().any(|symbol| symbol.name == "Card"));
        assert!(result
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "renderTitle"));
    }

    #[test]
    fn test_parse_typescript_reexports_as_imports() {
        let mut parser = TypeScriptParser::new();
        let content = br#"
export { helper as run } from "./shared";
export * from "./extra";
"#;
        let result = parser.parse_file(&PathBuf::from("index.ts"), content, "src/index.ts");

        assert_eq!(result.error_count, 0);
        assert!(result.imports.iter().any(|import| {
            import.kind == ImportKind::ReExportAlias
                && import.source == "./shared::helper"
                && import.alias.as_deref() == Some("run")
        }));
        assert!(result.imports.iter().any(|import| {
            import.kind == ImportKind::ReExportGlob && import.source == "./extra"
        }));
    }

    #[test]
    fn test_parse_typescript_exported_const_bindings() {
        let mut parser = TypeScriptParser::new();
        let content = br#"
const hidden = makeHidden();
export const parse = _parse(errors.real);
export let current = hidden;
"#;
        let result = parser.parse_file(&PathBuf::from("parse.ts"), content, "src/parse.ts");

        assert_eq!(result.error_count, 0);
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Constant && symbol.name == "parse"));
        assert!(result
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Variable && symbol.name == "current"));
    }
}
