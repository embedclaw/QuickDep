//! Project scanning pipeline that parses source files and persists graph data.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use thiserror::Error;
use tracing::debug;
use walkdir::WalkDir;

use crate::parser::{
    detect_language_with_map, CParser, CSharpParser, CppParser, GoParser, JavaParser, KotlinParser,
    Language, ObjcParser, ParseResult, Parser, PhpParser, PythonParser, RubyParser, RustParser,
    SwiftParser, TypeScriptParser,
};
use crate::project::Project;
use crate::resolver::{
    symbol::{BUILTIN_SYMBOL_FILE_PATH, EXTERNAL_SYMBOL_FILE_PATH},
    Import as ResolverImport, ImportKind as ResolverImportKind, ResolutionSummary, Resolver,
};
use crate::storage::{
    FileState, FileStatus, Import as StorageImport, ImportKind as StorageImportKind, Storage,
    StorageError,
};
use crate::watcher::{build_update_plan, IncrementalUpdatePlan, UpdateKind};

/// Scanner errors raised while building a project's local graph.
#[derive(Debug, Error)]
pub enum ScanError {
    /// File-system read/write failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Storage failure while persisting scan results.
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    /// Failure while planning an incremental update.
    #[error("Watcher error: {0}")]
    Watcher(#[from] crate::watcher::WatcherError),

    /// A scanned path was not inside the project root.
    #[error("Path is outside project root: {0}")]
    OutsideProject(PathBuf),

    /// The scan was cancelled.
    #[error("Operation cancelled")]
    Cancelled,
}

/// Summary returned after a full project scan completes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanSummary {
    /// Number of source files processed.
    pub file_count: usize,
    /// Number of symbols persisted.
    pub symbol_count: usize,
    /// Number of resolved dependencies persisted.
    pub dependency_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedFile {
    pub(crate) file_path: String,
    pub(crate) language: Option<Language>,
    pub(crate) symbols: Vec<crate::core::Symbol>,
    pub(crate) raw_dependencies: Vec<crate::core::Dependency>,
    pub(crate) imports: Vec<ResolverImport>,
    pub(crate) file_state: FileState,
}

/// Synchronous full-project scanner used by the project manager background task.
#[derive(Default)]
pub struct ProjectScanner {
    rust_parser: RustParser,
    typescript_parser: TypeScriptParser,
    java_parser: JavaParser,
    python_parser: PythonParser,
    go_parser: GoParser,
    c_parser: CParser,
    cpp_parser: CppParser,
    csharp_parser: CSharpParser,
    kotlin_parser: KotlinParser,
    php_parser: PhpParser,
    ruby_parser: RubyParser,
    swift_parser: SwiftParser,
    objc_parser: ObjcParser,
}

impl ProjectScanner {
    /// Create a scanner instance.
    pub fn new() -> Self {
        Self {
            rust_parser: RustParser::new(),
            typescript_parser: TypeScriptParser::new(),
            java_parser: JavaParser::new(),
            python_parser: PythonParser::new(),
            go_parser: GoParser::new(),
            c_parser: CParser::new(),
            cpp_parser: CppParser::new(),
            csharp_parser: CSharpParser::new(),
            kotlin_parser: KotlinParser::new(),
            php_parser: PhpParser::new(),
            ruby_parser: RubyParser::new(),
            swift_parser: SwiftParser::new(),
            objc_parser: ObjcParser::new(),
        }
    }

    /// Discover source files that should be scanned for a project.
    pub fn discover_files(&self, project: &Project) -> Result<Vec<PathBuf>, ScanError> {
        let mut files = Vec::new();

        for entry in WalkDir::new(&project.path)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let relative = normalize_relative_path(&project.path, entry.path())?;
            if !self.should_scan_path(project, &relative, entry.path()) {
                continue;
            }

            files.push(entry.into_path());
        }

        files.sort();
        Ok(files)
    }

    /// Parse a discovered source file into symbols, raw dependencies, imports, and file state.
    pub(crate) fn parse_file(
        &mut self,
        project: &Project,
        absolute_path: &Path,
    ) -> Result<ParsedFile, ScanError> {
        let file_path = normalize_relative_path(&project.path, absolute_path)?;
        let last_modified = file_last_modified(absolute_path);

        match fs::read(absolute_path) {
            Ok(content) => {
                let hash = blake3::hash(&content).to_hex().to_string();
                let language = self.detect_language(project, absolute_path);
                let result = self.parse_content(language, absolute_path, &content, &file_path);

                let file_state = if result.error_count == 0 {
                    FileState::new(file_path.clone(), hash, last_modified)
                } else {
                    FileState::new(file_path.clone(), hash, last_modified)
                        .with_status(FileStatus::Failed)
                        .with_error(format!(
                            "encountered {} parse error nodes",
                            result.error_count
                        ))
                };

                Ok(ParsedFile {
                    file_path,
                    language,
                    symbols: result.symbols,
                    raw_dependencies: result.dependencies,
                    imports: result.imports,
                    file_state,
                })
            }
            Err(error) => Ok(ParsedFile {
                file_path: file_path.clone(),
                language: self.detect_language(project, absolute_path),
                symbols: Vec::new(),
                raw_dependencies: Vec::new(),
                imports: Vec::new(),
                file_state: FileState::new(file_path, String::new(), last_modified)
                    .with_status(FileStatus::Failed)
                    .with_error(error.to_string()),
            }),
        }
    }

