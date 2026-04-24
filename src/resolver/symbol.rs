//! Symbol-resolution logic for parsed dependencies.

use crate::core::{Dependency, DependencyKind, Symbol, SymbolKind, SymbolSource};
use crate::parser::{detect_language, Language};
use crate::resolver::{normalize_module_path, rust_module_path, Import, ImportKind};
use path_clean::PathClean;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Virtual file path used for synthetic builtin symbols.
pub const BUILTIN_SYMBOL_FILE_PATH: &str = "<builtin>";

/// Virtual file path used for synthetic external symbols.
pub const EXTERNAL_SYMBOL_FILE_PATH: &str = "<external>";

#[derive(Debug, Default)]
struct ImportContext {
    exact: HashMap<String, String>,
    glob_modules: Vec<String>,
    exact_symbol_ids: HashMap<String, String>,
    glob_symbol_ids: HashMap<String, String>,
}

/// Unresolved dependency produced by the resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedDependency {
    /// Source symbol qualified name from the parser stage.
    pub from_qualified_name: String,
    /// Original unresolved target text from the parser stage.
    pub target: String,
    /// File where the dependency occurred.
    pub from_file: String,
    /// Source line number.
    pub from_line: u32,
}

/// Result of resolving a batch of raw parser dependencies.
#[derive(Debug, Clone, Default)]
pub struct ResolutionSummary {
    /// Dependencies successfully resolved to symbol IDs.
    pub resolved: Vec<Dependency>,
    /// Synthetic external/builtin symbols produced during resolution.
    pub non_local_symbols: Vec<Symbol>,
    /// Dependencies that could not be matched to a local symbol.
    pub unresolved: Vec<UnresolvedDependency>,
}

/// Resolver for mapping parser output to concrete project symbols.
#[derive(Debug, Clone, Default)]
pub struct Resolver {
    qualified_to_symbol_id: HashMap<String, String>,
    symbols_by_id: HashMap<String, Symbol>,
    rust_path_to_symbol_id: HashMap<String, String>,
    rust_method_suffix_to_symbol_ids: HashMap<String, Vec<String>>,
    rust_property_suffix_to_type_names: HashMap<String, Vec<String>>,
    file_name_to_symbol_ids: HashMap<(String, String), Vec<String>>,
    name_to_symbol_ids: HashMap<String, Vec<String>>,
    imports_by_file: HashMap<String, Vec<Import>>,
}

impl Resolver {
    /// Create a resolver from the current project symbol table.
    pub fn new(symbols: &[Symbol]) -> Self {
        Self::new_with_imports(symbols, &[])
    }

    /// Create a resolver from the current project symbol and import tables.
    pub fn new_with_imports(symbols: &[Symbol], imports: &[Import]) -> Self {
        let mut qualified_to_symbol_id = HashMap::new();
        let mut symbols_by_id = HashMap::new();
        let mut rust_path_to_symbol_id = HashMap::new();
        let mut rust_method_suffix_to_symbol_ids: HashMap<String, Vec<String>> = HashMap::new();
        let mut rust_property_suffix_to_type_names: HashMap<String, Vec<String>> = HashMap::new();
        let mut file_name_to_symbol_ids: HashMap<(String, String), Vec<String>> = HashMap::new();
        let mut name_to_symbol_ids: HashMap<String, Vec<String>> = HashMap::new();
        let mut imports_by_file: HashMap<String, Vec<Import>> = HashMap::new();

        for symbol in symbols {
            qualified_to_symbol_id.insert(symbol.qualified_name.clone(), symbol.id.clone());
            symbols_by_id.insert(symbol.id.clone(), symbol.clone());
            rust_path_to_symbol_id
                .insert(crate::resolver::symbol_rust_path(symbol), symbol.id.clone());
            file_name_to_symbol_ids
                .entry((symbol.file_path.clone(), symbol.name.clone()))
                .or_default()
                .push(symbol.id.clone());
            name_to_symbol_ids
                .entry(symbol.name.clone())
                .or_default()
                .push(symbol.id.clone());

            if let Some(suffix) = rust_symbol_suffix(symbol) {
                if symbol.kind == SymbolKind::Method {
                    rust_method_suffix_to_symbol_ids
                        .entry(suffix.clone())
                        .or_default()
                        .push(symbol.id.clone());
                }

                if symbol.kind == SymbolKind::Property {
                    if let Some(type_name) = symbol
                        .signature
                        .as_deref()
                        .and_then(extract_primary_rust_type_name)
                    {
                        rust_property_suffix_to_type_names
                            .entry(suffix)
                            .or_default()
                            .push(type_name);
                    }
                }
            }
        }

        for import in imports {
            imports_by_file
                .entry(import.file_path.clone())
                .or_default()
                .push(import.clone());
        }

        Self {
            qualified_to_symbol_id,
            symbols_by_id,
            rust_path_to_symbol_id,
            rust_method_suffix_to_symbol_ids,
            rust_property_suffix_to_type_names,
            file_name_to_symbol_ids,
            name_to_symbol_ids,
            imports_by_file,
        }
    }

    /// Resolve raw parser dependencies for a file using its imports.
    pub fn resolve_dependencies(
        &self,
        file_path: &str,
        imports: &[Import],
        raw_dependencies: &[Dependency],
    ) -> ResolutionSummary {
        let language = detect_language(Path::new(file_path));
        self.resolve_dependencies_with_language(file_path, imports, raw_dependencies, language)
    }

