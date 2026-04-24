use std::fs;

use quickdep::parser::{Language, Parser, TypeScriptParser};
use quickdep::resolver::Resolver;

use crate::common::create_typescript_workspace;

#[test]
fn resolves_typescript_alias_imports_across_files() {
    let workspace = create_typescript_workspace();
    let shared_path = workspace.path().join("src/shared.ts");
    let shared_source = fs::read(&shared_path).expect("failed to read shared fixture");
    let service_path = workspace.path().join("src/service.ts");
    let service_source = fs::read(&service_path).expect("failed to read service fixture");

    let mut parser = TypeScriptParser::new();
    let shared = parser.parse_file(&shared_path, &shared_source, "src/shared.ts");
    let service = parser.parse_file(&service_path, &service_source, "src/service.ts");

    let mut all_symbols = shared.symbols.clone();
    all_symbols.extend(service.symbols.clone());

    let resolver = Resolver::new(&all_symbols);
    let summary = resolver.resolve_dependencies_with_language(
        "src/service.ts",
        &service.imports,
        &service.dependencies,
        Some(Language::TypeScript),
    );

    assert!(summary.unresolved.is_empty());
    assert_eq!(summary.resolved.len(), 1);

    let target_symbol = all_symbols
        .iter()
        .find(|symbol| symbol.id == summary.resolved[0].to_symbol)
        .expect("resolved target should exist");
    assert_eq!(target_symbol.qualified_name, "src/shared.ts::formatName");
}
