//! Embedded tree-sitter query sources for supported languages.
//!
//! These `.scm` files document QuickDep's extraction rules and can be
//! compiled to validate that the checked-in queries stay aligned with the
//! bundled grammars.

use crate::parser::Language;

/// Rust query source.
pub const RUST_QUERY: &str = include_str!("../../queries/rust.scm");

/// TypeScript query source.
pub const TYPESCRIPT_QUERY: &str = include_str!("../../queries/typescript.scm");

/// Python query source.
pub const PYTHON_QUERY: &str = include_str!("../../queries/python.scm");

/// Go query source.
pub const GO_QUERY: &str = include_str!("../../queries/go.scm");

/// Return the checked-in query source for a supported language.
#[must_use]
pub fn query_source(language: Language) -> Option<&'static str> {
    match language {
        Language::Rust => Some(RUST_QUERY),
        Language::TypeScript => Some(TYPESCRIPT_QUERY),
        Language::Python => Some(PYTHON_QUERY),
        Language::Go => Some(GO_QUERY),
        Language::JavaScript
        | Language::Java
        | Language::CSharp
        | Language::Kotlin
        | Language::Php
        | Language::Ruby
        | Language::Swift
        | Language::Objc
        | Language::C
        | Language::Cpp => None,
    }
}

/// Compile the checked-in query for a supported language.
///
/// Returns `None` when QuickDep does not currently ship a query for the
/// requested language. For supported languages, the inner `Result` reports
/// whether the checked-in `.scm` source compiled against the grammar.
#[must_use]
pub fn compile_query(
    language: Language,
) -> Option<Result<tree_sitter::Query, tree_sitter::QueryError>> {
    let language_impl = match language {
        Language::Rust => tree_sitter_rust::LANGUAGE.into(),
        Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Language::Python => tree_sitter_python::LANGUAGE.into(),
        Language::Go => tree_sitter_go::LANGUAGE.into(),
        Language::JavaScript
        | Language::Java
        | Language::CSharp
        | Language::Kotlin
        | Language::Php
        | Language::Ruby
        | Language::Swift
        | Language::Objc
        | Language::C
        | Language::Cpp => return None,
    };

    Some(tree_sitter::Query::new(
        &language_impl,
        query_source(language).unwrap_or_default(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_queries_compile() {
        for language in [
            Language::Rust,
            Language::TypeScript,
            Language::Python,
            Language::Go,
        ] {
            let query = compile_query(language)
                .expect("supported languages should ship query files")
                .unwrap_or_else(|error| {
                    panic!("query for {} should compile: {error}", language.name())
                });
            let capture_names = query.capture_names();
            assert!(
                capture_names
                    .iter()
                    .any(|name| name.starts_with("definition.")),
                "query for {} should define symbol captures",
                language.name()
            );
            assert!(
                capture_names.contains(&"import"),
                "query for {} should define import captures",
                language.name()
            );
            assert!(
                capture_names
                    .iter()
                    .any(|name| name.starts_with("reference.call")),
                "query for {} should define call captures",
                language.name()
            );
        }
    }

    #[test]
    fn test_languages_without_checked_in_queries_return_none() {
        assert!(query_source(Language::JavaScript).is_none());
        assert!(compile_query(Language::JavaScript).is_none());
        assert!(query_source(Language::Java).is_none());
        assert!(compile_query(Language::Java).is_none());
        assert!(query_source(Language::CSharp).is_none());
        assert!(compile_query(Language::CSharp).is_none());
        assert!(query_source(Language::Kotlin).is_none());
        assert!(compile_query(Language::Kotlin).is_none());
        assert!(query_source(Language::Php).is_none());
        assert!(compile_query(Language::Php).is_none());
        assert!(query_source(Language::Ruby).is_none());
        assert!(compile_query(Language::Ruby).is_none());
        assert!(query_source(Language::Swift).is_none());
        assert!(compile_query(Language::Swift).is_none());
        assert!(query_source(Language::Objc).is_none());
        assert!(compile_query(Language::Objc).is_none());
        assert!(query_source(Language::C).is_none());
        assert!(compile_query(Language::C).is_none());
        assert!(query_source(Language::Cpp).is_none());
        assert!(compile_query(Language::Cpp).is_none());
    }
}