    /// Resolve raw parser dependencies for a file using its imports and a known source language.
    pub fn resolve_dependencies_with_language(
        &self,
        file_path: &str,
        imports: &[Import],
        raw_dependencies: &[Dependency],
        language: Option<Language>,
    ) -> ResolutionSummary {
        let import_context = self.build_import_context(file_path, imports, language);
        let file_module_path = rust_module_path(file_path);

        let mut resolved = Vec::new();
        let mut non_local_symbols = HashMap::new();
        let mut unresolved = Vec::new();

        for raw_dependency in raw_dependencies {
            let Some(from_symbol_id) = self
                .qualified_to_symbol_id
                .get(&raw_dependency.from_symbol)
                .cloned()
            else {
                unresolved.push(UnresolvedDependency {
                    from_qualified_name: raw_dependency.from_symbol.clone(),
                    target: raw_dependency.to_symbol.clone(),
                    from_file: raw_dependency.from_file.clone(),
                    from_line: raw_dependency.from_line,
                });
                continue;
            };

            let resolved_target = match language {
                Some(Language::TypeScript)
                | Some(Language::JavaScript)
                | Some(Language::Python) => self.resolve_script_target(
                    file_path,
                    &import_context,
                    &raw_dependency.to_symbol,
                ),
                Some(Language::Java) => {
                    self.resolve_java_target(file_path, &import_context, &raw_dependency.to_symbol)
                }
                Some(Language::CSharp) => self.resolve_csharp_target(
                    file_path,
                    &import_context,
                    &raw_dependency.to_symbol,
                ),
                Some(Language::Kotlin) => self.resolve_kotlin_target(
                    file_path,
                    &import_context,
                    &raw_dependency.to_symbol,
                ),
                Some(Language::Php) => {
                    self.resolve_php_target(file_path, &import_context, &raw_dependency.to_symbol)
                }
                Some(Language::Ruby) => {
                    self.resolve_ruby_target(file_path, &import_context, &raw_dependency.to_symbol)
                }
                Some(Language::Swift) => {
                    self.resolve_swift_target(file_path, &import_context, &raw_dependency.to_symbol)
                }
                Some(Language::Objc) => self.resolve_objc_target(
                    &raw_dependency.from_symbol,
                    file_path,
                    &import_context,
                    &raw_dependency.to_symbol,
                ),
                Some(Language::C) | Some(Language::Cpp) => self.resolve_include_target(
                    file_path,
                    &import_context,
                    &raw_dependency.to_symbol,
                ),
                _ => self
                    .resolve_rust_receiver_target(
                        &raw_dependency.from_symbol,
                        &raw_dependency.to_symbol,
                    )
                    .or_else(|| {
                        let candidates = self.target_candidates(
                            &raw_dependency.from_symbol,
                            file_path,
                            &file_module_path,
                            &import_context,
                            &raw_dependency.to_symbol,
                        );

                        candidates
                            .iter()
                            .find_map(|candidate| self.rust_path_to_symbol_id.get(candidate))
                            .cloned()
                    })
                    .or_else(|| self.unique_rust_symbol_id_by_suffix(&raw_dependency.to_symbol))
                    .or_else(|| {
                        self.resolve_bare_name(
                            file_path,
                            &import_context,
                            &raw_dependency.to_symbol,
                        )
                    }),
            };

            if let Some(to_symbol_id) = resolved_target {
                let mut dependency = raw_dependency.clone();
                dependency.from_symbol = from_symbol_id;
                dependency.to_symbol = to_symbol_id;
                resolved.push(dependency);
            } else if let Some(symbol) = self.classify_non_local_target(
                imports,
                &import_context,
                &raw_dependency.to_symbol,
                &raw_dependency.kind,
                language,
            ) {
                let mut dependency = raw_dependency.clone();
                dependency.from_symbol = from_symbol_id;
                dependency.to_symbol = symbol.id.clone();
                resolved.push(dependency);
                non_local_symbols.insert(symbol.qualified_name.clone(), symbol);
            } else {
                unresolved.push(UnresolvedDependency {
                    from_qualified_name: raw_dependency.from_symbol.clone(),
                    target: raw_dependency.to_symbol.clone(),
                    from_file: raw_dependency.from_file.clone(),
                    from_line: raw_dependency.from_line,
                });
            }
        }

        let mut non_local_symbols = non_local_symbols.into_values().collect::<Vec<_>>();
        non_local_symbols.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));

        ResolutionSummary {
            resolved,
            non_local_symbols,
            unresolved,
        }
    }

    fn build_import_context(
        &self,
        file_path: &str,
        imports: &[Import],
        language: Option<Language>,
    ) -> ImportContext {
        match language {
            Some(Language::TypeScript) => {
                self.build_script_import_context(file_path, imports, Language::TypeScript)
            }
            Some(Language::JavaScript) => {
                self.build_script_import_context(file_path, imports, Language::JavaScript)
            }
            Some(Language::Java) => self.build_java_import_context(imports),
            Some(Language::CSharp) => self.build_csharp_import_context(imports),
            Some(Language::Kotlin) => self.build_kotlin_import_context(imports),
            Some(Language::Php) => self.build_php_import_context(imports),
            Some(Language::Ruby) => self.build_ruby_import_context(file_path, imports),
            Some(Language::Swift) => self.build_swift_import_context(imports),
            Some(Language::Objc) => {
                self.build_include_import_context(file_path, imports, Language::Objc)
            }
            Some(Language::Python) => {
                self.build_script_import_context(file_path, imports, Language::Python)
            }
            Some(Language::C) => self.build_include_import_context(file_path, imports, Language::C),
            Some(Language::Cpp) => {
                self.build_include_import_context(file_path, imports, Language::Cpp)
            }
            _ => self.build_rust_import_context(file_path, imports),
        }
    }

    fn build_rust_import_context(&self, file_path: &str, imports: &[Import]) -> ImportContext {
        let file_module_path = rust_module_path(file_path);
        let mut context = ImportContext::default();

        for import in imports {
            let normalized_source = normalize_module_path(&import.source, &file_module_path);

            match import.kind {
                ImportKind::Glob => context.glob_modules.push(normalized_source),
                ImportKind::Named | ImportKind::Alias | ImportKind::SelfImport => {
                    if let Some(name) = import.effective_name() {
                        context.exact.insert(name.to_string(), normalized_source);
                    }
                }
                ImportKind::ReExportNamed
                | ImportKind::ReExportGlob
                | ImportKind::ReExportAlias => {}
            }
        }

        context
    }

    fn build_script_import_context(
        &self,
        file_path: &str,
        imports: &[Import],
        language: Language,
    ) -> ImportContext {
        let mut context = ImportContext::default();
        let mut glob_conflicts = HashSet::new();

        for import in imports {
            match import.kind {
                ImportKind::Named | ImportKind::Alias | ImportKind::SelfImport => {
                    if let Some((bound_name, symbol_id)) =
                        self.resolve_script_import(file_path, import, language)
                    {
                        context.exact_symbol_ids.insert(bound_name, symbol_id);
                    }
                }
                ImportKind::Glob => {
                    for candidate in
                        self.script_module_candidates(file_path, &import.source, language)
                    {
                        let exports =
                            self.collect_script_exports(&candidate, language, &mut HashSet::new());
                        self.merge_symbol_exports(
                            exports,
                            &mut context.glob_symbol_ids,
                            &mut glob_conflicts,
                        );
                    }
                }
                ImportKind::ReExportNamed
                | ImportKind::ReExportGlob
                | ImportKind::ReExportAlias => {}
            }
        }

        context
    }

    fn build_java_import_context(&self, imports: &[Import]) -> ImportContext {
        let mut context = ImportContext::default();
        let mut glob_conflicts = HashSet::new();

        for import in imports {
            match import.kind {
                ImportKind::Named | ImportKind::Alias | ImportKind::SelfImport => {
                    if let Some((bound_name, symbol_id)) = self.resolve_java_import(import) {
                        context.exact_symbol_ids.insert(bound_name, symbol_id);
                    }
                }
                ImportKind::Glob => {
                    let exports = self.collect_java_glob_exports(&import.source);
                    self.merge_symbol_exports(
                        exports,
                        &mut context.glob_symbol_ids,
                        &mut glob_conflicts,
                    );
                }
                ImportKind::ReExportNamed
                | ImportKind::ReExportGlob
                | ImportKind::ReExportAlias => {}
            }
        }

        context
    }

    fn build_csharp_import_context(&self, imports: &[Import]) -> ImportContext {
        let mut context = ImportContext::default();
        let mut glob_conflicts = HashSet::new();

        for import in imports {
            match import.kind {
                ImportKind::Named | ImportKind::Alias | ImportKind::SelfImport => {
                    if let Some((bound_name, symbol_id)) = self.resolve_csharp_import(import) {
                        context.exact_symbol_ids.insert(bound_name, symbol_id);
                    }
                }
                ImportKind::Glob => {
                    let exports = self.collect_csharp_glob_exports(&import.source);
                    self.merge_symbol_exports(
                        exports,
                        &mut context.glob_symbol_ids,
                        &mut glob_conflicts,
                    );
                }
                ImportKind::ReExportNamed
                | ImportKind::ReExportGlob
                | ImportKind::ReExportAlias => {}
            }
        }

        context
    }

    fn build_kotlin_import_context(&self, imports: &[Import]) -> ImportContext {
        let mut context = ImportContext::default();
        let mut glob_conflicts = HashSet::new();

        for import in imports {
            match import.kind {
                ImportKind::Named | ImportKind::Alias | ImportKind::SelfImport => {
                    if let Some((bound_name, symbol_id)) = self.resolve_kotlin_import(import) {
                        context.exact_symbol_ids.insert(bound_name, symbol_id);
                    }
                }
                ImportKind::Glob => {
                    let exports = self.collect_kotlin_glob_exports(&import.source);
                    self.merge_symbol_exports(
                        exports,
                        &mut context.glob_symbol_ids,
                        &mut glob_conflicts,
                    );
                }
                ImportKind::ReExportNamed
                | ImportKind::ReExportGlob
                | ImportKind::ReExportAlias => {}
            }
        }

        context
    }

    fn build_php_import_context(&self, imports: &[Import]) -> ImportContext {
        let mut context = ImportContext::default();
        let mut glob_conflicts = HashSet::new();

        for import in imports {
            match import.kind {
                ImportKind::Named | ImportKind::Alias | ImportKind::SelfImport => {
                    if let Some((bound_name, symbol_id)) = self.resolve_php_import(import) {
                        context.exact_symbol_ids.insert(bound_name, symbol_id);
                    }
                }
                ImportKind::Glob => {
                    let exports = self.collect_php_glob_exports(&import.source);
                    self.merge_symbol_exports(
                        exports,
                        &mut context.glob_symbol_ids,
                        &mut glob_conflicts,
                    );
                }
                ImportKind::ReExportNamed
                | ImportKind::ReExportGlob
                | ImportKind::ReExportAlias => {}
            }
        }

        context
    }

    fn build_ruby_import_context(&self, file_path: &str, imports: &[Import]) -> ImportContext {
        let mut context = ImportContext::default();
        let mut glob_conflicts = HashSet::new();

        for import in imports {
            if import.kind != ImportKind::Glob {
                continue;
            }

            for candidate in self.ruby_module_candidates(file_path, &import.source) {
                let exports = self.collect_ruby_exports(&candidate);
                self.merge_symbol_exports(
                    exports,
                    &mut context.glob_symbol_ids,
                    &mut glob_conflicts,
                );
            }
        }

        context
    }

    fn build_swift_import_context(&self, imports: &[Import]) -> ImportContext {
        let mut context = ImportContext::default();
        for import in imports {
            if import.kind == ImportKind::Glob {
                context.glob_modules.push(import.source.clone());
            }
        }
        context
    }

    fn build_include_import_context(
        &self,
        file_path: &str,
        imports: &[Import],
        language: Language,
    ) -> ImportContext {
        let mut context = ImportContext::default();
        let mut glob_conflicts = HashSet::new();

        for import in imports {
            if !matches!(
                import.kind,
                ImportKind::Named | ImportKind::Alias | ImportKind::SelfImport
            ) {
                continue;
            }

            for candidate in self.include_module_candidates(file_path, &import.source, language) {
                self.insert_glob_symbols_for_candidate(
                    &candidate,
                    &mut context.glob_symbol_ids,
                    &mut glob_conflicts,
                );
            }
        }

        context
    }

    fn classify_non_local_target(
        &self,
        imports: &[Import],
        import_context: &ImportContext,
        raw_target: &str,
        dependency_kind: &DependencyKind,
        language: Option<Language>,
    ) -> Option<Symbol> {
        if let Some(target) = builtin_target(raw_target, language) {
            return Some(synthetic_symbol(
                &target,
                SymbolSource::Builtin,
                dependency_kind,
            ));
        }

        let external_target = match language {
            Some(Language::TypeScript) | Some(Language::JavaScript) => {
                resolve_external_script_target(
                    imports,
                    raw_target,
                    language.unwrap_or(Language::TypeScript),
                )
            }
            Some(Language::Java) => self.resolve_external_java_target(imports, raw_target),
            Some(Language::CSharp) => self.resolve_external_csharp_target(imports, raw_target),
            Some(Language::Kotlin) => self.resolve_external_kotlin_target(imports, raw_target),
            Some(Language::Php) => self.resolve_external_php_target(imports, raw_target),
            Some(Language::Ruby) => self.resolve_external_ruby_target(imports, raw_target),
            Some(Language::Swift) => self.resolve_external_swift_target(imports, raw_target),
            Some(Language::Objc) => self.resolve_external_objc_target(imports, raw_target),
            Some(Language::Python) => {
                resolve_external_script_target(imports, raw_target, Language::Python)
            }
            Some(Language::Go) => resolve_external_script_target(imports, raw_target, Language::Go),
            _ => self.resolve_external_rust_target(import_context, raw_target),
        }?;

        Some(synthetic_symbol(
            &external_target,
            SymbolSource::External,
            dependency_kind,
        ))
    }

    fn resolve_bare_name(
        &self,
        file_path: &str,
        import_context: &ImportContext,
        raw_target: &str,
    ) -> Option<String> {
        let bare_target = raw_target.trim_end_matches('!');

        if let Some(symbol_ids) = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), bare_target.to_string()))
            .filter(|symbol_ids| symbol_ids.len() == 1)
        {
            return symbol_ids.first().cloned();
        }

        if let Some(imported_path) = import_context.exact.get(bare_target) {
            if let Some(symbol_id) = self.rust_path_to_symbol_id.get(imported_path) {
                return Some(symbol_id.clone());
            }
        }

        for module_path in &import_context.glob_modules {
            let candidate = format!("{module_path}::{bare_target}");
            if let Some(symbol_id) = self.rust_path_to_symbol_id.get(&candidate) {
                return Some(symbol_id.clone());
            }
        }

        self.name_to_symbol_ids
            .get(bare_target)
            .filter(|symbol_ids| symbol_ids.len() == 1)
            .and_then(|symbol_ids| symbol_ids.first().cloned())
    }

    fn resolve_rust_receiver_target(
        &self,
        from_qualified_name: &str,
        raw_target: &str,
    ) -> Option<String> {
        let target = raw_target.trim_end_matches('!').trim();
        if !target.contains('.') {
            return None;
        }

        let segments = target
            .split('.')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.len() < 2 {
            return None;
        }

        let method_name = segments.last()?.to_string();
        let mut owner = self.resolve_rust_receiver_root_type(from_qualified_name, segments[0])?;

        for field_name in &segments[1..segments.len().saturating_sub(1)] {
            owner = self.resolve_rust_property_type_by_suffix(&owner, field_name)?;
        }

        self.unique_rust_symbol_id_by_suffix(&format!("{owner}::{method_name}"))
    }

    fn resolve_rust_receiver_root_type(
        &self,
        from_qualified_name: &str,
        receiver: &str,
    ) -> Option<String> {
        if receiver == "self" {
            return rust_owner_suffix_from_qualified_name(from_qualified_name);
        }

        let symbol_id = self.qualified_to_symbol_id.get(from_qualified_name)?;
        let symbol = self.symbols_by_id.get(symbol_id)?;
        let signature = symbol.signature.as_deref()?;
        let parameter_types = rust_parameter_type_hints(signature);
        parameter_types.get(receiver).cloned()
    }

    fn resolve_rust_property_type_by_suffix(
        &self,
        owner_suffix: &str,
        property_name: &str,
    ) -> Option<String> {
        let suffix = format!("{owner_suffix}::{property_name}");
        let mut matches = self
            .rust_property_suffix_to_type_names
            .get(&suffix)?
            .to_vec();
        matches.sort();
        matches.dedup();
        (matches.len() == 1).then(|| matches.remove(0))
    }

    fn resolve_script_target(
        &self,
        file_path: &str,
        import_context: &ImportContext,
        raw_target: &str,
    ) -> Option<String> {
        let bare_target = raw_target
            .trim_end_matches('!')
            .rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();

        if let Some(symbol_ids) = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), bare_target.to_string()))
            .filter(|symbol_ids| symbol_ids.len() == 1)
        {
            return symbol_ids.first().cloned();
        }

        if let Some(symbol_id) = import_context.exact_symbol_ids.get(bare_target) {
            return Some(symbol_id.clone());
        }

        if let Some(symbol_id) = import_context.glob_symbol_ids.get(bare_target) {
            return Some(symbol_id.clone());
        }

        self.name_to_symbol_ids
            .get(bare_target)
            .filter(|symbol_ids| symbol_ids.len() == 1)
            .and_then(|symbol_ids| symbol_ids.first().cloned())
    }

    fn resolve_java_target(
        &self,
        file_path: &str,
        import_context: &ImportContext,
        raw_target: &str,
    ) -> Option<String> {
        let bare_target = raw_target
            .trim_end_matches('!')
            .rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();

        if bare_target.is_empty() {
            return None;
        }

        if let Some(symbol_ids) = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), bare_target.to_string()))
            .filter(|symbol_ids| symbol_ids.len() == 1)
        {
            return symbol_ids.first().cloned();
        }

        if let Some(symbol_id) = import_context.exact_symbol_ids.get(bare_target) {
            return Some(symbol_id.clone());
        }

        if let Some(symbol_id) = import_context.glob_symbol_ids.get(bare_target) {
            return Some(symbol_id.clone());
        }

        let same_package_candidate = Path::new(file_path)
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(format!("{bare_target}.java"))
            .clean()
            .to_string_lossy()
            .to_string();
        if let Some(symbol_id) =
            self.unique_java_top_level_symbol_id_for_file_name(&same_package_candidate, bare_target)
        {
            return Some(symbol_id);
        }

        self.name_to_symbol_ids
            .get(bare_target)
            .filter(|symbol_ids| symbol_ids.len() == 1)
            .and_then(|symbol_ids| symbol_ids.first().cloned())
    }

    fn resolve_csharp_target(
        &self,
        file_path: &str,
        import_context: &ImportContext,
        raw_target: &str,
    ) -> Option<String> {
        let bare_target = raw_target
            .trim_end_matches('!')
            .rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();

        if bare_target.is_empty() {
            return None;
        }

        if let Some(symbol_ids) = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), bare_target.to_string()))
            .filter(|symbol_ids| symbol_ids.len() == 1)
        {
            return symbol_ids.first().cloned();
        }

        if let Some(symbol_id) = import_context.exact_symbol_ids.get(bare_target) {
            return Some(symbol_id.clone());
        }

        if let Some(symbol_id) = import_context.glob_symbol_ids.get(bare_target) {
            return Some(symbol_id.clone());
        }

        let same_directory_candidate = Path::new(file_path)
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(format!("{bare_target}.cs"))
            .clean()
            .to_string_lossy()
            .to_string();
        if let Some(symbol_id) = self
            .unique_csharp_top_level_symbol_id_for_file_name(&same_directory_candidate, bare_target)
        {
            return Some(symbol_id);
        }

        self.name_to_symbol_ids
            .get(bare_target)
            .filter(|symbol_ids| symbol_ids.len() == 1)
            .and_then(|symbol_ids| symbol_ids.first().cloned())
    }

    fn resolve_kotlin_target(
        &self,
        file_path: &str,
        import_context: &ImportContext,
        raw_target: &str,
    ) -> Option<String> {
        let bare_target = raw_target
            .trim_end_matches('!')
            .rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();

        if bare_target.is_empty() {
            return None;
        }

        if let Some(symbol_ids) = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), bare_target.to_string()))
            .filter(|symbol_ids| symbol_ids.len() == 1)
        {
            return symbol_ids.first().cloned();
        }

        if let Some(symbol_id) = import_context.exact_symbol_ids.get(bare_target) {
            return Some(symbol_id.clone());
        }

        if let Some(symbol_id) = import_context.glob_symbol_ids.get(bare_target) {
            return Some(symbol_id.clone());
        }

        let same_package_candidate = Path::new(file_path)
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(format!("{bare_target}.kt"))
            .clean()
            .to_string_lossy()
            .to_string();
        if let Some(symbol_id) = self
            .unique_kotlin_top_level_symbol_id_for_file_name(&same_package_candidate, bare_target)
        {
            return Some(symbol_id);
        }

        self.name_to_symbol_ids
            .get(bare_target)
            .filter(|symbol_ids| symbol_ids.len() == 1)
            .and_then(|symbol_ids| symbol_ids.first().cloned())
    }

    fn resolve_php_target(
        &self,
        file_path: &str,
        import_context: &ImportContext,
        raw_target: &str,
    ) -> Option<String> {
        let bare_target = raw_target
            .trim_end_matches('!')
            .rsplit(['\\', '.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();

        if bare_target.is_empty() {
            return None;
        }

        if let Some(symbol_ids) = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), bare_target.to_string()))
            .filter(|symbol_ids| symbol_ids.len() == 1)
        {
            return symbol_ids.first().cloned();
        }

        if let Some(symbol_id) = import_context.exact_symbol_ids.get(bare_target) {
            return Some(symbol_id.clone());
        }

        if let Some(symbol_id) = import_context.glob_symbol_ids.get(bare_target) {
            return Some(symbol_id.clone());
        }

        let same_namespace_candidate = Path::new(file_path)
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(format!("{bare_target}.php"))
            .clean()
            .to_string_lossy()
            .to_string();
        if let Some(symbol_id) = self
            .unique_php_top_level_symbol_id_for_file_name(&same_namespace_candidate, bare_target)
        {
            return Some(symbol_id);
        }

        self.name_to_symbol_ids
            .get(bare_target)
            .filter(|symbol_ids| symbol_ids.len() == 1)
            .and_then(|symbol_ids| symbol_ids.first().cloned())
    }

    fn resolve_ruby_target(
        &self,
        file_path: &str,
        import_context: &ImportContext,
        raw_target: &str,
    ) -> Option<String> {
        let target = raw_target.trim_end_matches('!');
        if target.is_empty() {
            return None;
        }

        if let Some(symbol_id) = import_context.glob_symbol_ids.get(target) {
            return Some(symbol_id.clone());
        }

        if target.contains("::") {
            if let Some(symbol_id) = self.unique_ruby_symbol_id_by_suffix(target) {
                return Some(symbol_id);
            }
        }

        if let Some(symbol_ids) = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), target.to_string()))
            .filter(|symbol_ids| symbol_ids.len() == 1)
        {
            return symbol_ids.first().cloned();
        }

        let bare_target = target
            .rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();
        if bare_target.is_empty() {
            return None;
        }

        if let Some(symbol_ids) = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), bare_target.to_string()))
            .filter(|symbol_ids| symbol_ids.len() == 1)
        {
            return symbol_ids.first().cloned();
        }

        if let Some(symbol_id) = import_context.glob_symbol_ids.get(bare_target) {
            return Some(symbol_id.clone());
        }

        self.name_to_symbol_ids
            .get(bare_target)
            .filter(|symbol_ids| symbol_ids.len() == 1)
            .and_then(|symbol_ids| symbol_ids.first().cloned())
    }

    fn resolve_swift_target(
        &self,
        file_path: &str,
        _import_context: &ImportContext,
        raw_target: &str,
    ) -> Option<String> {
        let target = raw_target.trim_end_matches('!');
        if target.is_empty() {
            return None;
        }

        if target.contains('.') {
            let normalized = target.replace('.', "::");
            return self.unique_swift_symbol_id_by_suffix(&normalized);
        }

        if let Some(symbol_ids) = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), target.to_string()))
            .filter(|symbol_ids| symbol_ids.len() == 1)
        {
            return symbol_ids.first().cloned();
        }

        let bare_target = target
            .rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();
        if bare_target.is_empty() {
            return None;
        }

        if let Some(symbol_ids) = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), bare_target.to_string()))
            .filter(|symbol_ids| symbol_ids.len() == 1)
        {
            return symbol_ids.first().cloned();
        }

        self.name_to_symbol_ids
            .get(bare_target)
            .filter(|symbol_ids| symbol_ids.len() == 1)
            .and_then(|symbol_ids| symbol_ids.first().cloned())
    }

    fn resolve_objc_target(
        &self,
        from_qualified_name: &str,
        file_path: &str,
        import_context: &ImportContext,
        raw_target: &str,
    ) -> Option<String> {
        let target = raw_target
            .trim_end_matches('!')
            .rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();
        if target.is_empty() {
            return None;
        }

        if let Some((_, owner, _)) =
            from_qualified_name
                .rsplit_once("::")
                .and_then(|(prefix, method_name)| {
                    prefix
                        .rsplit_once("::")
                        .map(|(file, owner)| (file, owner, method_name))
                })
        {
            let sibling_target = format!("{file_path}::{owner}::{target}");
            if let Some(symbol_id) = self.qualified_to_symbol_id.get(&sibling_target) {
                return Some(symbol_id.clone());
            }
        }

        self.resolve_include_target(file_path, import_context, raw_target)
    }

    fn resolve_include_target(
        &self,
        file_path: &str,
        import_context: &ImportContext,
        raw_target: &str,
    ) -> Option<String> {
        let bare_target = raw_target
            .trim_end_matches('!')
            .rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();

        if let Some(symbol_ids) = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), bare_target.to_string()))
            .filter(|symbol_ids| symbol_ids.len() == 1)
        {
            return symbol_ids.first().cloned();
        }

        if let Some(symbol_id) = import_context.glob_symbol_ids.get(bare_target) {
            return Some(symbol_id.clone());
        }

        self.name_to_symbol_ids
            .get(bare_target)
            .filter(|symbol_ids| symbol_ids.len() == 1)
            .and_then(|symbol_ids| symbol_ids.first().cloned())
    }

    fn resolve_external_rust_target(
        &self,
        import_context: &ImportContext,
        raw_target: &str,
    ) -> Option<String> {
        let target = raw_target.trim_end_matches('!');

        if target.contains("::") {
            if let Some(import_target) = self.rewrite_import_prefixed_target(target, import_context)
            {
                if is_external_rust_path(&import_target) {
                    return Some(import_target);
                }
            }

            if is_known_rust_external_target(target) {
                return Some(target.to_string());
            }

            return None;
        }

        if let Some(imported_path) = import_context.exact.get(target) {
            if is_external_rust_path(imported_path) {
                return Some(imported_path.clone());
            }
        }

        let mut external_globs = import_context
            .glob_modules
            .iter()
            .filter(|module_path| is_external_rust_path(module_path))
            .map(|module_path| format!("{module_path}::{target}"))
            .collect::<Vec<_>>();
        external_globs.sort();
        external_globs.dedup();

        if external_globs.len() == 1 {
            return external_globs.into_iter().next();
        }

        // Bare targets with no import context stay unresolved; they may still be local.
        None
    }

    fn target_candidates(
        &self,
        from_qualified_name: &str,
        file_path: &str,
        file_module_path: &str,
        import_context: &ImportContext,
        raw_target: &str,
    ) -> Vec<String> {
        let mut candidates = Vec::new();
        let target = raw_target.trim_end_matches('!');

        if target.contains("::") {
            if let Some(self_target) =
                self.rewrite_self_target(from_qualified_name, target, file_module_path)
            {
                candidates.push(self_target);
            }

            if let Some(import_target) = self.rewrite_import_prefixed_target(target, import_context)
            {
                candidates.push(import_target);
            }

            let normalized = normalize_module_path(target, file_module_path);
            candidates.push(normalized.clone());

            if !file_module_path.is_empty()
                && !target.starts_with("crate::")
                && !target.starts_with("self::")
                && !target.starts_with("super::")
            {
                candidates.push(format!("{file_module_path}::{target}"));
            }
        } else {
            if let Some(import_target) = import_context.exact.get(target) {
                candidates.push(import_target.clone());
            }

            for module_path in &import_context.glob_modules {
                candidates.push(format!("{module_path}::{target}"));
            }

            if !file_module_path.is_empty() {
                candidates.push(format!("{file_module_path}::{target}"));
            }

            let file_local = self
                .file_name_to_symbol_ids
                .contains_key(&(file_path.to_string(), target.to_string()));
            if file_local {
                candidates.push(target.to_string());
            }
        }

        candidates.sort();
        candidates.dedup();
        candidates
    }

    fn rewrite_import_prefixed_target(
        &self,
        target: &str,
        import_context: &ImportContext,
    ) -> Option<String> {
        let (prefix, suffix) = target.split_once("::")?;
        let imported = import_context.exact.get(prefix)?;
        Some(format!("{imported}::{suffix}"))
    }

    fn rewrite_self_target(
        &self,
        from_qualified_name: &str,
        target: &str,
        file_module_path: &str,
    ) -> Option<String> {
        let suffix = target.strip_prefix("Self::")?;
        let source_prefix = from_qualified_name
            .split_once("::")
            .and_then(|(_, tail)| tail.rsplit_once("::").map(|(prefix, _)| prefix.to_string()))?;

        if file_module_path.is_empty() {
            Some(format!("{source_prefix}::{suffix}"))
        } else {
            Some(format!("{file_module_path}::{source_prefix}::{suffix}"))
        }
    }

    fn resolve_script_import(
        &self,
        file_path: &str,
        import: &Import,
        language: Language,
    ) -> Option<(String, String)> {
        let bound_name = import
            .alias
            .clone()
            .or_else(|| import.effective_name().map(ToString::to_string))?;
        let (module_path, symbol_name) = split_import_source(&import.source)?;
        if symbol_name == "default" {
            return None;
        }

        for candidate in self.script_module_candidates(file_path, module_path, language) {
            if let Some(symbol_id) =
                self.resolve_script_export(&candidate, symbol_name, language, &mut HashSet::new())
            {
                return Some((bound_name, symbol_id));
            }
        }

        None
    }

    fn resolve_java_import(&self, import: &Import) -> Option<(String, String)> {
        let bound_name = java_import_bound_name(import)?;

        if let Some((class_path, member_name)) = split_import_source(&import.source) {
            for candidate in self.java_type_candidates(class_path) {
                if let Some(symbol_id) =
                    self.unique_symbol_id_for_file_name(&candidate, member_name)
                {
                    return Some((bound_name, symbol_id));
                }
            }
            return None;
        }

        for candidate in self.java_type_candidates(&import.source) {
            if let Some(symbol_id) =
                self.unique_java_top_level_symbol_id_for_file_name(&candidate, &bound_name)
            {
                return Some((bound_name, symbol_id));
            }
        }

        None
    }

    fn resolve_csharp_import(&self, import: &Import) -> Option<(String, String)> {
        let bound_name = csharp_import_bound_name(import)?;
        let target_name = import
            .source
            .rsplit('.')
            .find(|segment| !segment.is_empty())?;

        for candidate in self.csharp_type_candidates(&import.source) {
            if let Some(symbol_id) =
                self.unique_csharp_top_level_symbol_id_for_file_name(&candidate, target_name)
            {
                return Some((bound_name, symbol_id));
            }
        }

        None
    }

    fn resolve_kotlin_import(&self, import: &Import) -> Option<(String, String)> {
        let bound_name = kotlin_import_bound_name(import)?;
        let target_name = import
            .source
            .rsplit('.')
            .find(|segment| !segment.is_empty())?;

        for candidate in self.kotlin_type_candidates(&import.source) {
            if let Some(symbol_id) =
                self.unique_kotlin_top_level_symbol_id_for_file_name(&candidate, target_name)
            {
                return Some((bound_name, symbol_id));
            }
        }

        if let Some((owner_path, member_name)) = import.source.rsplit_once('.') {
            for candidate in self.kotlin_type_candidates(owner_path) {
                if let Some(symbol_id) =
                    self.unique_symbol_id_for_file_name(&candidate, member_name)
                {
                    return Some((bound_name, symbol_id));
                }
            }

            if let Some(symbol_id) = self
                .collect_kotlin_package_exports(owner_path)
                .get(member_name)
            {
                return Some((bound_name, symbol_id.clone()));
            }
        }

        None
    }

    fn resolve_php_import(&self, import: &Import) -> Option<(String, String)> {
        let bound_name = php_import_bound_name(import)?;
        let target_name = import
            .source
            .rsplit(['\\', '.'])
            .find(|segment| !segment.is_empty())?;

        for candidate in self.php_type_candidates(&import.source) {
            if let Some(symbol_id) =
                self.unique_php_top_level_symbol_id_for_file_name(&candidate, target_name)
            {
                return Some((bound_name, symbol_id));
            }
        }

        if let Some((owner_path, member_name)) = php_owner_and_member(&import.source) {
            for candidate in self.php_type_candidates(owner_path) {
                if let Some(symbol_id) =
                    self.unique_symbol_id_for_file_name(&candidate, member_name)
                {
                    return Some((bound_name, symbol_id));
                }
            }

            if let Some(symbol_id) = self
                .collect_php_namespace_exports(owner_path)
                .get(member_name)
            {
                return Some((bound_name, symbol_id.clone()));
            }
        }

        None
    }

    fn script_module_candidates(
        &self,
        file_path: &str,
        module_path: &str,
        language: Language,
    ) -> Vec<String> {
        match language {
            Language::TypeScript | Language::JavaScript => {
                resolve_typescript_module_candidates(file_path, module_path)
            }
            Language::Python => resolve_python_module_candidates(file_path, module_path),
            _ => Vec::new(),
        }
    }

    fn java_type_candidates(&self, import_path: &str) -> Vec<String> {
        let relative = format!("{}.java", import_path.replace('.', "/"));
        let suffix = format!("/{relative}");
        let mut candidates = self
            .symbols_by_id
            .values()
            .map(|symbol| symbol.file_path.clone())
            .filter(|file_path| file_path == &relative || file_path.ends_with(&suffix))
            .collect::<Vec<_>>();
        candidates.sort();
        candidates.dedup();
        candidates
    }

    fn collect_java_glob_exports(&self, import_source: &str) -> HashMap<String, String> {
        let mut exports = HashMap::new();
        let mut conflicts = HashSet::new();

        for (name, symbol_id) in self.collect_java_package_exports(import_source) {
            insert_export_symbol(&mut exports, &mut conflicts, name, symbol_id);
        }
        for (name, symbol_id) in self.collect_java_member_exports(import_source) {
            insert_export_symbol(&mut exports, &mut conflicts, name, symbol_id);
        }

        exports
    }

    fn collect_java_package_exports(&self, package_path: &str) -> HashMap<String, String> {
        let relative_dir = package_path.replace('.', "/");
        let suffix = format!("/{relative_dir}");
        let mut exports = HashMap::new();
        let mut conflicts = HashSet::new();

        for symbol in self.symbols_by_id.values() {
            if !is_java_top_level_type(symbol) {
                continue;
            }

            let Some(parent) = Path::new(&symbol.file_path).parent() else {
                continue;
            };
            let parent = parent.to_string_lossy();
            if parent != relative_dir && !parent.ends_with(&suffix) {
                continue;
            }

            insert_export_symbol(
                &mut exports,
                &mut conflicts,
                symbol.name.clone(),
                symbol.id.clone(),
            );
        }

        exports
    }

    fn collect_java_member_exports(&self, class_path: &str) -> HashMap<String, String> {
        let class_name = class_path
            .rsplit('.')
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();
        let mut exports = HashMap::new();
        let mut conflicts = HashSet::new();

        for candidate in self.java_type_candidates(class_path) {
            let member_prefix = format!("{candidate}::{class_name}::");
            for symbol in self.symbols_by_id.values() {
                if symbol.file_path != candidate {
                    continue;
                }
                if !matches!(
                    symbol.kind,
                    SymbolKind::Method | SymbolKind::Property | SymbolKind::Constant
                ) {
                    continue;
                }
                if !symbol.qualified_name.starts_with(&member_prefix) {
                    continue;
                }

                insert_export_symbol(
                    &mut exports,
                    &mut conflicts,
                    symbol.name.clone(),
                    symbol.id.clone(),
                );
            }
        }

        exports
    }

    fn csharp_type_candidates(&self, namespace_path: &str) -> Vec<String> {
        let relative = format!("{}.cs", namespace_path.replace('.', "/"));
        let suffix = format!("/{relative}");
        let mut candidates = self
            .symbols_by_id
            .values()
            .map(|symbol| symbol.file_path.clone())
            .filter(|file_path| file_path == &relative || file_path.ends_with(&suffix))
            .collect::<Vec<_>>();
        candidates.sort();
        candidates.dedup();
        candidates
    }

    fn collect_csharp_glob_exports(&self, import_source: &str) -> HashMap<String, String> {
        let mut exports = HashMap::new();
        let mut conflicts = HashSet::new();

        for (name, symbol_id) in self.collect_csharp_namespace_exports(import_source) {
            insert_export_symbol(&mut exports, &mut conflicts, name, symbol_id);
        }
        for (name, symbol_id) in self.collect_csharp_member_exports(import_source) {
            insert_export_symbol(&mut exports, &mut conflicts, name, symbol_id);
        }

        exports
    }

    fn kotlin_type_candidates(&self, package_path: &str) -> Vec<String> {
        let relative = format!("{}.kt", package_path.replace('.', "/"));
        let suffix = format!("/{relative}");
        let mut candidates = self
            .symbols_by_id
            .values()
            .map(|symbol| symbol.file_path.clone())
            .filter(|file_path| file_path == &relative || file_path.ends_with(&suffix))
            .collect::<Vec<_>>();
        candidates.sort();
        candidates.dedup();
        candidates
    }

    fn collect_kotlin_glob_exports(&self, import_source: &str) -> HashMap<String, String> {
        let mut exports = HashMap::new();
        let mut conflicts = HashSet::new();

        for (name, symbol_id) in self.collect_kotlin_package_exports(import_source) {
            insert_export_symbol(&mut exports, &mut conflicts, name, symbol_id);
        }
        for (name, symbol_id) in self.collect_kotlin_member_exports(import_source) {
            insert_export_symbol(&mut exports, &mut conflicts, name, symbol_id);
        }

        exports
    }

    fn collect_kotlin_package_exports(&self, package_path: &str) -> HashMap<String, String> {
        let relative_dir = package_path.replace('.', "/");
        let suffix = format!("/{relative_dir}");
        let mut exports = HashMap::new();
        let mut conflicts = HashSet::new();

        for symbol in self.symbols_by_id.values() {
            if !is_kotlin_top_level_export(symbol) {
                continue;
            }

            let Some(parent) = Path::new(&symbol.file_path).parent() else {
                continue;
            };
            let parent = parent.to_string_lossy();
            if parent != relative_dir && !parent.ends_with(&suffix) {
                continue;
            }

            insert_export_symbol(
                &mut exports,
                &mut conflicts,
                symbol.name.clone(),
                symbol.id.clone(),
            );
        }

        exports
    }

    fn collect_kotlin_member_exports(&self, type_path: &str) -> HashMap<String, String> {
        let type_name = type_path
            .rsplit('.')
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();
        let mut exports = HashMap::new();
        let mut conflicts = HashSet::new();

        for candidate in self.kotlin_type_candidates(type_path) {
            let member_prefix = format!("{candidate}::{type_name}::");
            for symbol in self.symbols_by_id.values() {
                if symbol.file_path != candidate {
                    continue;
                }
                if !matches!(
                    symbol.kind,
                    SymbolKind::Method | SymbolKind::Property | SymbolKind::Constant
                ) {
                    continue;
                }
                if !symbol.qualified_name.starts_with(&member_prefix) {
                    continue;
                }

                insert_export_symbol(
                    &mut exports,
                    &mut conflicts,
                    symbol.name.clone(),
                    symbol.id.clone(),
                );
            }
        }

        exports
    }

    fn php_type_candidates(&self, namespace_path: &str) -> Vec<String> {
        let path = namespace_path
            .trim_start_matches('\\')
            .replace(['\\', '.'], "/");
        let relative = format!("{path}.php");
        let suffix = format!("/{relative}");
        let mut candidates = self
            .symbols_by_id
            .values()
            .map(|symbol| symbol.file_path.clone())
            .filter(|file_path| file_path == &relative || file_path.ends_with(&suffix))
            .collect::<Vec<_>>();
        candidates.sort();
        candidates.dedup();
        candidates
    }

    fn collect_php_glob_exports(&self, import_source: &str) -> HashMap<String, String> {
        let mut exports = HashMap::new();
        let mut conflicts = HashSet::new();

        for (name, symbol_id) in self.collect_php_namespace_exports(import_source) {
            insert_export_symbol(&mut exports, &mut conflicts, name, symbol_id);
        }
        for (name, symbol_id) in self.collect_php_member_exports(import_source) {
            insert_export_symbol(&mut exports, &mut conflicts, name, symbol_id);
        }

        exports
    }

    fn ruby_module_candidates(&self, file_path: &str, import_source: &str) -> Vec<String> {
        let import_source = import_source.trim();
        if import_source.is_empty() {
            return Vec::new();
        }

        let parent = Path::new(file_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let mut bases = Vec::new();

        if import_source.starts_with("./") || import_source.starts_with("../") {
            bases.push(parent.join(import_source).clean());
        } else {
            bases.push(PathBuf::from(import_source).clean());
            bases.push(parent.join(import_source).clean());
        }

        let mut candidates = Vec::new();
        for base in bases {
            if base.extension().is_some() {
                candidates.push(base);
                continue;
            }

            candidates.push(base.with_extension("rb"));
            candidates.push(base.with_extension("rake"));
        }

        normalize_candidates(&mut candidates)
    }

    fn collect_ruby_exports(&self, file_path: &str) -> HashMap<String, String> {
        let mut exports = HashMap::new();
        let mut conflicts = HashSet::new();

        for ((candidate_file, symbol_name), symbol_ids) in &self.file_name_to_symbol_ids {
            if candidate_file != file_path || symbol_ids.len() != 1 {
                continue;
            }

            let Some(symbol) = self.symbols_by_id.get(&symbol_ids[0]) else {
                continue;
            };
            if !is_ruby_export(symbol) {
                continue;
            }

            insert_export_symbol(
                &mut exports,
                &mut conflicts,
                symbol_name.clone(),
                symbol_ids[0].clone(),
            );

            if let Some(export_path) = ruby_export_path(symbol) {
                insert_export_symbol(
                    &mut exports,
                    &mut conflicts,
                    export_path,
                    symbol_ids[0].clone(),
                );
            }
        }

        exports
    }

    fn collect_php_namespace_exports(&self, namespace_path: &str) -> HashMap<String, String> {
        let relative_dir = namespace_path
            .trim_start_matches('\\')
            .replace(['\\', '.'], "/");
        let suffix = format!("/{relative_dir}");
        let mut exports = HashMap::new();
        let mut conflicts = HashSet::new();

        for symbol in self.symbols_by_id.values() {
            if !is_php_top_level_export(symbol) {
                continue;
            }

            let Some(parent) = Path::new(&symbol.file_path).parent() else {
                continue;
            };
            let parent = parent.to_string_lossy();
            if parent != relative_dir && !parent.ends_with(&suffix) {
                continue;
            }

            insert_export_symbol(
                &mut exports,
                &mut conflicts,
                symbol.name.clone(),
                symbol.id.clone(),
            );
        }

        exports
    }

    fn collect_php_member_exports(&self, type_path: &str) -> HashMap<String, String> {
        let type_name = type_path
            .rsplit(['\\', '.'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();
        let mut exports = HashMap::new();
        let mut conflicts = HashSet::new();

        for candidate in self.php_type_candidates(type_path) {
            let member_prefix = format!("{candidate}::{type_name}::");
            for symbol in self.symbols_by_id.values() {
                if symbol.file_path != candidate {
                    continue;
                }
                if !matches!(
                    symbol.kind,
                    SymbolKind::Method | SymbolKind::Property | SymbolKind::Constant
                ) {
                    continue;
                }
                if !symbol.qualified_name.starts_with(&member_prefix) {
                    continue;
                }

                insert_export_symbol(
                    &mut exports,
                    &mut conflicts,
                    symbol.name.clone(),
                    symbol.id.clone(),
                );
            }
        }

        exports
    }

    fn collect_csharp_namespace_exports(&self, namespace_path: &str) -> HashMap<String, String> {
        let relative_dir = namespace_path.replace('.', "/");
        let suffix = format!("/{relative_dir}");
        let mut exports = HashMap::new();
        let mut conflicts = HashSet::new();

        for symbol in self.symbols_by_id.values() {
            if !is_csharp_top_level_type(symbol) {
                continue;
            }

            let Some(parent) = Path::new(&symbol.file_path).parent() else {
                continue;
            };
            let parent = parent.to_string_lossy();
            if parent != relative_dir && !parent.ends_with(&suffix) {
                continue;
            }

            insert_export_symbol(
                &mut exports,
                &mut conflicts,
                symbol.name.clone(),
                symbol.id.clone(),
            );
        }

        exports
    }

    fn collect_csharp_member_exports(&self, type_path: &str) -> HashMap<String, String> {
        let type_name = type_path
            .rsplit('.')
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();
        let mut exports = HashMap::new();
        let mut conflicts = HashSet::new();

        for candidate in self.csharp_type_candidates(type_path) {
            let member_prefix = format!("{candidate}::{type_name}::");
            for symbol in self.symbols_by_id.values() {
                if symbol.file_path != candidate {
                    continue;
                }
                if !matches!(
                    symbol.kind,
                    SymbolKind::Method | SymbolKind::Property | SymbolKind::Constant
                ) {
                    continue;
                }
                if !symbol.qualified_name.starts_with(&member_prefix) {
                    continue;
                }

                insert_export_symbol(
                    &mut exports,
                    &mut conflicts,
                    symbol.name.clone(),
                    symbol.id.clone(),
                );
            }
        }

        exports
    }

    fn include_module_candidates(
        &self,
        file_path: &str,
        include_path: &str,
        language: Language,
    ) -> Vec<String> {
        let include_path = include_path.trim();
        if include_path.is_empty() {
            return Vec::new();
        }

        let parent = Path::new(file_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let relative_base = parent.join(include_path).clean();
        let root_base = PathBuf::from(include_path.trim_start_matches("./")).clean();
        let mut bases = vec![relative_base];
        if !root_base.as_os_str().is_empty() {
            bases.push(root_base);
        }

        let mut candidates = Vec::new();
        for base in bases {
            if base.extension().is_some() {
                candidates.push(base.clone());
                if let (Some(parent), Some(stem)) = (base.parent(), base.file_stem()) {
                    for extension in include_companion_extensions(language) {
                        candidates
                            .push(parent.join(format!("{}.{extension}", stem.to_string_lossy())));
                    }
                }
            } else {
                for extension in include_default_extensions(language) {
                    candidates.push(base.with_extension(extension));
                }
            }
        }

        normalize_candidates(&mut candidates)
    }

    fn insert_glob_symbols_for_candidate(
        &self,
        candidate: &str,
        target_map: &mut HashMap<String, String>,
        conflicts: &mut HashSet<String>,
    ) {
        for ((candidate_file, symbol_name), symbol_ids) in &self.file_name_to_symbol_ids {
            if candidate_file != candidate || symbol_ids.len() != 1 {
                continue;
            }

            if conflicts.contains(symbol_name) {
                continue;
            }

            if let Some(existing) = target_map.get(symbol_name) {
                if existing != &symbol_ids[0] {
                    target_map.remove(symbol_name);
                    conflicts.insert(symbol_name.clone());
                }
            } else {
                target_map.insert(symbol_name.clone(), symbol_ids[0].clone());
            }
        }
    }

    fn collect_script_exports(
        &self,
        file_path: &str,
        language: Language,
        visiting: &mut HashSet<String>,
    ) -> HashMap<String, String> {
        if !visiting.insert(file_path.to_string()) {
            return HashMap::new();
        }

        let mut exports = HashMap::new();
        let mut conflicts = HashSet::new();

        for ((candidate_file, symbol_name), symbol_ids) in &self.file_name_to_symbol_ids {
            if candidate_file != file_path || symbol_ids.len() != 1 {
                continue;
            }
            insert_export_symbol(
                &mut exports,
                &mut conflicts,
                symbol_name.clone(),
                symbol_ids[0].clone(),
            );
        }

        if let Some(imports) = self.imports_by_file.get(file_path) {
            for import in imports {
                match import.kind {
                    ImportKind::ReExportNamed | ImportKind::ReExportAlias => {
                        let Some(export_name) = import.effective_name().map(ToString::to_string)
                        else {
                            continue;
                        };
                        let Some((module_path, symbol_name)) = split_import_source(&import.source)
                        else {
                            continue;
                        };

                        for candidate in
                            self.script_module_candidates(file_path, module_path, language)
                        {
                            if let Some(symbol_id) = self.resolve_script_export(
                                &candidate,
                                symbol_name,
                                language,
                                visiting,
                            ) {
                                insert_export_symbol(
                                    &mut exports,
                                    &mut conflicts,
                                    export_name.clone(),
                                    symbol_id,
                                );
                            }
                        }
                    }
                    ImportKind::ReExportGlob => {
                        for candidate in
                            self.script_module_candidates(file_path, &import.source, language)
                        {
                            let nested =
                                self.collect_script_exports(&candidate, language, visiting);
                            self.merge_symbol_exports(nested, &mut exports, &mut conflicts);
                        }
                    }
                    _ => {}
                }
            }
        }

        visiting.remove(file_path);
        exports
    }

    fn resolve_script_export(
        &self,
        file_path: &str,
        export_name: &str,
        language: Language,
        visiting: &mut HashSet<String>,
    ) -> Option<String> {
        if !visiting.insert(file_path.to_string()) {
            return None;
        }

        if let Some(symbol_id) = self.unique_symbol_id_for_file_name(file_path, export_name) {
            visiting.remove(file_path);
            return Some(symbol_id);
        }

        let resolved = self.imports_by_file.get(file_path).and_then(|imports| {
            for import in imports {
                match import.kind {
                    ImportKind::ReExportNamed | ImportKind::ReExportAlias => {
                        if import.effective_name() != Some(export_name) {
                            continue;
                        }
                        let Some((module_path, symbol_name)) = split_import_source(&import.source)
                        else {
                            continue;
                        };
                        for candidate in
                            self.script_module_candidates(file_path, module_path, language)
                        {
                            if let Some(symbol_id) = self.resolve_script_export(
                                &candidate,
                                symbol_name,
                                language,
                                visiting,
                            ) {
                                return Some(symbol_id);
                            }
                        }
                    }
                    ImportKind::ReExportGlob => {
                        for candidate in
                            self.script_module_candidates(file_path, &import.source, language)
                        {
                            if let Some(symbol_id) = self.resolve_script_export(
                                &candidate,
                                export_name,
                                language,
                                visiting,
                            ) {
                                return Some(symbol_id);
                            }
                        }
                    }
                    _ => {}
                }
            }
            None
        });

        visiting.remove(file_path);
        resolved
    }

    fn unique_symbol_id_for_file_name(&self, file_path: &str, name: &str) -> Option<String> {
        self.file_name_to_symbol_ids
            .get(&(file_path.to_string(), name.to_string()))
            .filter(|symbol_ids| symbol_ids.len() == 1)
            .and_then(|symbol_ids| symbol_ids.first().cloned())
    }

    fn unique_java_top_level_symbol_id_for_file_name(
        &self,
        file_path: &str,
        name: &str,
    ) -> Option<String> {
        let symbol_ids = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), name.to_string()))?;
        let mut matches = symbol_ids
            .iter()
            .filter_map(|symbol_id| self.symbols_by_id.get(symbol_id))
            .filter(|symbol| is_java_top_level_type(symbol))
            .map(|symbol| symbol.id.clone())
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        (matches.len() == 1).then(|| matches.remove(0))
    }

    fn unique_csharp_top_level_symbol_id_for_file_name(
        &self,
        file_path: &str,
        name: &str,
    ) -> Option<String> {
        let symbol_ids = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), name.to_string()))?;
        let mut matches = symbol_ids
            .iter()
            .filter_map(|symbol_id| self.symbols_by_id.get(symbol_id))
            .filter(|symbol| is_csharp_top_level_type(symbol))
            .map(|symbol| symbol.id.clone())
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        (matches.len() == 1).then(|| matches.remove(0))
    }

    fn unique_kotlin_top_level_symbol_id_for_file_name(
        &self,
        file_path: &str,
        name: &str,
    ) -> Option<String> {
        let symbol_ids = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), name.to_string()))?;
        let mut matches = symbol_ids
            .iter()
            .filter_map(|symbol_id| self.symbols_by_id.get(symbol_id))
            .filter(|symbol| is_kotlin_top_level_export(symbol))
            .map(|symbol| symbol.id.clone())
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        (matches.len() == 1).then(|| matches.remove(0))
    }

    fn unique_php_top_level_symbol_id_for_file_name(
        &self,
        file_path: &str,
        name: &str,
    ) -> Option<String> {
        let symbol_ids = self
            .file_name_to_symbol_ids
            .get(&(file_path.to_string(), name.to_string()))?;
        let mut matches = symbol_ids
            .iter()
            .filter_map(|symbol_id| self.symbols_by_id.get(symbol_id))
            .filter(|symbol| is_php_top_level_export(symbol))
            .map(|symbol| symbol.id.clone())
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        (matches.len() == 1).then(|| matches.remove(0))
    }

    fn unique_ruby_symbol_id_by_suffix(&self, target: &str) -> Option<String> {
        let suffix = format!("::{target}");
        let mut matches = self
            .symbols_by_id
            .values()
            .filter(|symbol| is_ruby_export(symbol))
            .filter(|symbol| symbol.qualified_name.ends_with(&suffix))
            .map(|symbol| symbol.id.clone())
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        (matches.len() == 1).then(|| matches.remove(0))
    }

    fn unique_swift_symbol_id_by_suffix(&self, target: &str) -> Option<String> {
        let suffix = format!("::{target}");
        let mut matches = self
            .symbols_by_id
            .values()
            .filter(|symbol| symbol.qualified_name.ends_with(&suffix))
            .map(|symbol| symbol.id.clone())
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        (matches.len() == 1).then(|| matches.remove(0))
    }

    fn unique_rust_symbol_id_by_suffix(&self, target: &str) -> Option<String> {
        let target = target.trim_end_matches('!').trim();
        let mut matches = self.rust_method_suffix_to_symbol_ids.get(target)?.to_vec();
        matches.sort();
        matches.dedup();
        (matches.len() == 1).then(|| matches.remove(0))
    }

    fn merge_symbol_exports(
        &self,
        exports: HashMap<String, String>,
        target_map: &mut HashMap<String, String>,
        conflicts: &mut HashSet<String>,
    ) {
        for (name, symbol_id) in exports {
            insert_export_symbol(target_map, conflicts, name, symbol_id);
        }
    }

    fn resolve_external_java_target(&self, imports: &[Import], raw_target: &str) -> Option<String> {
        let bare_target = raw_target
            .trim_end_matches('!')
            .rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();

        if bare_target.is_empty() {
            return None;
        }

        let mut exact_matches = imports
            .iter()
            .filter(|import| self.is_external_java_import(import))
            .filter_map(|import| {
                let bound_name = java_import_bound_name(import)?;
                (bound_name == bare_target).then(|| import.source.clone())
            })
            .collect::<Vec<_>>();
        exact_matches.sort();
        exact_matches.dedup();
        if exact_matches.len() == 1 {
            return exact_matches.into_iter().next();
        }

        let mut glob_matches = imports
            .iter()
            .filter(|import| import.kind == ImportKind::Glob)
            .filter(|import| self.is_external_java_import(import))
            .map(|import| {
                let looks_like_class = import
                    .source
                    .rsplit('.')
                    .next()
                    .and_then(|segment| segment.chars().next())
                    .map(|first| first.is_ascii_uppercase())
                    .unwrap_or(false);
                if looks_like_class {
                    format!("{}::{}", import.source, bare_target)
                } else {
                    format!("{}.{}", import.source, bare_target)
                }
            })
            .collect::<Vec<_>>();
        glob_matches.sort();
        glob_matches.dedup();
        if glob_matches.len() == 1 {
            return glob_matches.into_iter().next();
        }

        None
    }

    fn is_external_java_import(&self, import: &Import) -> bool {
        match import.kind {
            ImportKind::Glob => self.collect_java_glob_exports(&import.source).is_empty(),
            ImportKind::Named | ImportKind::Alias | ImportKind::SelfImport => {
                self.resolve_java_import(import).is_none()
            }
            ImportKind::ReExportNamed | ImportKind::ReExportGlob | ImportKind::ReExportAlias => {
                false
            }
        }
    }

    fn resolve_external_csharp_target(
        &self,
        imports: &[Import],
        raw_target: &str,
    ) -> Option<String> {
        let bare_target = raw_target
            .trim_end_matches('!')
            .rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();

        if bare_target.is_empty() {
            return None;
        }

        let mut exact_matches = imports
            .iter()
            .filter(|import| self.is_external_csharp_import(import))
            .filter_map(|import| {
                let bound_name = csharp_import_bound_name(import)?;
                (bound_name == bare_target).then(|| import.source.clone())
            })
            .collect::<Vec<_>>();
        exact_matches.sort();
        exact_matches.dedup();
        if exact_matches.len() == 1 {
            return exact_matches.into_iter().next();
        }

        let mut glob_matches = imports
            .iter()
            .filter(|import| import.kind == ImportKind::Glob)
            .filter(|import| self.is_external_csharp_import(import))
            .map(|import| format!("{}.{}", import.source, bare_target))
            .collect::<Vec<_>>();
        glob_matches.sort();
        glob_matches.dedup();
        if glob_matches.len() == 1 {
            return glob_matches.into_iter().next();
        }

        None
    }

    fn is_external_csharp_import(&self, import: &Import) -> bool {
        match import.kind {
            ImportKind::Glob => self.collect_csharp_glob_exports(&import.source).is_empty(),
            ImportKind::Named | ImportKind::Alias | ImportKind::SelfImport => {
                self.resolve_csharp_import(import).is_none()
            }
            ImportKind::ReExportNamed | ImportKind::ReExportGlob | ImportKind::ReExportAlias => {
                false
            }
        }
    }

    fn resolve_external_kotlin_target(
        &self,
        imports: &[Import],
        raw_target: &str,
    ) -> Option<String> {
        let bare_target = raw_target
            .trim_end_matches('!')
            .rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();

        if bare_target.is_empty() {
            return None;
        }

        let mut exact_matches = imports
            .iter()
            .filter(|import| self.is_external_kotlin_import(import))
            .filter_map(|import| {
                let bound_name = kotlin_import_bound_name(import)?;
                (bound_name == bare_target).then(|| import.source.clone())
            })
            .collect::<Vec<_>>();
        exact_matches.sort();
        exact_matches.dedup();
        if exact_matches.len() == 1 {
            return exact_matches.into_iter().next();
        }

        let mut glob_matches = imports
            .iter()
            .filter(|import| import.kind == ImportKind::Glob)
            .filter(|import| self.is_external_kotlin_import(import))
            .map(|import| format!("{}.{}", import.source, bare_target))
            .collect::<Vec<_>>();
        glob_matches.sort();
        glob_matches.dedup();
        if glob_matches.len() == 1 {
            return glob_matches.into_iter().next();
        }

        None
    }

    fn is_external_kotlin_import(&self, import: &Import) -> bool {
        match import.kind {
            ImportKind::Glob => self.collect_kotlin_glob_exports(&import.source).is_empty(),
            ImportKind::Named | ImportKind::Alias | ImportKind::SelfImport => {
                self.resolve_kotlin_import(import).is_none()
            }
            ImportKind::ReExportNamed | ImportKind::ReExportGlob | ImportKind::ReExportAlias => {
                false
            }
        }
    }

    fn resolve_external_php_target(&self, imports: &[Import], raw_target: &str) -> Option<String> {
        let bare_target = raw_target
            .trim_end_matches('!')
            .rsplit(['\\', '.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();

        if bare_target.is_empty() {
            return None;
        }

        let mut exact_matches = imports
            .iter()
            .filter(|import| self.is_external_php_import(import))
            .filter_map(|import| {
                let bound_name = php_import_bound_name(import)?;
                (bound_name == bare_target).then(|| import.source.clone())
            })
            .collect::<Vec<_>>();
        exact_matches.sort();
        exact_matches.dedup();
        if exact_matches.len() == 1 {
            return exact_matches.into_iter().next();
        }

        None
    }

    fn is_external_php_import(&self, import: &Import) -> bool {
        match import.kind {
            ImportKind::Glob => self.collect_php_glob_exports(&import.source).is_empty(),
            ImportKind::Named | ImportKind::Alias | ImportKind::SelfImport => {
                self.resolve_php_import(import).is_none()
            }
            ImportKind::ReExportNamed | ImportKind::ReExportGlob | ImportKind::ReExportAlias => {
                false
            }
        }
    }

    fn resolve_external_ruby_target(
        &self,
        _imports: &[Import],
        _raw_target: &str,
    ) -> Option<String> {
        None
    }

    fn resolve_external_swift_target(
        &self,
        imports: &[Import],
        raw_target: &str,
    ) -> Option<String> {
        let target = raw_target.trim_end_matches('!');
        if target.is_empty() {
            return None;
        }

        if target.contains('.') {
            let module_name = target.split('.').next().unwrap_or_default();
            let imported = imports
                .iter()
                .any(|import| import.kind == ImportKind::Glob && import.source == module_name);

            if imported || !self.name_to_symbol_ids.contains_key(module_name) {
                return Some(target.replace('.', "::"));
            }
        }

        let bare_target = raw_target
            .trim_end_matches('!')
            .rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();

        if bare_target.is_empty() {
            return None;
        }

        let starts_with_uppercase = bare_target
            .chars()
            .next()
            .map(char::is_uppercase)
            .unwrap_or(false);
        if !starts_with_uppercase {
            return None;
        }

        let mut modules = imports
            .iter()
            .filter(|import| import.kind == ImportKind::Glob)
            .map(|import| import.source.clone())
            .collect::<Vec<_>>();
        modules.sort();
        modules.dedup();

        (modules.len() == 1).then(|| format!("{}::{}", modules[0], bare_target))
    }

    fn resolve_external_objc_target(&self, imports: &[Import], raw_target: &str) -> Option<String> {
        let bare_target = raw_target
            .trim_end_matches('!')
            .rsplit(['.', ':'])
            .find(|segment| !segment.is_empty())
            .unwrap_or_default();

        if bare_target.is_empty() {
            return None;
        }

        let starts_with_uppercase = bare_target
            .chars()
            .next()
            .map(char::is_uppercase)
            .unwrap_or(false);
        if !starts_with_uppercase {
            return None;
        }

        let mut modules = imports
            .iter()
            .filter_map(|import| import.source.split('/').next())
            .filter(|module| !module.is_empty() && !module.ends_with(".h"))
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        modules.sort();
        modules.dedup();

        (modules.len() == 1).then(|| format!("{}::{}", modules[0], bare_target))
    }
}

fn insert_export_symbol(
    target_map: &mut HashMap<String, String>,
    conflicts: &mut HashSet<String>,
    name: String,
    symbol_id: String,
) {
    if conflicts.contains(&name) {
        return;
    }

    if let Some(existing) = target_map.get(&name) {
        if existing != &symbol_id {
            target_map.remove(&name);
            conflicts.insert(name);
        }
    } else {
        target_map.insert(name, symbol_id);
    }
}

fn is_java_top_level_type(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
    ) && symbol.qualified_name == format!("{}::{}", symbol.file_path, symbol.name)
}

