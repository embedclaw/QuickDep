//! Parser module for extracting symbols and dependencies from source code.
//!
//! This module provides:
//! - Parser trait for language-specific parsers
//! - Language detection based on file extensions
//! - Tree-sitter based parsers for Rust, TypeScript, Ruby, Swift, Objective-C, Python, Go, C, C++

pub mod c;
pub mod cpp;
pub mod csharp;
pub mod go;
pub mod java;
pub mod kotlin;
pub mod language;
pub mod objc;
pub mod parser_trait;
pub mod php;
pub mod python;
pub mod queries;
pub mod ruby;
pub mod rust;
pub mod swift;
pub mod typescript;

pub use c::CParser;
pub use cpp::CppParser;
pub use csharp::CSharpParser;
pub use go::GoParser;
pub use java::JavaParser;
pub use kotlin::KotlinParser;
pub use language::{detect_language, detect_language_with_map, Language};
pub use objc::ObjcParser;
pub use parser_trait::{make_qualified_name, node_text, ParseResult, Parser};
pub use php::PhpParser;
pub use python::PythonParser;
pub use queries::{
    compile_query, query_source, GO_QUERY, PYTHON_QUERY, RUST_QUERY, TYPESCRIPT_QUERY,
};
pub use ruby::RubyParser;
pub use rust::RustParser;
pub use swift::SwiftParser;
pub use typescript::TypeScriptParser;