    /// Resolve parsed data and persist it into the project's SQLite database.
    pub(crate) fn persist_scan(
        &self,
        project: &Project,
        rebuild: bool,
        update_plan: Option<IncrementalUpdatePlan>,
        parsed_files: Vec<ParsedFile>,
    ) -> Result<ScanSummary, ScanError> {
        if project.is_cancelled() {
            return Err(ScanError::Cancelled);
        }

        let mut storage = Storage::new(&project.database_path())?;
        if rebuild {
            storage.clear_all()?;
        }

        let Some(update_plan) = update_plan else {
            let all_symbols = parsed_files
                .iter()
                .flat_map(|file| file.symbols.iter().cloned())
                .collect::<Vec<_>>();
            let all_imports = parsed_files
                .iter()
                .flat_map(|file| file.imports.iter().cloned())
                .collect::<Vec<_>>();
            let resolver = Resolver::new_with_imports(&all_symbols, &all_imports);

            for file in parsed_files {
                if project.is_cancelled() {
                    return Err(ScanError::Cancelled);
                }
                self.persist_parsed_file(&mut storage, &resolver, file)?;
            }

            return Ok(ScanSummary {
                file_count: storage.count_file_states()?,
                symbol_count: storage.count_symbols()?,
                dependency_count: storage.count_dependencies()?,
            });
        };

        if update_plan.updates.is_empty() {
            return Ok(ScanSummary {
                file_count: storage.count_file_states()?,
                symbol_count: storage.count_symbols()?,
                dependency_count: storage.count_dependencies()?,
            });
        }

        let updated_paths = update_plan
            .updates
            .iter()
            .map(|update| update.relative_path.clone())
            .collect::<HashSet<_>>();
        let mut all_symbols = self.load_symbols_excluding(&storage, &updated_paths)?;
        let mut all_imports = self.load_imports_excluding(&storage, &updated_paths)?;
        all_symbols.extend(
            parsed_files
                .iter()
                .flat_map(|file| file.symbols.iter().cloned()),
        );
        all_imports.extend(
            parsed_files
                .iter()
                .flat_map(|file| file.imports.iter().cloned()),
        );
        let resolver = Resolver::new_with_imports(&all_symbols, &all_imports);
        let mut parsed_by_path = parsed_files
            .into_iter()
            .map(|file| (file.file_path.clone(), file))
            .collect::<HashMap<_, _>>();

        for update in update_plan.updates {
            if project.is_cancelled() {
                return Err(ScanError::Cancelled);
            }

            match update.kind {
                UpdateKind::Deleted => {
                    storage.delete_file_data(&update.relative_path)?;
                    cleanup_orphan_non_local_symbols(&storage)?;
                }
                UpdateKind::Added | UpdateKind::Modified => {
                    let file = parsed_by_path
                        .remove(&update.relative_path)
                        .ok_or_else(|| {
                            ScanError::Io(std::io::Error::other(format!(
                                "missing parsed data for {}",
                                update.relative_path
                            )))
                        })?;
                    self.persist_parsed_file(&mut storage, &resolver, file)?;
                }
            }
        }

        Ok(ScanSummary {
            file_count: storage.count_file_states()?,
            symbol_count: storage.count_symbols()?,
            dependency_count: storage.count_dependencies()?,
        })
    }

    /// Compare the current workspace state with stored file hashes.
    pub(crate) fn plan_updates(
        &self,
        project: &Project,
        discovered_files: &[PathBuf],
    ) -> Result<IncrementalUpdatePlan, ScanError> {
        let storage = Storage::new(&project.database_path())?;
        let previous_states = storage
            .get_all_file_states()?
            .into_iter()
            .map(|state| (state.path.clone(), state))
            .collect::<HashMap<_, _>>();
        let discovered_relative_paths = discovered_files
            .iter()
            .map(|path| normalize_relative_path(&project.path, path))
            .collect::<Result<HashSet<_>, _>>()?;
        let mut update_plan = build_update_plan(&project.path, discovered_files, &previous_states)?;

        let mut removed_paths = previous_states
            .keys()
            .filter(|relative_path| !discovered_relative_paths.contains(*relative_path))
            .map(|relative_path| crate::watcher::IncrementalFileUpdate {
                absolute_path: project.path.join(relative_path),
                relative_path: relative_path.clone(),
                kind: UpdateKind::Deleted,
                hash: None,
                last_modified: None,
            })
            .collect::<Vec<_>>();
        removed_paths.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        update_plan.updates.extend(removed_paths);

        Ok(update_plan)
    }

    fn should_scan_path(
        &self,
        project: &Project,
        relative_path: &str,
        absolute_path: &Path,
    ) -> bool {
        if !matches_patterns(relative_path, &project.config.include) {
            return false;
        }

        if matches_patterns(relative_path, &project.config.exclude) {
            return false;
        }

        if !project.config.include_tests && is_test_path(relative_path) {
            return false;
        }

        let Some(language) = self.detect_language(project, absolute_path) else {
            return false;
        };

        if !project
            .config
            .languages
            .iter()
            .any(|configured| configured.eq_ignore_ascii_case(language.name()))
        {
            return false;
        }

        matches!(
            language,
            Language::Rust
                | Language::TypeScript
                | Language::JavaScript
                | Language::Java
                | Language::Python
                | Language::Go
                | Language::C
                | Language::Cpp
                | Language::CSharp
                | Language::Kotlin
                | Language::Php
                | Language::Ruby
                | Language::Swift
                | Language::Objc
        )
    }

    fn detect_language(&self, project: &Project, path: &Path) -> Option<Language> {
        detect_language_with_map(path, &project.config.parser_map)
    }

    fn parse_content(
        &mut self,
        language: Option<Language>,
        absolute_path: &Path,
        content: &[u8],
        file_path: &str,
    ) -> ParseResult {
        match language {
            Some(Language::Rust) => self
                .rust_parser
                .parse_file(absolute_path, content, file_path),
            Some(Language::TypeScript) => {
                self.typescript_parser
                    .parse_file(absolute_path, content, file_path)
            }
            Some(Language::JavaScript) => {
                self.typescript_parser
                    .parse_file(absolute_path, content, file_path)
            }
            Some(Language::Java) => self
                .java_parser
                .parse_file(absolute_path, content, file_path),
            Some(Language::Python) => {
                self.python_parser
                    .parse_file(absolute_path, content, file_path)
            }
            Some(Language::Go) => self.go_parser.parse_file(absolute_path, content, file_path),
            Some(Language::C) => self.c_parser.parse_file(absolute_path, content, file_path),
            Some(Language::Cpp) => self
                .cpp_parser
                .parse_file(absolute_path, content, file_path),
            Some(Language::CSharp) => {
                self.csharp_parser
                    .parse_file(absolute_path, content, file_path)
            }
            Some(Language::Kotlin) => {
                self.kotlin_parser
                    .parse_file(absolute_path, content, file_path)
            }
            Some(Language::Php) => self
                .php_parser
                .parse_file(absolute_path, content, file_path),
            Some(Language::Ruby) => self
                .ruby_parser
                .parse_file(absolute_path, content, file_path),
            Some(Language::Swift) => {
                self.swift_parser
                    .parse_file(absolute_path, content, file_path)
            }
            Some(Language::Objc) => self
                .objc_parser
                .parse_file(absolute_path, content, file_path),
            _ => ParseResult::default(),
        }
    }