fn is_csharp_top_level_type(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Class | SymbolKind::Interface | SymbolKind::Struct | SymbolKind::Enum
    ) && symbol.qualified_name == format!("{}::{}", symbol.file_path, symbol.name)
}

fn is_kotlin_top_level_export(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Enum
            | SymbolKind::Function
            | SymbolKind::Property
            | SymbolKind::Constant
    ) && symbol.qualified_name == format!("{}::{}", symbol.file_path, symbol.name)
}

fn is_php_top_level_export(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Class
            | SymbolKind::Interface
            | SymbolKind::Trait
            | SymbolKind::Enum
            | SymbolKind::Function
            | SymbolKind::Constant
    ) && symbol.qualified_name == format!("{}::{}", symbol.file_path, symbol.name)
}

fn is_ruby_export(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Class | SymbolKind::Module | SymbolKind::Function | SymbolKind::Constant
    )
}

fn ruby_export_path(symbol: &Symbol) -> Option<String> {
    symbol
        .qualified_name
        .strip_prefix(&format!("{}::", symbol.file_path))
        .map(ToString::to_string)
}

fn java_import_bound_name(import: &Import) -> Option<String> {
    if let Some(alias) = &import.alias {
        return Some(alias.clone());
    }

    if let Some((_, member_name)) = split_import_source(&import.source) {
        return Some(member_name.to_string());
    }

    import
        .source
        .rsplit('.')
        .find(|segment| !segment.is_empty())
        .map(ToString::to_string)
}

