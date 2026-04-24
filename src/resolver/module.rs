//! Rust module-path helpers used by the resolver.

use crate::core::Symbol;

/// Convert a Rust source file path into a crate-relative module path.
///
/// Examples:
/// - `src/lib.rs` -> `""`
/// - `src/models.rs` -> `"models"`
/// - `src/api/mod.rs` -> `"api"`
/// - `src/api/http.rs` -> `"api::http"`
pub fn rust_module_path(file_path: &str) -> String {
    let normalized = file_path.replace('\\', "/");
    let trimmed = normalized.strip_prefix("src/").unwrap_or(&normalized);

    if trimmed == "lib.rs" || trimmed == "main.rs" {
        return String::new();
    }

    let without_ext = trimmed.strip_suffix(".rs").unwrap_or(trimmed);
    let module_path = without_ext
        .strip_suffix("/mod")
        .unwrap_or(without_ext)
        .replace('/', "::");

    if module_path == "mod" {
        String::new()
    } else {
        module_path
    }
}

/// Normalize a Rust path relative to the current file's module.
///
/// Handles `crate::`, `self::`, and repeated `super::` prefixes.
pub fn normalize_module_path(path: &str, file_module_path: &str) -> String {
    if let Some(rest) = path.strip_prefix("crate::") {
        return rest.to_string();
    }

    if let Some(rest) = path.strip_prefix("self::") {
        if file_module_path.is_empty() {
            return rest.to_string();
        }
        return format!("{file_module_path}::{rest}");
    }

    if path.starts_with("super::") {
        let mut current_module = file_module_path.to_string();
        let mut remainder = path;

        while let Some(rest) = remainder.strip_prefix("super::") {
            current_module = current_module
                .rsplit_once("::")
                .map(|(parent, _)| parent.to_string())
                .unwrap_or_default();
            remainder = rest;
        }

        if current_module.is_empty() {
            return remainder.to_string();
        }

        return format!("{current_module}::{remainder}");
    }

    path.to_string()
}

/// Convert a stored symbol into its crate-relative Rust path.
pub fn symbol_rust_path(symbol: &Symbol) -> String {
    let prefix = format!("{}::", symbol.file_path);
    let tail = symbol
        .qualified_name
        .strip_prefix(&prefix)
        .unwrap_or(symbol.name.as_str());
    let module_path = rust_module_path(&symbol.file_path);

    if module_path.is_empty() {
        tail.to_string()
    } else {
        format!("{module_path}::{tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::SymbolKind;

    #[test]
    fn test_rust_module_path() {
        assert_eq!(rust_module_path("src/lib.rs"), "");
        assert_eq!(rust_module_path("src/main.rs"), "");
        assert_eq!(rust_module_path("src/models.rs"), "models");
        assert_eq!(rust_module_path("src/api/mod.rs"), "api");
        assert_eq!(rust_module_path("src/api/http.rs"), "api::http");
    }

    #[test]
    fn test_normalize_module_path() {
        assert_eq!(
            normalize_module_path("crate::models::User", "api"),
            "models::User"
        );
        assert_eq!(normalize_module_path("self::client", "api"), "api::client");
        assert_eq!(
            normalize_module_path("super::shared", "api::http"),
            "api::shared"
        );
        assert_eq!(
            normalize_module_path("super::super::shared::Client", "api::http::v1"),
            "api::shared::Client"
        );
        assert_eq!(normalize_module_path("models::User", "api"), "models::User");
    }

    #[test]
    fn test_symbol_rust_path() {
        let symbol = Symbol::new(
            "new".to_string(),
            "src/models.rs::User::new".to_string(),
            SymbolKind::Method,
            "src/models.rs".to_string(),
            1,
            1,
        );

        assert_eq!(symbol_rust_path(&symbol), "models::User::new");
    }
}
