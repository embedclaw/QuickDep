use quickdep::core::{Dependency, DependencyKind, Symbol, SymbolKind};
use quickdep::storage::{FileState, Storage};
use tempfile::TempDir;

#[test]
fn persists_symbols_dependencies_and_search_index_across_reopen() {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    let database_path = temp_dir.path().join("symbols.db");

    let process_symbol = Symbol::new(
        "process_data".to_string(),
        "src/lib.rs::process_data".to_string(),
        SymbolKind::Function,
        "src/lib.rs".to_string(),
        10,
        1,
    );
    let processor_symbol = Symbol::new(
        "DataProcessor".to_string(),
        "src/lib.rs::DataProcessor".to_string(),
        SymbolKind::Class,
        "src/lib.rs".to_string(),
        20,
        1,
    );
    let helper_symbol = Symbol::new(
        "helper".to_string(),
        "src/lib.rs::helper".to_string(),
        SymbolKind::Function,
        "src/lib.rs".to_string(),
        30,
        1,
    );

    {
        let storage = Storage::new(&database_path).expect("failed to create storage");
        storage
            .insert_symbol(&process_symbol)
            .expect("failed to insert process symbol");
        storage
            .insert_symbol(&processor_symbol)
            .expect("failed to insert processor symbol");
        storage
            .insert_symbol(&helper_symbol)
            .expect("failed to insert helper symbol");
        storage
            .insert_dependency(&Dependency::new(
                process_symbol.id.clone(),
                helper_symbol.id.clone(),
                "src/lib.rs".to_string(),
                12,
                DependencyKind::Call,
            ))
            .expect("failed to insert dependency");
        storage
            .upsert_file_state(&FileState::new(
                "src/lib.rs".to_string(),
                "hash-1".to_string(),
                123,
            ))
            .expect("failed to upsert file state");
    }

    let storage = Storage::new(&database_path).expect("failed to reopen storage");
    assert!(storage
        .get_symbol_by_qualified_name("src/lib.rs::process_data")
        .expect("lookup failed")
        .is_some());
    assert_eq!(
        storage
            .get_all_file_states()
            .expect("failed to load file states")
            .len(),
        1
    );

    let search_names = storage
        .search_symbols("process", 10)
        .expect("search failed")
        .into_iter()
        .map(|symbol| symbol.name)
        .collect::<Vec<_>>();
    assert!(search_names.iter().any(|name| name == "process_data"));
    assert!(search_names.iter().any(|name| name == "DataProcessor"));

    let chain = storage
        .get_dependency_chain_forward(&process_symbol.id, 5)
        .expect("failed to load dependency chain");
    assert!(chain.iter().any(|node| node.symbol_id == process_symbol.id));
    assert!(chain.iter().any(|node| node.symbol_id == helper_symbol.id));
}
