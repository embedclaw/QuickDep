use quickdep::{
    cli::{run_debug, run_scan, run_status},
    storage::Storage,
    CACHE_DIR, DB_FILE,
};

use crate::common::create_mixed_workspace;

#[tokio::test]
async fn scans_and_queries_a_mixed_language_workspace() {
    let workspace = create_mixed_workspace();

    let scan = run_scan(workspace.path(), false)
        .await
        .expect("scan command should succeed");
    assert!(
        scan["stats"]["symbols"]
            .as_u64()
            .expect("symbols should be numeric")
            >= 20
    );
    assert!(
        scan["stats"]["dependencies"]
            .as_u64()
            .expect("dependencies should be numeric")
            >= 6
    );

    let status = run_status(workspace.path())
        .await
        .expect("status command should succeed");
    assert_eq!(
        status["manifest"]["config"]["languages"]
            .as_array()
            .expect("languages should be an array")
            .len(),
        6
    );

    let debug = run_debug(workspace.path(), false, None, Some("src/sample.ts"))
        .await
        .expect("debug command should succeed");
    let interface_names = debug["file_interfaces"]["interfaces"]
        .as_array()
        .expect("interfaces should be an array")
        .iter()
        .filter_map(|value| value["name"].as_str())
        .collect::<Vec<_>>();
    assert!(interface_names.contains(&"UserService"));
    assert!(interface_names.contains(&"run"));

    let storage = Storage::new(&workspace.path().join(CACHE_DIR).join(DB_FILE))
        .expect("database should exist after scan");
    let format_results = storage
        .search_symbols("format", 20)
        .expect("format search should succeed");
    assert!(format_results
        .iter()
        .any(|symbol| symbol.qualified_name.ends_with("src/shared.ts::formatName")));
    assert!(format_results.iter().any(|symbol| symbol
        .qualified_name
        .ends_with("src/sample.py::format_name")));
    assert!(format_results
        .iter()
        .any(|symbol| symbol.qualified_name.ends_with("src/sample.go::FormatName")));
    assert!(storage
        .search_symbols("helper", 20)
        .expect("helper search should succeed")
        .iter()
        .any(|symbol| symbol.qualified_name.ends_with("src/sample.c::helper")));
    assert!(storage
        .search_symbols("UserService", 20)
        .expect("UserService search should succeed")
        .iter()
        .any(|symbol| symbol
            .qualified_name
            .ends_with("src/sample.cpp::app::UserService")));
}
