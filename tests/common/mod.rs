#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

pub fn fixture_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(relative)
}

pub fn read_fixture(relative: &str) -> String {
    fs::read_to_string(fixture_path(relative)).expect("failed to read fixture")
}

pub fn write_file(path: impl AsRef<Path>, contents: &str) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("failed to create parent directory");
    }
    fs::write(path, contents).expect("failed to write file");
}

pub fn create_simple_rust_workspace() -> TempDir {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    write_file(
        temp_dir.path().join("src/lib.rs"),
        r#"
pub fn entry() {
    helper();
}

pub fn helper() {}
"#,
    );
    temp_dir
}

pub fn create_typescript_workspace() -> TempDir {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    write_file(
        temp_dir.path().join("src/shared.ts"),
        r#"
export function formatName(name: string): string {
  return name.trim();
}
"#,
    );
    write_file(
        temp_dir.path().join("src/service.ts"),
        r#"
import { formatName as format } from "./shared";

export function run(name: string): string {
  return format(name);
}
"#,
    );
    temp_dir
}

pub fn create_mixed_workspace() -> TempDir {
    let temp_dir = TempDir::new().expect("failed to create temp dir");
    write_file(
        temp_dir.path().join("quickdep.toml"),
        r#"
[scan]
include = ["src/**"]
exclude = []
languages = ["rust", "typescript", "python", "go", "c", "cpp"]
"#,
    );
    write_file(
        temp_dir.path().join("Cargo.toml"),
        &read_fixture("rust/Cargo.toml"),
    );
    write_file(
        temp_dir.path().join("src/main.rs"),
        &read_fixture("rust/src/main.rs"),
    );
    write_file(
        temp_dir.path().join("src/models.rs"),
        &read_fixture("rust/src/models.rs"),
    );
    write_file(
        temp_dir.path().join("src/utils.rs"),
        &read_fixture("rust/src/utils.rs"),
    );
    write_file(
        temp_dir.path().join("src/sample.ts"),
        &read_fixture("typescript/sample.ts"),
    );
    write_file(
        temp_dir.path().join("src/shared.ts"),
        r#"
export function formatName(name: string): string {
  return name.trim();
}
"#,
    );
    write_file(
        temp_dir.path().join("src/sample.py"),
        &read_fixture("python/sample.py"),
    );
    write_file(
        temp_dir.path().join("src/helpers.py"),
        r#"
def format_name(value):
    return value.strip()
"#,
    );
    write_file(
        temp_dir.path().join("src/sample.go"),
        &read_fixture("go/sample.go"),
    );
    write_file(
        temp_dir.path().join("src/sample.c"),
        &read_fixture("c/sample.c"),
    );
    write_file(
        temp_dir.path().join("src/sample.cpp"),
        &read_fixture("cpp/sample.cpp"),
    );
    temp_dir
}