fn csharp_import_bound_name(import: &Import) -> Option<String> {
    if let Some(alias) = &import.alias {
        return Some(alias.clone());
    }

    import
        .source
        .rsplit('.')
        .find(|segment| !segment.is_empty())
        .map(ToString::to_string)
}

fn kotlin_import_bound_name(import: &Import) -> Option<String> {
    if let Some(alias) = &import.alias {
        return Some(alias.clone());
    }

    import
        .source
        .rsplit('.')
        .find(|segment| !segment.is_empty())
        .map(ToString::to_string)
}

fn php_import_bound_name(import: &Import) -> Option<String> {
    if let Some(alias) = &import.alias {
        return Some(alias.clone());
    }

    import
        .source
        .rsplit(['\\', '.'])
        .find(|segment| !segment.is_empty())
        .map(ToString::to_string)
}

fn php_owner_and_member(source: &str) -> Option<(&str, &str)> {
    source.rsplit_once('\\').or_else(|| source.rsplit_once('.'))
}

fn split_import_source(source: &str) -> Option<(&str, &str)> {
    let (module_path, symbol_name) = source.rsplit_once("::")?;
    if module_path.is_empty() || symbol_name.is_empty() {
        None
    } else {
        Some((module_path, symbol_name))
    }
}

fn resolve_typescript_module_candidates(file_path: &str, module_path: &str) -> Vec<String> {
    if !module_path.starts_with('.') && !module_path.starts_with('/') {
        return Vec::new();
    }

    let base = if module_path.starts_with('/') {
        PathBuf::from(module_path.trim_start_matches('/'))
    } else {
        let parent = Path::new(file_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        parent.join(module_path)
    }
    .clean();

    let mut candidates = if base.extension().is_some() {
        vec![base]
    } else {
        vec![
            base.with_extension("ts"),
            base.with_extension("tsx"),
            base.with_extension("js"),
            base.with_extension("jsx"),
            base.with_extension("mjs"),
            base.with_extension("cjs"),
            base.join("index.ts"),
            base.join("index.tsx"),
            base.join("index.js"),
            base.join("index.jsx"),
            base.join("index.mjs"),
            base.join("index.cjs"),
        ]
    };

    normalize_candidates(&mut candidates)
}

fn resolve_python_module_candidates(file_path: &str, module_path: &str) -> Vec<String> {
    let (relative_levels, remainder) = split_python_module_path(module_path);
    let mut bases = Vec::new();

    if relative_levels > 0 {
        let current_dir = Path::new(file_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        bases.push(ascend_dir(current_dir, relative_levels.saturating_sub(1)));
    } else {
        if let Some(parent) = Path::new(file_path).parent() {
            if let Some(first_component) = parent.components().next() {
                let candidate = PathBuf::from(first_component.as_os_str());
                if !candidate.as_os_str().is_empty() {
                    bases.push(candidate);
                }
            }
        }
        bases.push(PathBuf::new());
    }

    let mut candidates = Vec::new();
    for base in bases {
        let mut module_base = base;
        if !remainder.is_empty() {
            module_base = module_base.join(remainder.replace("::", "/"));
        }
        candidates.push(module_base.with_extension("py"));
        candidates.push(module_base.join("__init__.py"));
    }

    normalize_candidates(&mut candidates)
}

fn include_default_extensions(language: Language) -> &'static [&'static str] {
    match language {
        Language::C => &["h", "c"],
        Language::Cpp => &["h", "hh", "hpp", "hxx", "cc", "cpp", "cxx"],
        Language::Objc => &["h", "m"],
        _ => &[],
    }
}

fn include_companion_extensions(language: Language) -> &'static [&'static str] {
    match language {
        Language::C => &["h", "c"],
        Language::Cpp => &["h", "hh", "hpp", "hxx", "cc", "cpp", "cxx"],
        Language::Objc => &["h", "m"],
        _ => &[],
    }
}