    fn load_symbols_excluding(
        &self,
        storage: &Storage,
        excluded_paths: &HashSet<String>,
    ) -> Result<Vec<crate::core::Symbol>, ScanError> {
        let mut symbols = Vec::new();
        for file_state in storage.get_all_file_states()? {
            if excluded_paths.contains(&file_state.path) {
                continue;
            }
            symbols.extend(storage.get_symbols_by_file(&file_state.path)?);
        }
        Ok(symbols)
    }

    fn load_imports_excluding(
        &self,
        storage: &Storage,
        excluded_paths: &HashSet<String>,
    ) -> Result<Vec<ResolverImport>, ScanError> {
        Ok(storage
            .get_all_imports()?
            .into_iter()
            .filter(|import| !excluded_paths.contains(&import.file_path))
            .map(resolver_import_from_storage)
            .collect())
    }

    fn persist_parsed_file(
        &self,
        storage: &mut Storage,
        resolver: &Resolver,
        file: ParsedFile,
    ) -> Result<(), ScanError> {
        let ResolutionSummary {
            resolved,
            non_local_symbols,
            unresolved,
        } = resolver.resolve_dependencies_with_language(
            &file.file_path,
            &file.imports,
            &file.raw_dependencies,
            file.language,
        );

        if !unresolved.is_empty() {
            debug!(
                file_path = %file.file_path,
                unresolved = unresolved.len(),
                "leaving dependencies unresolved after local resolution"
            );
        }

        if !non_local_symbols.is_empty() {
            debug!(
                file_path = %file.file_path,
                non_local_symbols = non_local_symbols.len(),
                "materialized builtin/external symbols during resolution"
            );
        }

        let imports = file
            .imports
            .iter()
            .map(storage_import_from_resolver)
            .collect::<Vec<_>>();

        let existing_file = storage.get_file_state(&file.file_path)?.is_some();
        if existing_file {
            let diff = persist_incremental_file_update(
                storage,
                &file.file_path,
                &file.symbols,
                &resolved,
                &imports,
                &file.file_state,
            )?;
            debug!(
                file_path = %file.file_path,
                added = diff.added.len(),
                removed = diff.removed.len(),
                modified = diff.modified.len(),
                "applied symbol-level incremental update"
            );
        } else {
            storage.replace_file_data(
                &file.file_path,
                &file.symbols,
                &resolved,
                &imports,
                &file.file_state,
            )?;
        }
        persist_non_local_symbols(storage, &non_local_symbols)?;
        cleanup_orphan_non_local_symbols(storage)?;
        Ok(())
    }
}

fn persist_incremental_file_update(
    storage: &mut Storage,
    file_path: &str,
    symbols: &[crate::core::Symbol],
    deps: &[crate::core::Dependency],
    imports: &[StorageImport],
    file_state: &FileState,
) -> Result<crate::core::SymbolDiff, ScanError> {
    let existing_symbols = storage.get_symbols_by_file(file_path)?;
    let existing_dependencies = storage.get_dependencies_by_file(file_path)?;
    let diff = compute_symbol_diff(&existing_symbols, symbols, &existing_dependencies, deps);

    for symbol_id in &diff.modified {
        clear_outgoing_dependencies(storage, symbol_id)?;
    }

    for symbol_id in &diff.removed {
        storage.delete_symbol(symbol_id)?;
    }

    let added_ids = diff.added.iter().cloned().collect::<HashSet<_>>();
    let modified_ids = diff.modified.iter().cloned().collect::<HashSet<_>>();
    for symbol in symbols {
        if added_ids.contains(&symbol.id) {
            storage.insert_symbol(symbol)?;
        } else if modified_ids.contains(&symbol.id) {
            storage.update_symbol(symbol)?;
        }
    }

    let changed_ids = added_ids
        .union(&modified_ids)
        .cloned()
        .collect::<HashSet<_>>();
    for dep in deps {
        if changed_ids.contains(&dep.from_symbol) {
            storage.insert_dependency(dep)?;
        }
    }

    storage.delete_imports_by_file(file_path)?;
    for import in imports {
        storage.insert_import(import)?;
    }
    storage.upsert_file_state(file_state)?;

    Ok(diff)
}

fn compute_symbol_diff(
    existing_symbols: &[crate::core::Symbol],
    new_symbols: &[crate::core::Symbol],
    existing_dependencies: &[crate::core::Dependency],
    new_dependencies: &[crate::core::Dependency],
) -> crate::core::SymbolDiff {
    let existing_by_id = existing_symbols
        .iter()
        .map(|symbol| (symbol.id.clone(), symbol))
        .collect::<HashMap<_, _>>();
    let new_by_id = new_symbols
        .iter()
        .map(|symbol| (symbol.id.clone(), symbol))
        .collect::<HashMap<_, _>>();
    let existing_dep_signatures = dependency_signatures_by_symbol(existing_dependencies);
    let new_dep_signatures = dependency_signatures_by_symbol(new_dependencies);

    let mut diff = crate::core::SymbolDiff::default();

    for symbol_id in new_by_id.keys() {
        if !existing_by_id.contains_key(symbol_id) {
            diff.added.push(symbol_id.clone());
            continue;
        }

        let existing = existing_by_id
            .get(symbol_id)
            .expect("existing symbol should be present");
        let new = new_by_id
            .get(symbol_id)
            .expect("new symbol should be present");
        let deps_changed =
            existing_dep_signatures.get(symbol_id) != new_dep_signatures.get(symbol_id);

        if !symbols_match(existing, new) || deps_changed {
            diff.modified.push(symbol_id.clone());
        }
    }

    for symbol_id in existing_by_id.keys() {
        if !new_by_id.contains_key(symbol_id) {
            diff.removed.push(symbol_id.clone());
        }
    }

    diff.added.sort();
    diff.removed.sort();
    diff.modified.sort();
    diff
}

