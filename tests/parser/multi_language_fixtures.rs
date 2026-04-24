use std::fs;

use quickdep::parser::{
    detect_language, CParser, CSharpParser, CppParser, GoParser, JavaParser, KotlinParser,
    Language, ObjcParser, Parser, PhpParser, PythonParser, RubyParser, RustParser, SwiftParser,
    TypeScriptParser,
};

use crate::common::fixture_path;

#[test]
fn parses_all_supported_language_fixtures() {
    let rust_path = fixture_path("rust/src/main.rs");
    let rust_source = fs::read(&rust_path).expect("failed to read rust fixture");
    assert_eq!(detect_language(&rust_path), Some(Language::Rust));
    let mut rust_parser = RustParser::new();
    let rust = rust_parser.parse_file(&rust_path, &rust_source, "src/main.rs");
    assert_eq!(rust.error_count, 0);
    assert!(rust.symbols.iter().any(|symbol| symbol.name == "main"));
    assert!(!rust.dependencies.is_empty());

    let typescript_path = fixture_path("typescript/sample.ts");
    let typescript_source = fs::read(&typescript_path).expect("failed to read ts fixture");
    assert_eq!(
        detect_language(&typescript_path),
        Some(Language::TypeScript)
    );
    let mut typescript_parser = TypeScriptParser::new();
    let typescript =
        typescript_parser.parse_file(&typescript_path, &typescript_source, "src/sample.ts");
    assert_eq!(typescript.error_count, 0);
    assert!(typescript
        .symbols
        .iter()
        .any(|symbol| symbol.name == "UserService"));
    assert_eq!(typescript.imports.len(), 1);
    assert!(!typescript.dependencies.is_empty());

    let javascript_path = fixture_path("javascript/sample.js");
    let javascript_source = fs::read(&javascript_path).expect("failed to read js fixture");
    assert_eq!(
        detect_language(&javascript_path),
        Some(Language::JavaScript)
    );
    let javascript =
        typescript_parser.parse_file(&javascript_path, &javascript_source, "src/sample.js");
    assert_eq!(javascript.error_count, 0);
    assert!(javascript
        .symbols
        .iter()
        .any(|symbol| symbol.name == "UserService"));
    assert_eq!(javascript.imports.len(), 1);
    assert!(!javascript.dependencies.is_empty());

    let java_path = fixture_path("java/sample.java");
    let java_source = fs::read(&java_path).expect("failed to read java fixture");
    assert_eq!(detect_language(&java_path), Some(Language::Java));
    let mut java_parser = JavaParser::new();
    let java = java_parser.parse_file(&java_path, &java_source, "src/sample.java");
    assert_eq!(java.error_count, 0);
    assert!(java
        .symbols
        .iter()
        .any(|symbol| symbol.name == "UserService"));
    assert_eq!(java.imports.len(), 2);
    assert!(!java.dependencies.is_empty());

    let csharp_path = fixture_path("csharp/Sample.cs");
    let csharp_source = fs::read(&csharp_path).expect("failed to read csharp fixture");
    assert_eq!(detect_language(&csharp_path), Some(Language::CSharp));
    let mut csharp_parser = CSharpParser::new();
    let csharp = csharp_parser.parse_file(&csharp_path, &csharp_source, "src/Sample.cs");
    assert_eq!(csharp.error_count, 0);
    assert!(csharp
        .symbols
        .iter()
        .any(|symbol| symbol.name == "UserService"));
    assert_eq!(csharp.imports.len(), 2);
    assert!(!csharp.dependencies.is_empty());

    let kotlin_path = fixture_path("kotlin/sample.kt");
    let kotlin_source = fs::read(&kotlin_path).expect("failed to read kotlin fixture");
    assert_eq!(detect_language(&kotlin_path), Some(Language::Kotlin));
    let mut kotlin_parser = KotlinParser::new();
    let kotlin = kotlin_parser.parse_file(&kotlin_path, &kotlin_source, "src/sample.kt");
    assert_eq!(kotlin.error_count, 0);
    assert!(kotlin
        .symbols
        .iter()
        .any(|symbol| symbol.name == "UserService"));
    assert_eq!(kotlin.imports.len(), 3);
    assert!(!kotlin.dependencies.is_empty());

    let php_path = fixture_path("php/sample.php");
    let php_source = fs::read(&php_path).expect("failed to read php fixture");
    assert_eq!(detect_language(&php_path), Some(Language::Php));
    let mut php_parser = PhpParser::new();
    let php = php_parser.parse_file(&php_path, &php_source, "src/sample.php");
    assert_eq!(php.error_count, 0);
    assert!(php
        .symbols
        .iter()
        .any(|symbol| symbol.name == "UserService"));
    assert_eq!(php.imports.len(), 3);
    assert!(!php.dependencies.is_empty());

    let ruby_path = fixture_path("ruby/sample.rb");
    let ruby_source = fs::read(&ruby_path).expect("failed to read ruby fixture");
    assert_eq!(detect_language(&ruby_path), Some(Language::Ruby));
    let mut ruby_parser = RubyParser::new();
    let ruby = ruby_parser.parse_file(&ruby_path, &ruby_source, "lib/sample.rb");
    assert_eq!(ruby.error_count, 0);
    assert!(ruby
        .symbols
        .iter()
        .any(|symbol| symbol.name == "UserService"));
    assert_eq!(ruby.imports.len(), 1);
    assert!(!ruby.dependencies.is_empty());

    let swift_path = fixture_path("swift/sample.swift");
    let swift_source = fs::read(&swift_path).expect("failed to read swift fixture");
    assert_eq!(detect_language(&swift_path), Some(Language::Swift));
    let mut swift_parser = SwiftParser::new();
    let swift = swift_parser.parse_file(&swift_path, &swift_source, "Sources/App/sample.swift");
    assert_eq!(swift.error_count, 0);
    assert!(swift
        .symbols
        .iter()
        .any(|symbol| symbol.name == "InMemoryRepo"));
    assert_eq!(swift.imports.len(), 1);
    assert!(!swift.dependencies.is_empty());

    let objc_path = fixture_path("objc/sample.m");
    let objc_source = fs::read(&objc_path).expect("failed to read objc fixture");
    assert_eq!(detect_language(&objc_path), Some(Language::Objc));
    let mut objc_parser = ObjcParser::new();
    let objc = objc_parser.parse_file(&objc_path, &objc_source, "src/sample.m");
    assert_eq!(objc.error_count, 0);
    assert!(objc
        .symbols
        .iter()
        .any(|symbol| symbol.name == "Calculator"));
    assert_eq!(objc.imports.len(), 2);
    assert!(!objc.dependencies.is_empty());

    let python_path = fixture_path("python/sample.py");
    let python_source = fs::read(&python_path).expect("failed to read python fixture");
    assert_eq!(detect_language(&python_path), Some(Language::Python));
    let mut python_parser = PythonParser::new();
    let python = python_parser.parse_file(&python_path, &python_source, "src/sample.py");
    assert_eq!(python.error_count, 0);
    assert!(python.symbols.iter().any(|symbol| symbol.name == "Greeter"));
    assert!(!python.imports.is_empty());
    assert!(!python.dependencies.is_empty());

    let go_path = fixture_path("go/sample.go");
    let go_source = fs::read(&go_path).expect("failed to read go fixture");
    assert_eq!(detect_language(&go_path), Some(Language::Go));
    let mut go_parser = GoParser::new();
    let go = go_parser.parse_file(&go_path, &go_source, "src/sample.go");
    assert_eq!(go.error_count, 0);
    assert!(go.symbols.iter().any(|symbol| symbol.name == "FormatName"));
    assert_eq!(go.imports.len(), 2);
    assert!(!go.dependencies.is_empty());

    let c_path = fixture_path("c/sample.c");
    let c_source = fs::read(&c_path).expect("failed to read c fixture");
    assert_eq!(detect_language(&c_path), Some(Language::C));
    let mut c_parser = CParser::new();
    let c = c_parser.parse_file(&c_path, &c_source, "src/sample.c");
    assert_eq!(c.error_count, 0);
    assert!(c.symbols.iter().any(|symbol| symbol.name == "run"));
    assert_eq!(c.imports.len(), 2);
    assert!(!c.dependencies.is_empty());

    let cpp_path = fixture_path("cpp/sample.cpp");
    let cpp_source = fs::read(&cpp_path).expect("failed to read cpp fixture");
    assert_eq!(detect_language(&cpp_path), Some(Language::Cpp));
    let mut cpp_parser = CppParser::new();
    let cpp = cpp_parser.parse_file(&cpp_path, &cpp_source, "src/sample.cpp");
    assert_eq!(cpp.error_count, 0);
    assert!(cpp
        .symbols
        .iter()
        .any(|symbol| symbol.name == "UserService"));
    assert_eq!(cpp.imports.len(), 2);
    assert!(!cpp.dependencies.is_empty());
}