fn split_python_module_path(module_path: &str) -> (usize, &str) {
    let mut levels = 0;
    let mut remainder = module_path;

    while let Some(rest) = remainder.strip_prefix("::") {
        levels += 1;
        remainder = rest;
    }

    (levels, remainder)
}

fn ascend_dir(path: &Path, levels: usize) -> PathBuf {
    let mut current = path.to_path_buf();
    for _ in 0..levels {
        current.pop();
    }
    current
}

fn normalize_candidates(candidates: &mut Vec<PathBuf>) -> Vec<String> {
    let mut normalized = candidates
        .drain(..)
        .map(|path| path.clean().to_string_lossy().replace('\\', "/"))
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn synthetic_symbol(
    target: &str,
    source: SymbolSource,
    dependency_kind: &DependencyKind,
) -> Symbol {
    let file_path = match source {
        SymbolSource::Builtin => BUILTIN_SYMBOL_FILE_PATH,
        SymbolSource::External => EXTERNAL_SYMBOL_FILE_PATH,
        SymbolSource::Local => unreachable!("synthetic symbols must be non-local"),
    };
    let name = target
        .rsplit("::")
        .find(|segment| !segment.is_empty())
        .unwrap_or(target)
        .to_string();

    Symbol::new(
        name,
        format!("{file_path}::{target}"),
        synthetic_symbol_kind(dependency_kind),
        file_path.to_string(),
        1,
        1,
    )
    .with_source(source)
}

fn synthetic_symbol_kind(dependency_kind: &DependencyKind) -> SymbolKind {
    match dependency_kind {
        DependencyKind::Call => SymbolKind::Function,
        DependencyKind::Inherit => SymbolKind::Class,
        DependencyKind::Implement => SymbolKind::Interface,
        DependencyKind::TypeUse => SymbolKind::TypeAlias,
        DependencyKind::Import => SymbolKind::Module,
    }
}

fn builtin_target(raw_target: &str, language: Option<Language>) -> Option<String> {
    let target = raw_target.trim();
    let bare_target = target.trim_end_matches('!');

    let is_builtin = match language.unwrap_or(Language::Rust) {
        Language::Rust => matches!(
            target,
            "assert!"
                | "assert_eq!"
                | "assert_ne!"
                | "dbg!"
                | "eprint!"
                | "eprintln!"
                | "format!"
                | "panic!"
                | "print!"
                | "println!"
                | "todo!"
                | "unimplemented!"
                | "vec!"
                | "write!"
                | "writeln!"
        ),
        Language::Python => matches!(
            bare_target,
            "bool"
                | "dict"
                | "enumerate"
                | "filter"
                | "float"
                | "int"
                | "len"
                | "list"
                | "map"
                | "max"
                | "min"
                | "open"
                | "print"
                | "range"
                | "set"
                | "sorted"
                | "str"
                | "sum"
                | "super"
                | "tuple"
                | "zip"
        ),
        Language::TypeScript | Language::JavaScript => matches!(
            bare_target,
            "Array"
                | "BigInt"
                | "Boolean"
                | "Date"
                | "JSON"
                | "Map"
                | "Math"
                | "Number"
                | "Promise"
                | "RegExp"
                | "Set"
                | "String"
                | "clearInterval"
                | "clearTimeout"
                | "parseFloat"
                | "parseInt"
                | "queueMicrotask"
                | "setInterval"
                | "setTimeout"
        ),
        Language::Java => matches!(
            bare_target,
            "Boolean"
                | "Byte"
                | "Class"
                | "Double"
                | "Exception"
                | "Float"
                | "Integer"
                | "Long"
                | "Math"
                | "Object"
                | "RuntimeException"
                | "Short"
                | "String"
                | "StringBuilder"
                | "StringBuffer"
                | "System"
                | "Thread"
        ),
        Language::CSharp => matches!(
            bare_target,
            "Array"
                | "Boolean"
                | "Console"
                | "DateTime"
                | "Decimal"
                | "Double"
                | "Exception"
                | "Guid"
                | "Int32"
                | "Int64"
                | "Math"
                | "Object"
                | "String"
                | "StringBuilder"
                | "Task"
                | "ValueTask"
        ),
        Language::Kotlin => matches!(
            bare_target,
            "Any"
                | "Array"
                | "Boolean"
                | "Double"
                | "Float"
                | "Int"
                | "Long"
                | "Nothing"
                | "String"
                | "Unit"
                | "check"
                | "emptyList"
                | "emptyMap"
                | "emptySet"
                | "listOf"
                | "mapOf"
                | "print"
                | "println"
                | "require"
                | "runCatching"
                | "setOf"
        ),
        Language::Php => matches!(
            bare_target,
            "array_filter"
                | "array_map"
                | "array_merge"
                | "array_reduce"
                | "count"
                | "explode"
                | "implode"
                | "in_array"
                | "is_array"
                | "is_null"
                | "json_decode"
                | "json_encode"
                | "sprintf"
                | "strlen"
                | "strtolower"
                | "strtoupper"
                | "substr"
                | "trim"
        ),
        Language::Ruby => matches!(
            bare_target,
            "Array" | "File" | "Hash" | "String" | "Time" | "p" | "pp" | "print" | "puts"
        ),
        Language::Swift => matches!(
            bare_target,
            "Any"
                | "Array"
                | "Bool"
                | "Dictionary"
                | "Double"
                | "Error"
                | "Float"
                | "Int"
                | "Never"
                | "Optional"
                | "Result"
                | "Set"
                | "String"
                | "Task"
                | "UInt"
                | "fatalError"
                | "print"
        ),
        Language::Objc => matches!(bare_target, "NSLog"),
        Language::Go => matches!(
            bare_target,
            "append"
                | "cap"
                | "clear"
                | "close"
                | "complex"
                | "copy"
                | "delete"
                | "imag"
                | "len"
                | "make"
                | "max"
                | "min"
                | "new"
                | "panic"
                | "print"
                | "println"
                | "real"
                | "recover"
        ),
        Language::C | Language::Cpp => false,
    };

    is_builtin.then(|| target.to_string())
}

fn resolve_external_script_target(
    imports: &[Import],
    raw_target: &str,
    language: Language,
) -> Option<String> {
    let bare_target = raw_target
        .trim_end_matches('!')
        .rsplit(['.', ':'])
        .find(|segment| !segment.is_empty())
        .unwrap_or_default();

    if bare_target.is_empty() {
        return None;
    }

    let mut exact_matches = imports
        .iter()
        .filter(|import| is_external_script_import(import, language))
        .filter_map(|import| {
            let bound_name = import
                .alias
                .as_deref()
                .or_else(|| import.effective_name())?;
            (bound_name == bare_target).then(|| import.source.clone())
        })
        .collect::<Vec<_>>();
    exact_matches.sort();
    exact_matches.dedup();
    if exact_matches.len() == 1 {
        return exact_matches.into_iter().next();
    }

    let mut glob_matches = imports
        .iter()
        .filter(|import| import.kind == ImportKind::Glob)
        .filter(|import| is_external_script_import(import, language))
        .map(|import| format!("{}::{bare_target}", import.source))
        .collect::<Vec<_>>();
    glob_matches.sort();
    glob_matches.dedup();
    if glob_matches.len() == 1 {
        return glob_matches.into_iter().next();
    }

    None
}

fn is_external_script_import(import: &Import, language: Language) -> bool {
    match language {
        Language::TypeScript | Language::JavaScript => {
            !import.source.starts_with('.') && !import.source.starts_with('/')
        }
        Language::Python => !import.source.starts_with("::"),
        Language::Go => !import.source.is_empty(),
        Language::Java
        | Language::CSharp
        | Language::Kotlin
        | Language::Php
        | Language::Ruby
        | Language::Swift
        | Language::Objc
        | Language::C
        | Language::Cpp => false,
        Language::Rust => false,
    }
}

fn is_external_rust_path(path: &str) -> bool {
    !matches!(path.split("::").next(), Some("crate" | "self" | "super"))
}

fn is_known_rust_external_target(target: &str) -> bool {
    matches!(target.split("::").next(), Some("std" | "core" | "alloc"))
}

fn rust_symbol_suffix(symbol: &Symbol) -> Option<String> {
    let prefix = format!("{}::", symbol.file_path);
    symbol
        .qualified_name
        .strip_prefix(&prefix)
        .map(ToString::to_string)
}

fn rust_owner_suffix_from_qualified_name(qualified_name: &str) -> Option<String> {
    let (_, remainder) = qualified_name.split_once("::")?;
    let (owner, _) = remainder.rsplit_once("::")?;
    Some(owner.to_string())
}

fn rust_parameter_type_hints(signature: &str) -> HashMap<String, String> {
    let mut hints = HashMap::new();
    let Some(parameters) = outer_parameter_list(signature) else {
        return hints;
    };

    for parameter in split_top_level(parameters, ',') {
        let parameter = parameter.trim();
        if parameter.is_empty() || parameter.contains("self") {
            continue;
        }

        let Some((binding, ty)) = parameter.rsplit_once(':') else {
            continue;
        };
        let Some(binding_name) = extract_rust_binding_name(binding) else {
            continue;
        };
        let Some(type_name) = extract_primary_rust_type_name(ty) else {
            continue;
        };

        hints.insert(binding_name, type_name);
    }

    hints
}

fn outer_parameter_list(signature: &str) -> Option<&str> {
    let start = signature.find('(')?;
    let mut depth = 0_usize;

    for (index, ch) in signature[start..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = start + index;
                    return signature.get(start + 1..end);
                }
            }
            _ => {}
        }
    }

    None
}