fn symbols_match(left: &crate::core::Symbol, right: &crate::core::Symbol) -> bool {
    left.name == right.name
        && left.qualified_name == right.qualified_name
        && left.kind.as_str() == right.kind.as_str()
        && left.file_path == right.file_path
        && left.line == right.line
        && left.column == right.column
        && left.visibility.as_str() == right.visibility.as_str()
        && left.signature == right.signature
        && left.source.as_str() == right.source.as_str()
}

fn dependency_signatures_by_symbol(
    dependencies: &[crate::core::Dependency],
) -> HashMap<String, Vec<(String, u32, String)>> {
    let mut grouped = HashMap::new();

    for dependency in dependencies {
        grouped
            .entry(dependency.from_symbol.clone())
            .or_insert_with(Vec::new)
            .push((
                dependency.to_symbol.clone(),
                dependency.from_line,
                dependency.kind.as_str().to_string(),
            ));
    }

    for signatures in grouped.values_mut() {
        signatures.sort();
    }

    grouped
}

fn clear_outgoing_dependencies(storage: &Storage, symbol_id: &str) -> Result<(), ScanError> {
    for dependency in storage.get_dependencies_from(symbol_id)? {
        storage.delete_dependency(&dependency.id)?;
    }

    Ok(())
}

fn persist_non_local_symbols(
    storage: &Storage,
    symbols: &[crate::core::Symbol],
) -> Result<(), ScanError> {
    for symbol in symbols {
        if storage.get_symbol(&symbol.id)?.is_none() {
            storage.insert_symbol(symbol)?;
        }
    }
    Ok(())
}

fn cleanup_orphan_non_local_symbols(storage: &Storage) -> Result<(), ScanError> {
    for file_path in [BUILTIN_SYMBOL_FILE_PATH, EXTERNAL_SYMBOL_FILE_PATH] {
        for symbol in storage.get_symbols_by_file(file_path)? {
            if storage.get_dependencies_from(&symbol.id)?.is_empty()
                && storage.get_dependencies_to(&symbol.id)?.is_empty()
            {
                storage.delete_symbol(&symbol.id)?;
            }
        }
    }

    Ok(())
}