fn split_top_level(input: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0_usize;
    let mut paren_depth = 0_usize;
    let mut bracket_depth = 0_usize;
    let mut brace_depth = 0_usize;
    let mut angle_depth = 0_usize;

    for (index, ch) in input.char_indices() {
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            _ => {}
        }

        if ch == separator
            && paren_depth == 0
            && bracket_depth == 0
            && brace_depth == 0
            && angle_depth == 0
        {
            parts.push(&input[start..index]);
            start = index + ch.len_utf8();
        }
    }

    parts.push(&input[start..]);
    parts
}

fn extract_rust_binding_name(binding: &str) -> Option<String> {
    binding
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .rfind(|token| !token.is_empty() && !matches!(*token, "mut" | "ref" | "self" | "_"))
        .map(ToString::to_string)
}

fn extract_primary_rust_type_name(type_text: &str) -> Option<String> {
    type_text
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .rfind(|token| {
            !token.is_empty()
                && !token.starts_with('\'')
                && !matches!(
                    *token,
                    "dyn" | "impl" | "mut" | "const" | "fn" | "where" | "for"
                )
                && token
                    .chars()
                    .next()
                    .map(|ch| ch.is_ascii_uppercase())
                    .unwrap_or(false)
        })
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{DependencyKind, SymbolKind, SymbolSource};
    use crate::resolver::ImportKind;

    fn make_symbol(
        id: &str,
        qualified_name: &str,
        file_path: &str,
        name: &str,
        kind: SymbolKind,
    ) -> Symbol {
        let mut symbol = Symbol::new(
            name.to_string(),
            qualified_name.to_string(),
            kind,
            file_path.to_string(),
            1,
            1,
        );
        symbol.id = id.to_string();
        symbol
    }

    fn make_symbol_with_signature(
        id: &str,
        qualified_name: &str,
        file_path: &str,
        name: &str,
        kind: SymbolKind,
        signature: &str,
    ) -> Symbol {
        let mut symbol = make_symbol(id, qualified_name, file_path, name, kind);
        symbol.signature = Some(signature.to_string());
        symbol
    }

    fn make_raw_dependency(from: &str, to: &str, file_path: &str) -> Dependency {
        Dependency::new(
            from.to_string(),
            to.to_string(),
            file_path.to_string(),
            10,
            DependencyKind::Call,
        )
    }

    #[test]
    fn test_resolve_same_file_bare_name() {
        let symbols = vec![
            make_symbol(
                "caller",
                "src/utils.rs::run",
                "src/utils.rs",
                "run",
                SymbolKind::Function,
            ),
            make_symbol(
                "callee",
                "src/utils.rs::helper",
                "src/utils.rs",
                "helper",
                SymbolKind::Function,
            ),
        ];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/utils.rs",
            &[],
            &[make_raw_dependency(
                "src/utils.rs::run",
                "helper",
                "src/utils.rs",
            )],
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].from_symbol, "caller");
        assert_eq!(summary.resolved[0].to_symbol, "callee");
    }

    #[test]
    fn test_resolve_alias_import_method_call() {
        let symbols = vec![
            make_symbol(
                "main",
                "src/main.rs::main",
                "src/main.rs",
                "main",
                SymbolKind::Function,
            ),
            make_symbol(
                "user-new",
                "src/models.rs::User::new",
                "src/models.rs",
                "new",
                SymbolKind::Method,
            ),
        ];
        let imports = vec![Import::new(
            "crate::models::User".to_string(),
            "src/main.rs".to_string(),
            1,
            ImportKind::Alias,
        )
        .with_alias("Person".to_string())];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/main.rs",
            &imports,
            &[make_raw_dependency(
                "src/main.rs::main",
                "Person::new",
                "src/main.rs",
            )],
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "user-new");
    }

    #[test]
    fn test_resolve_glob_import_bare_name() {
        let symbols = vec![
            make_symbol(
                "main",
                "src/main.rs::main",
                "src/main.rs",
                "main",
                SymbolKind::Function,
            ),
            make_symbol(
                "calculate",
                "src/utils.rs::calculate",
                "src/utils.rs",
                "calculate",
                SymbolKind::Function,
            ),
        ];
        let imports = vec![Import::new(
            "crate::utils".to_string(),
            "src/main.rs".to_string(),
            1,
            ImportKind::Glob,
        )];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/main.rs",
            &imports,
            &[make_raw_dependency(
                "src/main.rs::main",
                "calculate",
                "src/main.rs",
            )],
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "calculate");
    }

    #[test]
    fn test_resolve_scoped_module_path_without_import() {
        let symbols = vec![
            make_symbol(
                "main",
                "src/main.rs::main",
                "src/main.rs",
                "main",
                SymbolKind::Function,
            ),
            make_symbol(
                "user-new",
                "src/models.rs::User::new",
                "src/models.rs",
                "new",
                SymbolKind::Method,
            ),
        ];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/main.rs",
            &[],
            &[make_raw_dependency(
                "src/main.rs::main",
                "models::User::new",
                "src/main.rs",
            )],
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "user-new");
    }

    #[test]
    fn test_resolve_self_method_call() {
        let symbols = vec![
            make_symbol(
                "with-age",
                "src/models.rs::User::with_age",
                "src/models.rs",
                "with_age",
                SymbolKind::Method,
            ),
            make_symbol(
                "user-new",
                "src/models.rs::User::new",
                "src/models.rs",
                "new",
                SymbolKind::Method,
            ),
        ];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/models.rs",
            &[],
            &[make_raw_dependency(
                "src/models.rs::User::with_age",
                "Self::new",
                "src/models.rs",
            )],
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "user-new");
    }

    #[test]
    fn test_resolve_self_import_alias_prefix() {
        let symbols = vec![
            make_symbol(
                "main",
                "src/main.rs::main",
                "src/main.rs",
                "main",
                SymbolKind::Function,
            ),
            make_symbol(
                "user-new",
                "src/models.rs::User::new",
                "src/models.rs",
                "new",
                SymbolKind::Method,
            ),
        ];
        let imports = vec![Import::new(
            "crate::models".to_string(),
            "src/main.rs".to_string(),
            1,
            ImportKind::SelfImport,
        )
        .with_alias("model_types".to_string())];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/main.rs",
            &imports,
            &[make_raw_dependency(
                "src/main.rs::main",
                "model_types::User::new",
                "src/main.rs",
            )],
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "user-new");
    }

    #[test]
    fn test_resolve_rust_receiver_chain_from_self_field() {
        let symbols = vec![
            make_symbol_with_signature(
                "runtime-next",
                "src/flow.rs::RuntimeCore::next_conflict_queue_head",
                "src/flow.rs",
                "next_conflict_queue_head",
                SymbolKind::Method,
                "(&self, concurrency_key: &str)",
            ),
            make_symbol_with_signature(
                "runtime-executions",
                "src/lib.rs::RuntimeCore::executions",
                "src/lib.rs",
                "executions",
                SymbolKind::Property,
                "ExecutionService",
            ),
            make_symbol_with_signature(
                "execution-next",
                "src/execution.rs::ExecutionService::next_conflict_queue_head",
                "src/execution.rs",
                "next_conflict_queue_head",
                SymbolKind::Method,
                "(&self, store: &Store, scheduler: &Scheduler, concurrency_key: &str)",
            ),
        ];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/flow.rs",
            &[],
            &[make_raw_dependency(
                "src/flow.rs::RuntimeCore::next_conflict_queue_head",
                "self.executions.next_conflict_queue_head",
                "src/flow.rs",
            )],
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "execution-next");
    }

    #[test]
    fn test_resolve_rust_receiver_chain_from_typed_parameter_field() {
        let symbols = vec![
            make_symbol_with_signature(
                "process-turn",
                "src/core_flow_service.rs::CoreFlowService::process_turn",
                "src/core_flow_service.rs",
                "process_turn",
                SymbolKind::Method,
                "(&self, core: &RuntimeCore, execution: &ExecutionRecord)",
            ),
            make_symbol_with_signature(
                "runtime-verifications",
                "src/lib.rs::RuntimeCore::verifications",
                "src/lib.rs",
                "verifications",
                SymbolKind::Property,
                "VerificationService",
            ),
            make_symbol_with_signature(
                "verify-pre-dispatch",
                "src/runtime_verification.rs::VerificationService::verify_pre_dispatch",
                "src/runtime_verification.rs",
                "verify_pre_dispatch",
                SymbolKind::Method,
                "(&self, store: &Store, execution: &ExecutionRecord, action_spec: &ActionSpec)",
            ),
        ];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/core_flow_service.rs",
            &[],
            &[make_raw_dependency(
                "src/core_flow_service.rs::CoreFlowService::process_turn",
                "core.verifications.verify_pre_dispatch",
                "src/core_flow_service.rs",
            )],
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "verify-pre-dispatch");
    }

    #[test]
    fn test_resolve_rust_receiver_method_from_typed_parameter() {
        let symbols = vec![
            make_symbol_with_signature(
                "execution-next",
                "src/execution.rs::ExecutionService::next_conflict_queue_head",
                "src/execution.rs",
                "next_conflict_queue_head",
                SymbolKind::Method,
                "(&self, scheduler: &Scheduler, concurrency_key: &str)",
            ),
            make_symbol_with_signature(
                "scheduler-head",
                "src/scheduler.rs::Scheduler::dispatchable_head",
                "src/scheduler.rs",
                "dispatchable_head",
                SymbolKind::Method,
                "(&self, candidates: &[ExecutionRecord])",
            ),
        ];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/execution.rs",
            &[],
            &[make_raw_dependency(
                "src/execution.rs::ExecutionService::next_conflict_queue_head",
                "scheduler.dispatchable_head",
                "src/execution.rs",
            )],
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "scheduler-head");
    }

    #[test]
    fn test_resolve_rust_method_suffix_without_module_path() {
        let symbols = vec![
            make_symbol(
                "main",
                "src/main.rs::main",
                "src/main.rs",
                "main",
                SymbolKind::Function,
            ),
            make_symbol_with_signature(
                "scheduler-head",
                "src/scheduler.rs::Scheduler::dispatchable_head",
                "src/scheduler.rs",
                "dispatchable_head",
                SymbolKind::Method,
                "(&self, candidates: &[ExecutionRecord])",
            ),
        ];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/main.rs",
            &[],
            &[make_raw_dependency(
                "src/main.rs::main",
                "Scheduler::dispatchable_head",
                "src/main.rs",
            )],
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "scheduler-head");
    }

    #[test]
    fn test_collect_unresolved_dependencies() {
        let symbols = vec![make_symbol(
            "main",
            "src/main.rs::main",
            "src/main.rs",
            "main",
            SymbolKind::Function,
        )];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/main.rs",
            &[],
            &[make_raw_dependency(
                "src/main.rs::main",
                "missing_helper",
                "src/main.rs",
            )],
        );

        assert!(summary.resolved.is_empty());
        assert!(summary.non_local_symbols.is_empty());
        assert_eq!(
            summary.unresolved,
            vec![UnresolvedDependency {
                from_qualified_name: "src/main.rs::main".to_string(),
                target: "missing_helper".to_string(),
                from_file: "src/main.rs".to_string(),
                from_line: 10,
            }]
        );
    }

    #[test]
    fn test_mark_builtin_macro_as_builtin_symbol() {
        let symbols = vec![make_symbol(
            "main",
            "src/main.rs::main",
            "src/main.rs",
            "main",
            SymbolKind::Function,
        )];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/main.rs",
            &[],
            &[make_raw_dependency(
                "src/main.rs::main",
                "println!",
                "src/main.rs",
            )],
        );

        assert!(summary.unresolved.is_empty());
        assert_eq!(summary.resolved.len(), 1);
        assert_eq!(summary.non_local_symbols.len(), 1);
        assert_eq!(summary.non_local_symbols[0].source, SymbolSource::Builtin);
        assert_eq!(
            summary.non_local_symbols[0].qualified_name,
            "<builtin>::println!"
        );
        assert_eq!(
            summary.resolved[0].to_symbol,
            summary.non_local_symbols[0].id
        );
    }

    #[test]
    fn test_mark_rust_std_symbols_as_external() {
        let symbols = vec![make_symbol(
            "main",
            "src/main.rs::main",
            "src/main.rs",
            "main",
            SymbolKind::Function,
        )];
        let imports = vec![Import::new(
            "std::collections::HashMap".to_string(),
            "src/main.rs".to_string(),
            1,
            ImportKind::Named,
        )];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/main.rs",
            &imports,
            &[make_raw_dependency(
                "src/main.rs::main",
                "HashMap::new",
                "src/main.rs",
            )],
        );

        assert!(summary.unresolved.is_empty());
        assert_eq!(summary.resolved.len(), 1);
        assert_eq!(summary.non_local_symbols.len(), 1);
        assert_eq!(summary.non_local_symbols[0].source, SymbolSource::External);
        assert_eq!(
            summary.non_local_symbols[0].qualified_name,
            "<external>::std::collections::HashMap::new"
        );
        assert_eq!(
            summary.resolved[0].to_symbol,
            summary.non_local_symbols[0].id
        );
    }

    #[test]
    fn test_mark_typescript_package_imports_as_external() {
        let symbols = vec![make_symbol(
            "ts-run",
            "src/main.ts::run",
            "src/main.ts",
            "run",
            SymbolKind::Function,
        )];
        let imports = vec![Import::new(
            "lodash::mapValues".to_string(),
            "src/main.ts".to_string(),
            1,
            ImportKind::Named,
        )];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/main.ts",
            &imports,
            &[make_raw_dependency(
                "src/main.ts::run",
                "mapValues",
                "src/main.ts",
            )],
            Some(Language::TypeScript),
        );

        assert!(summary.unresolved.is_empty());
        assert_eq!(summary.non_local_symbols.len(), 1);
        assert_eq!(summary.non_local_symbols[0].source, SymbolSource::External);
        assert_eq!(
            summary.non_local_symbols[0].qualified_name,
            "<external>::lodash::mapValues"
        );
    }

    #[test]
    fn test_resolve_typescript_named_import_alias() {
        let symbols = vec![
            make_symbol(
                "ts-run",
                "src/main.ts::run",
                "src/main.ts",
                "run",
                SymbolKind::Function,
            ),
            make_symbol(
                "ts-format",
                "src/shared.ts::formatName",
                "src/shared.ts",
                "formatName",
                SymbolKind::Function,
            ),
        ];
        let imports = vec![Import::new(
            "./shared::formatName".to_string(),
            "src/main.ts".to_string(),
            1,
            ImportKind::Alias,
        )
        .with_alias("format".to_string())];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/main.ts",
            &imports,
            &[make_raw_dependency(
                "src/main.ts::run",
                "format",
                "src/main.ts",
            )],
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "ts-format");
    }

    #[test]
    fn test_resolve_typescript_glob_import() {
        let symbols = vec![
            make_symbol(
                "ts-run",
                "src/main.ts::run",
                "src/main.ts",
                "run",
                SymbolKind::Function,
            ),
            make_symbol(
                "ts-format",
                "src/shared.ts::formatName",
                "src/shared.ts",
                "formatName",
                SymbolKind::Function,
            ),
        ];
        let imports = vec![Import::new(
            "./shared".to_string(),
            "src/main.ts".to_string(),
            1,
            ImportKind::Glob,
        )];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/main.ts",
            &imports,
            &[make_raw_dependency(
                "src/main.ts::run",
                "formatName",
                "src/main.ts",
            )],
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "ts-format");
    }

    #[test]
    fn test_resolve_typescript_named_reexport() {
        let symbols = vec![
            make_symbol(
                "ts-run",
                "src/main.ts::run",
                "src/main.ts",
                "run",
                SymbolKind::Function,
            ),
            make_symbol(
                "ts-helper",
                "src/impl.ts::helper",
                "src/impl.ts",
                "helper",
                SymbolKind::Function,
            ),
            make_symbol(
                "other-helper",
                "src/other.ts::helper",
                "src/other.ts",
                "helper",
                SymbolKind::Function,
            ),
        ];
        let all_imports = vec![
            Import::new(
                "./barrel::helper".to_string(),
                "src/main.ts".to_string(),
                1,
                ImportKind::Named,
            ),
            Import::new(
                "./impl::helper".to_string(),
                "src/barrel.ts".to_string(),
                1,
                ImportKind::ReExportNamed,
            ),
        ];
        let main_imports = vec![all_imports[0].clone()];

        let resolver = Resolver::new_with_imports(&symbols, &all_imports);
        let summary = resolver.resolve_dependencies_with_language(
            "src/main.ts",
            &main_imports,
            &[make_raw_dependency(
                "src/main.ts::run",
                "helper",
                "src/main.ts",
            )],
            Some(Language::TypeScript),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "ts-helper");
    }

    #[test]
    fn test_resolve_typescript_glob_reexport() {
        let symbols = vec![
            make_symbol(
                "ts-run",
                "src/main.ts::run",
                "src/main.ts",
                "run",
                SymbolKind::Function,
            ),
            make_symbol(
                "ts-helper",
                "src/impl.ts::helper",
                "src/impl.ts",
                "helper",
                SymbolKind::Function,
            ),
        ];
        let all_imports = vec![
            Import::new(
                "./barrel".to_string(),
                "src/main.ts".to_string(),
                1,
                ImportKind::Glob,
            ),
            Import::new(
                "./impl".to_string(),
                "src/barrel.ts".to_string(),
                1,
                ImportKind::ReExportGlob,
            ),
        ];
        let main_imports = vec![all_imports[0].clone()];

        let resolver = Resolver::new_with_imports(&symbols, &all_imports);
        let summary = resolver.resolve_dependencies_with_language(
            "src/main.ts",
            &main_imports,
            &[make_raw_dependency(
                "src/main.ts::run",
                "helper",
                "src/main.ts",
            )],
            Some(Language::TypeScript),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "ts-helper");
    }

    #[test]
    fn test_resolve_javascript_named_import_alias() {
        let symbols = vec![
            make_symbol(
                "js-run",
                "src/main.js::run",
                "src/main.js",
                "run",
                SymbolKind::Function,
            ),
            make_symbol(
                "js-format",
                "src/shared.js::formatName",
                "src/shared.js",
                "formatName",
                SymbolKind::Function,
            ),
        ];
        let imports = vec![Import::new(
            "./shared::formatName".to_string(),
            "src/main.js".to_string(),
            1,
            ImportKind::Alias,
        )
        .with_alias("format".to_string())];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/main.js",
            &imports,
            &[make_raw_dependency(
                "src/main.js::run",
                "format",
                "src/main.js",
            )],
            Some(Language::JavaScript),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "js-format");
    }

    #[test]
    fn test_resolve_javascript_module_candidates_cover_index_and_js_extensions() {
        let candidates = resolve_typescript_module_candidates("src/main.js", "./shared");
        assert!(candidates.contains(&"src/shared.js".to_string()));
        assert!(candidates.contains(&"src/shared.jsx".to_string()));
        assert!(candidates.contains(&"src/shared/index.js".to_string()));
        assert!(candidates.contains(&"src/shared/index.cjs".to_string()));
    }

    #[test]
    fn test_resolve_java_explicit_import() {
        let symbols = vec![
            make_symbol(
                "java-run",
                "src/main/java/com/example/service/App.java::App::run",
                "src/main/java/com/example/service/App.java",
                "run",
                SymbolKind::Method,
            ),
            make_symbol(
                "java-user",
                "src/main/java/com/example/auth/User.java::User",
                "src/main/java/com/example/auth/User.java",
                "User",
                SymbolKind::Class,
            ),
        ];
        let imports = vec![Import::new(
            "com.example.auth.User".to_string(),
            "src/main/java/com/example/service/App.java".to_string(),
            1,
            ImportKind::Named,
        )];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/main/java/com/example/service/App.java",
            &imports,
            &[make_raw_dependency(
                "src/main/java/com/example/service/App.java::App::run",
                "User",
                "src/main/java/com/example/service/App.java",
            )],
            Some(Language::Java),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "java-user");
    }

    #[test]
    fn test_resolve_java_same_package_type_without_import() {
        let symbols = vec![
            make_symbol(
                "java-run",
                "src/main/java/com/example/service/App.java::App::run",
                "src/main/java/com/example/service/App.java",
                "run",
                SymbolKind::Method,
            ),
            make_symbol(
                "java-helper",
                "src/main/java/com/example/service/Helper.java::Helper",
                "src/main/java/com/example/service/Helper.java",
                "Helper",
                SymbolKind::Class,
            ),
        ];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/main/java/com/example/service/App.java",
            &[],
            &[make_raw_dependency(
                "src/main/java/com/example/service/App.java::App::run",
                "Helper",
                "src/main/java/com/example/service/App.java",
            )],
            Some(Language::Java),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "java-helper");
    }

    #[test]
    fn test_resolve_java_static_import_member() {
        let symbols = vec![
            make_symbol(
                "java-run",
                "src/main/java/com/example/service/App.java::App::run",
                "src/main/java/com/example/service/App.java",
                "run",
                SymbolKind::Method,
            ),
            make_symbol(
                "java-math",
                "src/main/java/com/example/util/MathUtil.java::MathUtil",
                "src/main/java/com/example/util/MathUtil.java",
                "MathUtil",
                SymbolKind::Class,
            ),
            make_symbol(
                "java-max",
                "src/main/java/com/example/util/MathUtil.java::MathUtil::max",
                "src/main/java/com/example/util/MathUtil.java",
                "max",
                SymbolKind::Method,
            ),
        ];
        let imports = vec![Import::new(
            "com.example.util.MathUtil::max".to_string(),
            "src/main/java/com/example/service/App.java".to_string(),
            1,
            ImportKind::Named,
        )];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/main/java/com/example/service/App.java",
            &imports,
            &[make_raw_dependency(
                "src/main/java/com/example/service/App.java::App::run",
                "max",
                "src/main/java/com/example/service/App.java",
            )],
            Some(Language::Java),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "java-max");
    }

    #[test]
    fn test_resolve_csharp_namespace_using() {
        let symbols = vec![
            make_symbol(
                "cs-run",
                "src/App.cs::App::Run",
                "src/App.cs",
                "Run",
                SymbolKind::Method,
            ),
            make_symbol(
                "cs-helper",
                "src/Acme/Shared/Helper.cs::Helper",
                "src/Acme/Shared/Helper.cs",
                "Helper",
                SymbolKind::Class,
            ),
        ];
        let imports = vec![Import::new(
            "Acme.Shared".to_string(),
            "src/App.cs".to_string(),
            1,
            ImportKind::Glob,
        )];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/App.cs",
            &imports,
            &[make_raw_dependency(
                "src/App.cs::App::Run",
                "Helper",
                "src/App.cs",
            )],
            Some(Language::CSharp),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "cs-helper");
    }

    #[test]
    fn test_resolve_csharp_using_alias() {
        let symbols = vec![
            make_symbol(
                "cs-run",
                "src/App.cs::App::Run",
                "src/App.cs",
                "Run",
                SymbolKind::Method,
            ),
            make_symbol(
                "cs-helper",
                "src/Acme/Shared/Helper.cs::Helper",
                "src/Acme/Shared/Helper.cs",
                "Helper",
                SymbolKind::Class,
            ),
        ];
        let imports = vec![Import::new(
            "Acme.Shared.Helper".to_string(),
            "src/App.cs".to_string(),
            1,
            ImportKind::Alias,
        )
        .with_alias("SharedHelper".to_string())];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/App.cs",
            &imports,
            &[make_raw_dependency(
                "src/App.cs::App::Run",
                "SharedHelper",
                "src/App.cs",
            )],
            Some(Language::CSharp),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "cs-helper");
    }

    #[test]
    fn test_resolve_kotlin_glob_import() {
        let symbols = vec![
            make_symbol(
                "kt-run",
                "src/acme/app/App.kt::App::run",
                "src/acme/app/App.kt",
                "run",
                SymbolKind::Method,
            ),
            make_symbol(
                "kt-helper",
                "src/acme/shared/Helper.kt::Helper",
                "src/acme/shared/Helper.kt",
                "Helper",
                SymbolKind::Class,
            ),
        ];
        let imports = vec![Import::new(
            "acme.shared".to_string(),
            "src/acme/app/App.kt".to_string(),
            1,
            ImportKind::Glob,
        )];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/acme/app/App.kt",
            &imports,
            &[make_raw_dependency(
                "src/acme/app/App.kt::App::run",
                "Helper",
                "src/acme/app/App.kt",
            )],
            Some(Language::Kotlin),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "kt-helper");
    }

    #[test]
    fn test_resolve_kotlin_import_alias() {
        let symbols = vec![
            make_symbol(
                "kt-run",
                "src/acme/app/App.kt::App::run",
                "src/acme/app/App.kt",
                "run",
                SymbolKind::Method,
            ),
            make_symbol(
                "kt-helper",
                "src/acme/shared/Helper.kt::Helper",
                "src/acme/shared/Helper.kt",
                "Helper",
                SymbolKind::Class,
            ),
        ];
        let imports = vec![Import::new(
            "acme.shared.Helper".to_string(),
            "src/acme/app/App.kt".to_string(),
            1,
            ImportKind::Alias,
        )
        .with_alias("SharedHelper".to_string())];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/acme/app/App.kt",
            &imports,
            &[make_raw_dependency(
                "src/acme/app/App.kt::App::run",
                "SharedHelper",
                "src/acme/app/App.kt",
            )],
            Some(Language::Kotlin),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "kt-helper");
    }

    #[test]
    fn test_resolve_php_import_alias() {
        let symbols = vec![
            make_symbol(
                "php-run",
                "src/Acme/App/App.php::App::run",
                "src/Acme/App/App.php",
                "run",
                SymbolKind::Method,
            ),
            make_symbol(
                "php-helper",
                "src/Acme/Shared/Helper.php::Helper",
                "src/Acme/Shared/Helper.php",
                "Helper",
                SymbolKind::Class,
            ),
        ];
        let imports = vec![Import::new(
            "Acme\\Shared\\Helper".to_string(),
            "src/Acme/App/App.php".to_string(),
            1,
            ImportKind::Alias,
        )
        .with_alias("SharedHelper".to_string())];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/Acme/App/App.php",
            &imports,
            &[make_raw_dependency(
                "src/Acme/App/App.php::App::run",
                "SharedHelper",
                "src/Acme/App/App.php",
            )],
            Some(Language::Php),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "php-helper");
    }

    #[test]
    fn test_resolve_php_function_import() {
        let symbols = vec![
            make_symbol(
                "php-run",
                "src/Acme/App/App.php::App::run",
                "src/Acme/App/App.php",
                "run",
                SymbolKind::Method,
            ),
            make_symbol(
                "php-format",
                "src/Acme/Shared/functions.php::format_name",
                "src/Acme/Shared/functions.php",
                "format_name",
                SymbolKind::Function,
            ),
        ];
        let imports = vec![Import::new(
            "Acme\\Shared\\format_name".to_string(),
            "src/Acme/App/App.php".to_string(),
            1,
            ImportKind::Named,
        )];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/Acme/App/App.php",
            &imports,
            &[make_raw_dependency(
                "src/Acme/App/App.php::App::run",
                "format_name",
                "src/Acme/App/App.php",
            )],
            Some(Language::Php),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "php-format");
    }

    #[test]
    fn test_resolve_ruby_require_relative_type_use() {
        let symbols = vec![
            make_symbol(
                "ruby-run",
                "lib/user_service.rb::UserService::greet",
                "lib/user_service.rb",
                "greet",
                SymbolKind::Method,
            ),
            make_symbol(
                "ruby-helper",
                "lib/shared/helper.rb::Helper",
                "lib/shared/helper.rb",
                "Helper",
                SymbolKind::Class,
            ),
        ];
        let imports = vec![Import::new(
            "./shared/helper".to_string(),
            "lib/user_service.rb".to_string(),
            1,
            ImportKind::Glob,
        )];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "lib/user_service.rb",
            &imports,
            &[Dependency::new(
                "lib/user_service.rb::UserService::greet".to_string(),
                "Helper".to_string(),
                "lib/user_service.rb".to_string(),
                10,
                DependencyKind::TypeUse,
            )],
            Some(Language::Ruby),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "ruby-helper");
    }

    #[test]
    fn test_resolve_ruby_namespaced_module_from_required_file() {
        let symbols = vec![
            make_symbol(
                "ruby-run",
                "lib/user_service.rb::UserService::greet",
                "lib/user_service.rb",
                "greet",
                SymbolKind::Method,
            ),
            make_symbol(
                "ruby-acme",
                "lib/shared/formatter.rb::Acme",
                "lib/shared/formatter.rb",
                "Acme",
                SymbolKind::Module,
            ),
            make_symbol(
                "ruby-shared",
                "lib/shared/formatter.rb::Acme::Shared",
                "lib/shared/formatter.rb",
                "Shared",
                SymbolKind::Module,
            ),
            make_symbol(
                "ruby-formatter",
                "lib/shared/formatter.rb::Acme::Shared::Formatter",
                "lib/shared/formatter.rb",
                "Formatter",
                SymbolKind::Module,
            ),
        ];
        let imports = vec![Import::new(
            "./shared/formatter".to_string(),
            "lib/user_service.rb".to_string(),
            1,
            ImportKind::Glob,
        )];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "lib/user_service.rb",
            &imports,
            &[Dependency::new(
                "lib/user_service.rb::UserService::greet".to_string(),
                "Acme::Shared::Formatter".to_string(),
                "lib/user_service.rb".to_string(),
                12,
                DependencyKind::Implement,
            )],
            Some(Language::Ruby),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "ruby-formatter");
    }

    #[test]
    fn test_mark_ruby_builtin_as_builtin_symbol() {
        let symbols = vec![make_symbol(
            "ruby-run",
            "lib/app.rb::App::run",
            "lib/app.rb",
            "run",
            SymbolKind::Method,
        )];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "lib/app.rb",
            &[],
            &[make_raw_dependency(
                "lib/app.rb::App::run",
                "puts",
                "lib/app.rb",
            )],
            Some(Language::Ruby),
        );

        assert!(summary.unresolved.is_empty());
        assert_eq!(summary.non_local_symbols.len(), 1);
        assert_eq!(summary.non_local_symbols[0].source, SymbolSource::Builtin);
        assert_eq!(
            summary.non_local_symbols[0].qualified_name,
            "<builtin>::puts"
        );
    }

    #[test]
    fn test_resolve_swift_type_use_across_files() {
        let symbols = vec![
            make_symbol(
                "swift-user",
                "Sources/App/Models.swift::User",
                "Sources/App/Models.swift",
                "User",
                SymbolKind::Struct,
            ),
            make_symbol(
                "swift-make-user",
                "Sources/App/Service.swift::makeUser",
                "Sources/App/Service.swift",
                "makeUser",
                SymbolKind::Function,
            ),
        ];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "Sources/App/Service.swift",
            &[],
            &[Dependency::new(
                "Sources/App/Service.swift::makeUser".to_string(),
                "User".to_string(),
                "Sources/App/Service.swift".to_string(),
                10,
                DependencyKind::TypeUse,
            )],
            Some(Language::Swift),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "swift-user");
    }

    #[test]
    fn test_resolve_swift_inheritance_across_files() {
        let symbols = vec![
            make_symbol(
                "swift-repo-protocol",
                "Sources/App/Contracts.swift::UserRepository",
                "Sources/App/Contracts.swift",
                "UserRepository",
                SymbolKind::Interface,
            ),
            make_symbol(
                "swift-in-memory-repo",
                "Sources/App/Repo.swift::InMemoryRepo",
                "Sources/App/Repo.swift",
                "InMemoryRepo",
                SymbolKind::Class,
            ),
        ];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "Sources/App/Repo.swift",
            &[],
            &[Dependency::new(
                "Sources/App/Repo.swift::InMemoryRepo".to_string(),
                "UserRepository".to_string(),
                "Sources/App/Repo.swift".to_string(),
                6,
                DependencyKind::Inherit,
            )],
            Some(Language::Swift),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "swift-repo-protocol");
    }

    #[test]
    fn test_mark_swift_imported_module_symbol_as_external() {
        let symbols = vec![make_symbol(
            "swift-run",
            "Sources/App/main.swift::run",
            "Sources/App/main.swift",
            "run",
            SymbolKind::Function,
        )];
        let imports = vec![Import::new(
            "Foundation".to_string(),
            "Sources/App/main.swift".to_string(),
            1,
            ImportKind::Glob,
        )];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "Sources/App/main.swift",
            &imports,
            &[Dependency::new(
                "Sources/App/main.swift::run".to_string(),
                "Date".to_string(),
                "Sources/App/main.swift".to_string(),
                4,
                DependencyKind::TypeUse,
            )],
            Some(Language::Swift),
        );

        assert!(summary.unresolved.is_empty());
        assert_eq!(summary.non_local_symbols.len(), 1);
        assert_eq!(summary.non_local_symbols[0].source, SymbolSource::External);
        assert_eq!(
            summary.non_local_symbols[0].qualified_name,
            "<external>::Foundation::Date"
        );
    }

    #[test]
    fn test_mark_explicit_swift_module_type_as_external_even_with_local_name_collision() {
        let symbols = vec![
            make_symbol(
                "swift-local-date",
                "Sources/App/main.swift::Date",
                "Sources/App/main.swift",
                "Date",
                SymbolKind::Struct,
            ),
            make_symbol(
                "swift-run",
                "Sources/App/main.swift::run",
                "Sources/App/main.swift",
                "run",
                SymbolKind::Function,
            ),
        ];
        let imports = vec![
            Import::new(
                "Foundation".to_string(),
                "Sources/App/main.swift".to_string(),
                1,
                ImportKind::Glob,
            ),
            Import::new(
                "Dispatch".to_string(),
                "Sources/App/main.swift".to_string(),
                2,
                ImportKind::Glob,
            ),
        ];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "Sources/App/main.swift",
            &imports,
            &[Dependency::new(
                "Sources/App/main.swift::run".to_string(),
                "Foundation.Date".to_string(),
                "Sources/App/main.swift".to_string(),
                4,
                DependencyKind::TypeUse,
            )],
            Some(Language::Swift),
        );

        assert!(summary.unresolved.is_empty());
        assert_eq!(summary.resolved.len(), 1);
        assert_eq!(summary.non_local_symbols.len(), 1);
        assert_eq!(summary.non_local_symbols[0].source, SymbolSource::External);
        assert_eq!(
            summary.non_local_symbols[0].qualified_name,
            "<external>::Foundation::Date"
        );
        assert_eq!(
            summary.resolved[0].to_symbol,
            summary.non_local_symbols[0].id
        );
    }

    #[test]
    fn test_do_not_mark_lowercase_swift_member_call_as_external() {
        let symbols = vec![make_symbol(
            "swift-run",
            "Sources/App/main.swift::run",
            "Sources/App/main.swift",
            "run",
            SymbolKind::Function,
        )];
        let imports = vec![Import::new(
            "Foundation".to_string(),
            "Sources/App/main.swift".to_string(),
            1,
            ImportKind::Glob,
        )];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "Sources/App/main.swift",
            &imports,
            &[Dependency::new(
                "Sources/App/main.swift::run".to_string(),
                "save".to_string(),
                "Sources/App/main.swift".to_string(),
                4,
                DependencyKind::Call,
            )],
            Some(Language::Swift),
        );

        assert_eq!(summary.resolved.len(), 0);
        assert_eq!(summary.non_local_symbols.len(), 0);
        assert_eq!(summary.unresolved.len(), 1);
        assert_eq!(summary.unresolved[0].target, "save");
    }

    #[test]
    fn test_resolve_objc_self_message_prefers_enclosing_class_method() {
        let symbols = vec![
            make_symbol(
                "objc-add",
                "src/sample.m::Calculator::add",
                "src/sample.m",
                "add",
                SymbolKind::Method,
            ),
            make_symbol(
                "objc-class-log",
                "src/sample.m::Calculator::logResult",
                "src/sample.m",
                "logResult",
                SymbolKind::Method,
            ),
            make_symbol(
                "objc-protocol-log",
                "src/sample.m::ResultLogging::logResult",
                "src/sample.m",
                "logResult",
                SymbolKind::Method,
            ),
        ];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/sample.m",
            &[],
            &[Dependency::new(
                "src/sample.m::Calculator::add".to_string(),
                "logResult".to_string(),
                "src/sample.m".to_string(),
                16,
                DependencyKind::Call,
            )],
            Some(Language::Objc),
        );

        assert!(summary.unresolved.is_empty());
        assert_eq!(summary.resolved[0].to_symbol, "objc-class-log");
    }

    #[test]
    fn test_resolve_objc_include_to_companion_implementation() {
        let symbols = vec![
            make_symbol(
                "objc-main",
                "src/main.m::run",
                "src/main.m",
                "run",
                SymbolKind::Function,
            ),
            make_symbol(
                "objc-log-message",
                "src/Logger.m::Logger::logMessage",
                "src/Logger.m",
                "logMessage",
                SymbolKind::Method,
            ),
        ];
        let imports = vec![Import::new(
            "Logger.h".to_string(),
            "src/main.m".to_string(),
            1,
            ImportKind::Named,
        )];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/main.m",
            &imports,
            &[Dependency::new(
                "src/main.m::run".to_string(),
                "logMessage".to_string(),
                "src/main.m".to_string(),
                4,
                DependencyKind::Call,
            )],
            Some(Language::Objc),
        );

        assert!(summary.unresolved.is_empty());
        assert_eq!(summary.resolved[0].to_symbol, "objc-log-message");
    }

    #[test]
    fn test_mark_objc_foundation_base_class_as_external() {
        let symbols = vec![make_symbol(
            "objc-calculator",
            "src/sample.m::Calculator",
            "src/sample.m",
            "Calculator",
            SymbolKind::Class,
        )];
        let imports = vec![Import::new(
            "Foundation/Foundation.h".to_string(),
            "src/sample.m".to_string(),
            1,
            ImportKind::Named,
        )];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/sample.m",
            &imports,
            &[Dependency::new(
                "src/sample.m::Calculator".to_string(),
                "NSObject".to_string(),
                "src/sample.m".to_string(),
                4,
                DependencyKind::Inherit,
            )],
            Some(Language::Objc),
        );

        assert!(summary.unresolved.is_empty());
        assert_eq!(summary.non_local_symbols.len(), 1);
        assert_eq!(summary.non_local_symbols[0].source, SymbolSource::External);
        assert_eq!(
            summary.non_local_symbols[0].qualified_name,
            "<external>::Foundation::NSObject"
        );
    }

    #[test]
    fn test_mark_objc_nslog_as_builtin_symbol() {
        let symbols = vec![make_symbol(
            "objc-main",
            "src/sample.m::main",
            "src/sample.m",
            "main",
            SymbolKind::Function,
        )];
        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/sample.m",
            &[],
            &[Dependency::new(
                "src/sample.m::main".to_string(),
                "NSLog".to_string(),
                "src/sample.m".to_string(),
                22,
                DependencyKind::Call,
            )],
            Some(Language::Objc),
        );

        assert!(summary.unresolved.is_empty());
        assert_eq!(summary.non_local_symbols.len(), 1);
        assert_eq!(summary.non_local_symbols[0].source, SymbolSource::Builtin);
        assert_eq!(
            summary.non_local_symbols[0].qualified_name,
            "<builtin>::NSLog"
        );
    }

    #[test]
    fn test_resolve_python_from_import_alias() {
        let symbols = vec![
            make_symbol(
                "py-run",
                "src/main.py::run",
                "src/main.py",
                "run",
                SymbolKind::Function,
            ),
            make_symbol(
                "py-format",
                "src/helpers.py::format_name",
                "src/helpers.py",
                "format_name",
                SymbolKind::Function,
            ),
        ];
        let imports = vec![Import::new(
            "helpers::format_name".to_string(),
            "src/main.py".to_string(),
            1,
            ImportKind::Alias,
        )
        .with_alias("helper".to_string())];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/main.py",
            &imports,
            &[make_raw_dependency(
                "src/main.py::run",
                "helper",
                "src/main.py",
            )],
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "py-format");
    }

    #[test]
    fn test_resolve_python_relative_import() {
        let symbols = vec![
            make_symbol(
                "py-run",
                "src/pkg/main.py::run",
                "src/pkg/main.py",
                "run",
                SymbolKind::Function,
            ),
            make_symbol(
                "py-format",
                "src/pkg/helpers.py::format_name",
                "src/pkg/helpers.py",
                "format_name",
                SymbolKind::Function,
            ),
        ];
        let imports = vec![Import::new(
            "::helpers::format_name".to_string(),
            "src/pkg/main.py".to_string(),
            1,
            ImportKind::Named,
        )];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies(
            "src/pkg/main.py",
            &imports,
            &[make_raw_dependency(
                "src/pkg/main.py::run",
                "format_name",
                "src/pkg/main.py",
            )],
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "py-format");
    }

    #[test]
    fn test_resolve_c_include_to_same_stem_source() {
        let symbols = vec![
            make_symbol(
                "c-main",
                "src/main.c::run",
                "src/main.c",
                "run",
                SymbolKind::Function,
            ),
            make_symbol(
                "c-helper",
                "src/shared.c::helper",
                "src/shared.c",
                "helper",
                SymbolKind::Function,
            ),
        ];
        let imports = vec![Import::new(
            "shared.h".to_string(),
            "src/main.c".to_string(),
            1,
            ImportKind::Named,
        )];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/main.c",
            &imports,
            &[make_raw_dependency(
                "src/main.c::run",
                "helper",
                "src/main.c",
            )],
            Some(Language::C),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "c-helper");
    }

    #[test]
    fn test_resolve_cpp_include_to_companion_translation_unit() {
        let symbols = vec![
            make_symbol(
                "cpp-main",
                "src/main.cpp::run",
                "src/main.cpp",
                "run",
                SymbolKind::Function,
            ),
            make_symbol(
                "cpp-build",
                "src/user_service.cpp::build",
                "src/user_service.cpp",
                "build",
                SymbolKind::Method,
            ),
        ];
        let imports = vec![Import::new(
            "user_service.hpp".to_string(),
            "src/main.cpp".to_string(),
            1,
            ImportKind::Named,
        )];

        let resolver = Resolver::new(&symbols);
        let summary = resolver.resolve_dependencies_with_language(
            "src/main.cpp",
            &imports,
            &[make_raw_dependency(
                "src/main.cpp::run",
                "build",
                "src/main.cpp",
            )],
            Some(Language::Cpp),
        );

        assert_eq!(summary.unresolved.len(), 0);
        assert_eq!(summary.resolved[0].to_symbol, "cpp-build");
    }
}