fn normalize_relative_path(project_root: &Path, path: &Path) -> Result<String, ScanError> {
    let relative = path
        .strip_prefix(project_root)
        .map_err(|_| ScanError::OutsideProject(path.to_path_buf()))?;

    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn matches_patterns(path: &str, patterns: &[String]) -> bool {
    patterns
        .iter()
        .any(|pattern| glob_match::glob_match(pattern, path))
}

fn is_test_path(path: &str) -> bool {
    let file_name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    path.starts_with("tests/")
        || path.starts_with("spec/")
        || path.starts_with("Tests/")
        || path.contains("/tests/")
        || path.contains("/spec/")
        || path.contains("/Tests/")
        || path.starts_with("src/test/")
        || path.contains("/src/test/")
        || file_name.ends_with("_test.rs")
        || file_name.ends_with(".test.rs")
        || file_name.ends_with("_test.go")
        || file_name.ends_with("_tests.go")
        || file_name.ends_with(".test.ts")
        || file_name.ends_with(".spec.ts")
        || file_name.ends_with(".test.tsx")
        || file_name.ends_with(".spec.tsx")
        || file_name.ends_with(".test.js")
        || file_name.ends_with(".spec.js")
        || file_name.ends_with(".test.jsx")
        || file_name.ends_with(".spec.jsx")
        || file_name.ends_with("Test.java")
        || file_name.ends_with("Tests.java")
        || file_name.ends_with("Test.cs")
        || file_name.ends_with("Tests.cs")
        || file_name.ends_with("Test.kt")
        || file_name.ends_with("Tests.kt")
        || file_name.ends_with("Test.php")
        || file_name.ends_with("Tests.php")
        || file_name.ends_with("_test.rb")
        || file_name.ends_with("_spec.rb")
        || file_name.ends_with("Tests.swift")
        || file_name.ends_with("Tests.m")
        || file_name.ends_with("_test.py")
        || file_name.starts_with("test_")
}

fn file_last_modified(path: &Path) -> u64 {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn storage_import_from_resolver(import: &ResolverImport) -> StorageImport {
    let kind = match import.kind {
        ResolverImportKind::Named => StorageImportKind::Named,
        ResolverImportKind::Glob => StorageImportKind::Glob,
        ResolverImportKind::SelfImport => StorageImportKind::SelfImport,
        ResolverImportKind::Alias => StorageImportKind::Alias,
        ResolverImportKind::ReExportNamed => StorageImportKind::ReExportNamed,
        ResolverImportKind::ReExportGlob => StorageImportKind::ReExportGlob,
        ResolverImportKind::ReExportAlias => StorageImportKind::ReExportAlias,
    };

    let mut stored = StorageImport::new(
        import.source.clone(),
        import.file_path.clone(),
        import.line,
        kind,
    );

    if let Some(alias) = &import.alias {
        stored = stored.with_alias(alias.clone());
    }

    stored
}

fn resolver_import_from_storage(import: StorageImport) -> ResolverImport {
    let kind = match import.kind {
        StorageImportKind::Named => ResolverImportKind::Named,
        StorageImportKind::Glob => ResolverImportKind::Glob,
        StorageImportKind::SelfImport => ResolverImportKind::SelfImport,
        StorageImportKind::Alias => ResolverImportKind::Alias,
        StorageImportKind::ReExportNamed => ResolverImportKind::ReExportNamed,
        StorageImportKind::ReExportGlob => ResolverImportKind::ReExportGlob,
        StorageImportKind::ReExportAlias => ResolverImportKind::ReExportAlias,
    };

    let mut resolved = ResolverImport::new(import.source, import.file_path, import.line, kind);
    if let Some(alias) = import.alias {
        resolved = resolved.with_alias(alias);
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_is_test_path_covers_supported_language_patterns() {
        assert!(is_test_path("tests/service.ts"));
        assert!(is_test_path("spec/services/user_service_spec.rb"));
        assert!(is_test_path("Tests/UserRepositoryTests.swift"));
        assert!(is_test_path("Tests/CalculatorTests.m"));
        assert!(is_test_path("src/test/java/AppTest.java"));
        assert!(is_test_path("tests/AppTests.cs"));
        assert!(is_test_path("src/AppTest.kt"));
        assert!(is_test_path("tests/AppTest.php"));
        assert!(is_test_path("src/component.test.tsx"));
        assert!(is_test_path("src/component.spec.ts"));
        assert!(is_test_path("src/component.test.js"));
        assert!(is_test_path("src/component.spec.jsx"));
        assert!(is_test_path("pkg/service_test.go"));
        assert!(is_test_path("src/helper_test.py"));
        assert!(is_test_path("src/test_worker.py"));
        assert!(is_test_path("test/models/user_test.rb"));
        assert!(!is_test_path("src/main/java/App.java"));
        assert!(!is_test_path("src/App.cs"));
        assert!(!is_test_path("src/App.kt"));
        assert!(!is_test_path("src/App.php"));
        assert!(!is_test_path("app/models/user.rb"));
        assert!(!is_test_path("Sources/App/main.swift"));
        assert!(!is_test_path("src/main.m"));
        assert!(!is_test_path("src/service.ts"));
        assert!(!is_test_path("src/service.js"));
        assert!(!is_test_path("pkg/service.go"));
        assert!(!is_test_path("src/worker.py"));
    }

    #[test]
    fn test_parse_content_supports_java_javascript_typescript_python_go_c_cpp_csharp_kotlin_php_ruby_swift_and_objc(
    ) {
        let mut scanner = ProjectScanner::new();

        let java = scanner.parse_content(
            Some(Language::Java),
            Path::new("src/App.java"),
            br#"class App { void run() { helper(); } void helper() {} }"#,
            "src/App.java",
        );
        assert!(java
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "helper"));

        let js = scanner.parse_content(
            Some(Language::JavaScript),
            Path::new("src/service.js"),
            b"export function entry() { return helper(); }\nfunction helper() { return 1; }\n",
            "src/service.js",
        );
        assert_eq!(js.symbols.len(), 2);
        assert!(js
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "helper"));

        let ts = scanner.parse_content(
            Some(Language::TypeScript),
            Path::new("src/service.ts"),
            b"export function entry() { return helper(); }\nfunction helper() { return 1; }\n",
            "src/service.ts",
        );
        assert_eq!(ts.symbols.len(), 2);
        assert!(ts
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "helper"));

        let py = scanner.parse_content(
            Some(Language::Python),
            Path::new("src/tasks.py"),
            b"def entry():\n    return helper()\n\ndef helper():\n    return 'ok'\n",
            "src/tasks.py",
        );
        assert_eq!(py.symbols.len(), 2);
        assert!(py
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "helper"));

        let go = scanner.parse_content(
            Some(Language::Go),
            Path::new("src/service.go"),
            b"type service struct{}\nfunc (s *service) run() { helper() }\nfunc helper() {}\n",
            "src/service.go",
        );
        assert_eq!(go.symbols.len(), 3);
        assert!(go
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "helper"));

        let c = scanner.parse_content(
            Some(Language::C),
            Path::new("src/service.c"),
            b"#include \"shared.h\"\nint helper(void) { return 1; }\nint run(void) { return helper(); }\n",
            "src/service.c",
        );
        assert_eq!(c.symbols.len(), 2);
        assert_eq!(c.imports.len(), 1);
        assert!(c
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "helper"));

        let cpp = scanner.parse_content(
            Some(Language::Cpp),
            Path::new("src/service.cpp"),
            b"#include \"shared.hpp\"\nnamespace app { class User { public: int run() { return helper(); } }; }\nint helper() { return 1; }\n",
            "src/service.cpp",
        );
        assert!(cpp.symbols.iter().any(|symbol| symbol.name == "User"));
        assert!(cpp.symbols.iter().any(|symbol| symbol.name == "run"));
        assert_eq!(cpp.imports.len(), 1);
        assert!(cpp
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "helper"));

        let csharp = scanner.parse_content(
            Some(Language::CSharp),
            Path::new("src/App.cs"),
            br#"class App { void Run() { Helper(); } void Helper() {} }"#,
            "src/App.cs",
        );
        assert!(csharp
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "Helper"));

        let kotlin = scanner.parse_content(
            Some(Language::Kotlin),
            Path::new("src/App.kt"),
            b"class App {\n    fun run() { helper() }\n    fun helper() {}\n}\n",
            "src/App.kt",
        );
        assert!(kotlin
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "helper"));

        let php = scanner.parse_content(
            Some(Language::Php),
            Path::new("src/App.php"),
            b"<?php\nclass App {\n    function run() { helper(); }\n}\nfunction helper() {}\n",
            "src/App.php",
        );
        assert!(php
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "helper"));

        let ruby = scanner.parse_content(
            Some(Language::Ruby),
            Path::new("app/services/user_service.rb"),
            br#"require_relative "../shared/helper"

class UserService
  def run
    Helper.new
  end
end
"#,
            "app/services/user_service.rb",
        );
        assert!(ruby
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "Helper"));
        assert_eq!(ruby.imports.len(), 1);

        let swift = scanner.parse_content(
            Some(Language::Swift),
            Path::new("Sources/App/main.swift"),
            br#"import Foundation

struct User {}

func makeUser() -> User {
    User()
}
"#,
            "Sources/App/main.swift",
        );
        assert!(swift
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "User"));
        assert_eq!(swift.imports.len(), 1);

        let objc = scanner.parse_content(
            Some(Language::Objc),
            Path::new("src/main.m"),
            br#"#import <Foundation/Foundation.h>

@interface Calculator : NSObject
+ (Calculator *)sharedCalculator;
@end

int main(void) {
    [Calculator sharedCalculator];
    NSLog(@"hi");
    return 0;
}
"#,
            "src/main.m",
        );
        assert!(objc
            .dependencies
            .iter()
            .any(|dependency| dependency.to_symbol == "sharedCalculator"));
        assert_eq!(objc.imports.len(), 1);
    }

    #[test]
    fn test_discover_files_respects_language_filter_for_java_javascript_typescript_python_go_c_cpp_csharp_kotlin_php_ruby_swift_and_objc(
    ) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        std::fs::create_dir_all(temp_dir.path().join("src")).expect("Failed to create src dir");
        std::fs::create_dir_all(temp_dir.path().join("app")).expect("Failed to create app dir");
        std::fs::create_dir_all(temp_dir.path().join("Sources/App"))
            .expect("Failed to create Swift sources dir");
        std::fs::write(temp_dir.path().join("src/lib.rs"), "pub fn entry() {}\n")
            .expect("Failed to write Rust file");
        std::fs::write(
            temp_dir.path().join("src/service.java"),
            "class Service { void entry() {} }\n",
        )
        .expect("Failed to write Java file");
        std::fs::write(
            temp_dir.path().join("src/service.js"),
            "export function entry() {}\n",
        )
        .expect("Failed to write JavaScript file");
        std::fs::write(
            temp_dir.path().join("src/service.ts"),
            "export function entry() {}\n",
        )
        .expect("Failed to write TypeScript file");
        std::fs::write(
            temp_dir.path().join("src/tasks.py"),
            "def entry():\n    return 1\n",
        )
        .expect("Failed to write Python file");
        std::fs::write(
            temp_dir.path().join("src/service.go"),
            "package sample\n\nfunc Entry() {}\n",
        )
        .expect("Failed to write Go file");
        std::fs::write(
            temp_dir.path().join("src/native.c"),
            "int helper(void) { return 1; }\n",
        )
        .expect("Failed to write C file");
        std::fs::write(
            temp_dir.path().join("src/native.cpp"),
            "int helper() { return 1; }\n",
        )
        .expect("Failed to write C++ file");
        std::fs::write(
            temp_dir.path().join("src/App.cs"),
            "class App { void Entry() {} }\n",
        )
        .expect("Failed to write C# file");
        std::fs::write(
            temp_dir.path().join("src/App.kt"),
            "class App { fun entry() {} }\n",
        )
        .expect("Failed to write Kotlin file");
        std::fs::write(
            temp_dir.path().join("src/App.php"),
            "<?php\nclass App { public function entry(): void {} }\n",
        )
        .expect("Failed to write PHP file");
        std::fs::write(
            temp_dir.path().join("app/service.rb"),
            "class Service\n  def entry; end\nend\n",
        )
        .expect("Failed to write Ruby file");
        std::fs::write(
            temp_dir.path().join("Sources/App/main.swift"),
            "import Foundation\n\nfunc entry() { print(\"ok\") }\n",
        )
        .expect("Failed to write Swift file");
        std::fs::write(
            temp_dir.path().join("src/main.m"),
            "#import <Foundation/Foundation.h>\n\nint main(void) { NSLog(@\"ok\"); return 0; }\n",
        )
        .expect("Failed to write Objective-C file");

        let project = Project::new(
            temp_dir.path(),
            "scanner-project",
            Some(
                crate::project::ProjectConfig::default()
                    .with_include(vec![
                        "src/**".to_string(),
                        "app/**".to_string(),
                        "Sources/**".to_string(),
                    ])
                    .with_languages(vec![
                        "java".to_string(),
                        "javascript".to_string(),
                        "typescript".to_string(),
                        "python".to_string(),
                        "go".to_string(),
                        "c".to_string(),
                        "cpp".to_string(),
                        "csharp".to_string(),
                        "kotlin".to_string(),
                        "php".to_string(),
                        "ruby".to_string(),
                        "swift".to_string(),
                        "objc".to_string(),
                    ]),
            ),
        )
        .expect("Failed to create project");

        let scanner = ProjectScanner::new();
        let files = scanner
            .discover_files(&project)
            .expect("Failed to discover files");
        let relative = files
            .iter()
            .map(|path| normalize_relative_path(&project.path, path).expect("relative path"))
            .collect::<Vec<_>>();

        assert_eq!(
            relative,
            vec![
                "Sources/App/main.swift".to_string(),
                "app/service.rb".to_string(),
                "src/App.cs".to_string(),
                "src/App.kt".to_string(),
                "src/App.php".to_string(),
                "src/main.m".to_string(),
                "src/native.c".to_string(),
                "src/native.cpp".to_string(),
                "src/service.go".to_string(),
                "src/service.java".to_string(),
                "src/service.js".to_string(),
                "src/service.ts".to_string(),
                "src/tasks.py".to_string(),
            ]
        );
    }

    #[test]
    fn test_discover_files_uses_parser_map_override() {
        use std::collections::HashMap;

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        std::fs::create_dir_all(temp_dir.path().join("src")).expect("Failed to create src dir");
        std::fs::write(
            temp_dir.path().join("src/component.vue"),
            "export function render() { return helper(); }\nfunction helper() { return 1; }\n",
        )
        .expect("Failed to write Vue file");

        let project = Project::new(
            temp_dir.path(),
            "parser-map-project",
            Some(
                crate::project::ProjectConfig::default()
                    .with_include(vec!["src/**".to_string()])
                    .with_languages(vec!["typescript".to_string()])
                    .with_parser_map(HashMap::from([(
                        ".vue".to_string(),
                        "typescript".to_string(),
                    )])),
            ),
        )
        .expect("Failed to create project");

        let mut scanner = ProjectScanner::new();
        let files = scanner
            .discover_files(&project)
            .expect("Failed to discover files");
        assert_eq!(files.len(), 1);

        let parsed = scanner
            .parse_file(&project, &files[0])
            .expect("Failed to parse file");
        assert_eq!(parsed.language, Some(Language::TypeScript));
        assert_eq!(parsed.symbols.len(), 2);
    }

    #[test]
    fn test_plan_updates_marks_unchanged_files_as_skipped() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        std::fs::create_dir_all(temp_dir.path().join("src")).expect("Failed to create src dir");
        std::fs::write(
            temp_dir.path().join("src/lib.rs"),
            "pub fn entry() { helper(); }\npub fn helper() {}\n",
        )
        .expect("Failed to write Rust file");

        let project = Project::new(temp_dir.path(), "incremental-project", None)
            .expect("Failed to create project");
        let mut scanner = ProjectScanner::new();
        let files = scanner
            .discover_files(&project)
            .expect("Failed to discover files");
        let parsed = scanner
            .parse_file(&project, &files[0])
            .expect("Failed to parse file");

        scanner
            .persist_scan(
                &project,
                false,
                Some(crate::watcher::IncrementalUpdatePlan {
                    updates: vec![crate::watcher::IncrementalFileUpdate {
                        absolute_path: files[0].clone(),
                        relative_path: "src/lib.rs".to_string(),
                        kind: crate::watcher::UpdateKind::Added,
                        hash: Some(parsed.file_state.hash.clone()),
                        last_modified: Some(parsed.file_state.last_modified),
                    }],
                    skipped: Vec::new(),
                }),
                vec![parsed],
            )
            .expect("Failed to persist initial scan");

        let plan = scanner
            .plan_updates(&project, &files)
            .expect("Failed to plan updates");
        assert!(plan.updates.is_empty());
        assert_eq!(plan.skipped.len(), 1);
    }

    #[test]
    fn test_plan_updates_deletes_files_excluded_by_updated_config() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        std::fs::create_dir_all(temp_dir.path().join("src")).expect("Failed to create src dir");
        std::fs::write(
            temp_dir.path().join("src/helper_test.rs"),
            "pub fn helper() {}\n",
        )
        .expect("Failed to write test file");

        let initial_project = Project::new(
            temp_dir.path(),
            "config-project",
            Some(crate::project::ProjectConfig::default().with_tests(true)),
        )
        .expect("Failed to create project");
        let mut scanner = ProjectScanner::new();
        let files = scanner
            .discover_files(&initial_project)
            .expect("Failed to discover files");
        assert_eq!(files.len(), 1);
        let parsed = scanner
            .parse_file(&initial_project, &files[0])
            .expect("Failed to parse file");

        scanner
            .persist_scan(
                &initial_project,
                false,
                Some(crate::watcher::IncrementalUpdatePlan {
                    updates: vec![crate::watcher::IncrementalFileUpdate {
                        absolute_path: files[0].clone(),
                        relative_path: "src/helper_test.rs".to_string(),
                        kind: crate::watcher::UpdateKind::Added,
                        hash: Some(parsed.file_state.hash.clone()),
                        last_modified: Some(parsed.file_state.last_modified),
                    }],
                    skipped: Vec::new(),
                }),
                vec![parsed],
            )
            .expect("Failed to persist initial scan");

        let updated_project = Project::new(temp_dir.path(), "config-project", None)
            .expect("Failed to create updated project");
        let discovered_after_config_change = scanner
            .discover_files(&updated_project)
            .expect("Failed to discover files after config update");
        assert!(discovered_after_config_change.is_empty());

        let plan = scanner
            .plan_updates(&updated_project, &discovered_after_config_change)
            .expect("Failed to plan updates after config change");
        assert_eq!(plan.updates.len(), 1);
        assert_eq!(plan.updates[0].relative_path, "src/helper_test.rs");
        assert_eq!(plan.updates[0].kind, crate::watcher::UpdateKind::Deleted);

        scanner
            .persist_scan(&updated_project, false, Some(plan), Vec::new())
            .expect("Failed to persist config-driven deletion");

        let storage = Storage::new(&updated_project.database_path()).expect("Open storage");
        assert!(storage
            .get_file_state("src/helper_test.rs")
            .expect("Lookup file state")
            .is_none());
        assert!(storage
            .get_symbols_by_file("src/helper_test.rs")
            .expect("Lookup symbols")
            .is_empty());
    }

    #[test]
    fn test_persist_scan_materializes_and_cleans_non_local_symbols() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        std::fs::create_dir_all(temp_dir.path().join("src")).expect("Failed to create src dir");
        std::fs::write(
            temp_dir.path().join("src/main.rs"),
            "use std::collections::HashMap;\n\
             pub fn run() {\n\
                 println!(\"hi\");\n\
                 let _items: HashMap<String, String> = HashMap::new();\n\
             }\n",
        )
        .expect("Failed to write Rust file");

        let project =
            Project::new(temp_dir.path(), "non-local-symbol-project", None).expect("project");
        let mut scanner = ProjectScanner::new();
        let files = scanner
            .discover_files(&project)
            .expect("Failed to discover files");
        let parsed = scanner
            .parse_file(&project, &files[0])
            .expect("Failed to parse file");

        scanner
            .persist_scan(
                &project,
                false,
                Some(crate::watcher::IncrementalUpdatePlan {
                    updates: vec![crate::watcher::IncrementalFileUpdate {
                        absolute_path: files[0].clone(),
                        relative_path: "src/main.rs".to_string(),
                        kind: crate::watcher::UpdateKind::Added,
                        hash: Some(parsed.file_state.hash.clone()),
                        last_modified: Some(parsed.file_state.last_modified),
                    }],
                    skipped: Vec::new(),
                }),
                vec![parsed],
            )
            .expect("Failed to persist initial scan");

        let storage = Storage::new(&project.database_path()).expect("Open storage");
        let builtin_symbols = storage
            .get_symbols_by_file(BUILTIN_SYMBOL_FILE_PATH)
            .expect("Load builtin symbols");
        let external_symbols = storage
            .get_symbols_by_file(EXTERNAL_SYMBOL_FILE_PATH)
            .expect("Load external symbols");
        assert!(builtin_symbols
            .iter()
            .any(|symbol| symbol.name == "println!"));
        assert!(external_symbols.iter().any(|symbol| symbol.name == "new"));
        drop(storage);

        std::fs::write(
            temp_dir.path().join("src/main.rs"),
            "pub fn run() { helper(); }\nfn helper() {}\n",
        )
        .expect("Failed to rewrite Rust file");
        let updated_files = scanner
            .discover_files(&project)
            .expect("Failed to rediscover files");
        let plan = scanner
            .plan_updates(&project, &updated_files)
            .expect("Failed to plan updates");
        let updated_parsed = scanner
            .parse_file(&project, &updated_files[0])
            .expect("Failed to parse updated file");

        scanner
            .persist_scan(&project, false, Some(plan), vec![updated_parsed])
            .expect("Failed to persist updated scan");

        let storage = Storage::new(&project.database_path()).expect("Reopen storage");
        assert!(storage
            .get_symbols_by_file(BUILTIN_SYMBOL_FILE_PATH)
            .expect("Load builtin symbols after cleanup")
            .is_empty());
        assert!(storage
            .get_symbols_by_file(EXTERNAL_SYMBOL_FILE_PATH)
            .expect("Load external symbols after cleanup")
            .is_empty());
    }

    #[test]
    fn test_incremental_updates_preserve_unchanged_symbol_dependencies() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        std::fs::create_dir_all(temp_dir.path().join("src")).expect("Failed to create src dir");
        std::fs::write(
            temp_dir.path().join("src/lib.rs"),
            "pub fn entry() { helper(); }\n\
             pub fn helper() { local(); }\n\
             pub fn local() {}\n",
        )
        .expect("Failed to write initial file");

        let project = Project::new(temp_dir.path(), "symbol-diff-project", None).expect("project");
        let mut scanner = ProjectScanner::new();
        let files = scanner
            .discover_files(&project)
            .expect("Failed to discover files");
        let parsed = scanner
            .parse_file(&project, &files[0])
            .expect("Failed to parse initial file");

        scanner
            .persist_scan(
                &project,
                false,
                Some(crate::watcher::IncrementalUpdatePlan {
                    updates: vec![crate::watcher::IncrementalFileUpdate {
                        absolute_path: files[0].clone(),
                        relative_path: "src/lib.rs".to_string(),
                        kind: crate::watcher::UpdateKind::Added,
                        hash: Some(parsed.file_state.hash.clone()),
                        last_modified: Some(parsed.file_state.last_modified),
                    }],
                    skipped: Vec::new(),
                }),
                vec![parsed],
            )
            .expect("Failed to persist initial scan");

        let storage = Storage::new(&project.database_path()).expect("Open storage");
        let entry = storage
            .get_symbol_by_qualified_name("src/lib.rs::entry")
            .expect("Lookup entry")
            .expect("entry symbol should exist");
        let helper = storage
            .get_symbol_by_qualified_name("src/lib.rs::helper")
            .expect("Lookup helper")
            .expect("helper symbol should exist");
        let entry_dep_id = storage
            .get_dependencies_from(&entry.id)
            .expect("Load entry dependencies")
            .into_iter()
            .find(|dependency| dependency.to_symbol == helper.id)
            .expect("entry should depend on helper")
            .id;
        drop(storage);

        std::fs::write(
            temp_dir.path().join("src/lib.rs"),
            "pub fn entry() { helper(); }\n\
             pub fn helper() { other(); }\n\
             pub fn local() {}\n\
             pub fn other() {}\n",
        )
        .expect("Failed to rewrite file");

        let updated_files = scanner
            .discover_files(&project)
            .expect("Failed to rediscover files");
        let plan = scanner
            .plan_updates(&project, &updated_files)
            .expect("Failed to plan updates");
        let updated_parsed = scanner
            .parse_file(&project, &updated_files[0])
            .expect("Failed to parse updated file");

        scanner
            .persist_scan(&project, false, Some(plan), vec![updated_parsed])
            .expect("Failed to persist updated scan");

        let storage = Storage::new(&project.database_path()).expect("Reopen storage");
        let entry = storage
            .get_symbol_by_qualified_name("src/lib.rs::entry")
            .expect("Lookup entry after update")
            .expect("entry symbol should exist after update");
        let helper = storage
            .get_symbol_by_qualified_name("src/lib.rs::helper")
            .expect("Lookup helper after update")
            .expect("helper symbol should exist after update");
        let entry_dep = storage
            .get_dependencies_from(&entry.id)
            .expect("Load entry dependencies after update")
            .into_iter()
            .find(|dependency| dependency.to_symbol == helper.id)
            .expect("entry should still depend on helper");
        let helper_targets = storage
            .get_dependencies_from(&helper.id)
            .expect("Load helper dependencies after update")
            .into_iter()
            .map(|dependency| dependency.to_symbol)
            .collect::<Vec<_>>();
        let other = storage
            .get_symbol_by_qualified_name("src/lib.rs::other")
            .expect("Lookup other")
            .expect("other symbol should exist");
        let local = storage
            .get_symbol_by_qualified_name("src/lib.rs::local")
            .expect("Lookup local")
            .expect("local symbol should exist");

        assert_eq!(entry_dep.id, entry_dep_id);
        assert!(helper_targets.contains(&other.id));
        assert!(!helper_targets.contains(&local.id));
    }
}
