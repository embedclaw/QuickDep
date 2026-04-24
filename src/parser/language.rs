//! Language detection based on file extensions.
//!
//! Maps file extensions to supported languages for parser selection.

use std::collections::HashMap;
use std::path::Path;

/// Supported programming languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Java,
    CSharp,
    Kotlin,
    Php,
    Ruby,
    Swift,
    Objc,
    Python,
    Go,
    C,
    Cpp,
}

impl Language {
    /// Returns the language name as a string.
    pub fn name(&self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::TypeScript => "typescript",
            Language::JavaScript => "javascript",
            Language::Java => "java",
            Language::CSharp => "csharp",
            Language::Kotlin => "kotlin",
            Language::Php => "php",
            Language::Ruby => "ruby",
            Language::Swift => "swift",
            Language::Objc => "objc",
            Language::Python => "python",
            Language::Go => "go",
            Language::C => "c",
            Language::Cpp => "cpp",
        }
    }

    /// Returns supported file extensions for this language.
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Language::Rust => &["rs"],
            Language::TypeScript => &["ts", "tsx"],
            Language::JavaScript => &["js", "jsx", "mjs", "cjs"],
            Language::Java => &["java"],
            Language::CSharp => &["cs"],
            Language::Kotlin => &["kt", "kts"],
            Language::Php => &["php", "phtml"],
            Language::Ruby => &["rb", "rake"],
            Language::Swift => &["swift"],
            Language::Objc => &["m"],
            Language::Python => &["py", "pyi"],
            Language::Go => &["go"],
            Language::C => &["c", "h"],
            Language::Cpp => &["cc", "cpp", "cxx", "hh", "hpp", "hxx"],
        }
    }

    /// Parse a language name from configuration into a supported language.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "rust" => Some(Language::Rust),
            "typescript" => Some(Language::TypeScript),
            "javascript" => Some(Language::JavaScript),
            "java" => Some(Language::Java),
            "csharp" | "c#" => Some(Language::CSharp),
            "kotlin" => Some(Language::Kotlin),
            "php" => Some(Language::Php),
            "ruby" => Some(Language::Ruby),
            "swift" => Some(Language::Swift),
            "objc" | "objective-c" => Some(Language::Objc),
            "python" => Some(Language::Python),
            "go" => Some(Language::Go),
            "c" => Some(Language::C),
            "cpp" | "c++" => Some(Language::Cpp),
            _ => None,
        }
    }
}

/// Mapping of file extensions to languages.
const EXTENSION_MAP: &[(&str, Language)] = &[
    // Rust
    ("rs", Language::Rust),
    // TypeScript
    ("ts", Language::TypeScript),
    ("tsx", Language::TypeScript),
    // JavaScript
    ("js", Language::JavaScript),
    ("jsx", Language::JavaScript),
    ("mjs", Language::JavaScript),
    ("cjs", Language::JavaScript),
    // Java
    ("java", Language::Java),
    // C#
    ("cs", Language::CSharp),
    // Kotlin
    ("kt", Language::Kotlin),
    ("kts", Language::Kotlin),
    // PHP
    ("php", Language::Php),
    ("phtml", Language::Php),
    // Ruby
    ("rb", Language::Ruby),
    ("rake", Language::Ruby),
    // Swift
    ("swift", Language::Swift),
    // Objective-C
    ("m", Language::Objc),
    // Python
    ("py", Language::Python),
    ("pyi", Language::Python),
    // Go
    ("go", Language::Go),
    // C
    ("c", Language::C),
    ("h", Language::C),
    // C++
    ("cc", Language::Cpp),
    ("cpp", Language::Cpp),
    ("cxx", Language::Cpp),
    ("hh", Language::Cpp),
    ("hpp", Language::Cpp),
    ("hxx", Language::Cpp),
];

/// Detect language from file extension.
///
/// # Arguments
/// * `path` - Path to the file
///
/// # Returns
/// Language if the extension is recognized, None otherwise.
pub fn detect_language(path: &Path) -> Option<Language> {
    let ext = normalized_extension(path.extension()?.to_str()?)?;

    for (extension, language) in EXTENSION_MAP {
        if ext == *extension {
            return Some(*language);
        }
    }

    None
}

/// Detect language from file extension with configuration overrides.
///
/// Override keys may include the leading dot (for example `.vue`) or omit it (`vue`).
/// Override values must match supported language names.
#[must_use]
pub fn detect_language_with_map(
    path: &Path,
    extension_map: &HashMap<String, String>,
) -> Option<Language> {
    let ext = normalized_extension(path.extension()?.to_str()?)?;

    if let Some(language) = extension_map
        .get(&format!(".{ext}"))
        .or_else(|| extension_map.get(&ext))
        .and_then(|configured_language| Language::from_name(configured_language))
    {
        return Some(language);
    }

    detect_language(path)
}

/// Get all supported extensions.
pub fn all_extensions() -> Vec<&'static str> {
    EXTENSION_MAP.iter().map(|(ext, _)| *ext).collect()
}

fn normalized_extension(extension: &str) -> Option<String> {
    let normalized = extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_detect_rust() {
        assert_eq!(
            detect_language(&PathBuf::from("src/main.rs")),
            Some(Language::Rust)
        );
        assert_eq!(
            detect_language(&PathBuf::from("lib.rs")),
            Some(Language::Rust)
        );
    }

    #[test]
    fn test_detect_typescript() {
        assert_eq!(
            detect_language(&PathBuf::from("app.ts")),
            Some(Language::TypeScript)
        );
        assert_eq!(
            detect_language(&PathBuf::from("component.tsx")),
            Some(Language::TypeScript)
        );
    }

    #[test]
    fn test_detect_javascript() {
        assert_eq!(
            detect_language(&PathBuf::from("index.js")),
            Some(Language::JavaScript)
        );
        assert_eq!(
            detect_language(&PathBuf::from("app.jsx")),
            Some(Language::JavaScript)
        );
    }

    #[test]
    fn test_detect_java() {
        assert_eq!(
            detect_language(&PathBuf::from("src/main/java/App.java")),
            Some(Language::Java)
        );
    }

    #[test]
    fn test_detect_python() {
        assert_eq!(
            detect_language(&PathBuf::from("main.py")),
            Some(Language::Python)
        );
        assert_eq!(
            detect_language(&PathBuf::from("utils.pyi")),
            Some(Language::Python)
        );
    }

    #[test]
    fn test_detect_csharp() {
        assert_eq!(
            detect_language(&PathBuf::from("src/App.cs")),
            Some(Language::CSharp)
        );
    }

    #[test]
    fn test_detect_kotlin() {
        assert_eq!(
            detect_language(&PathBuf::from("src/App.kt")),
            Some(Language::Kotlin)
        );
        assert_eq!(
            detect_language(&PathBuf::from("build.gradle.kts")),
            Some(Language::Kotlin)
        );
    }

    #[test]
    fn test_detect_php() {
        assert_eq!(
            detect_language(&PathBuf::from("src/App.php")),
            Some(Language::Php)
        );
        assert_eq!(
            detect_language(&PathBuf::from("templates/index.phtml")),
            Some(Language::Php)
        );
    }

    #[test]
    fn test_detect_ruby() {
        assert_eq!(
            detect_language(&PathBuf::from("app/models/user.rb")),
            Some(Language::Ruby)
        );
        assert_eq!(
            detect_language(&PathBuf::from("tasks/release.rake")),
            Some(Language::Ruby)
        );
    }

    #[test]
    fn test_detect_swift() {
        assert_eq!(
            detect_language(&PathBuf::from("Sources/App/main.swift")),
            Some(Language::Swift)
        );
    }

    #[test]
    fn test_detect_objc() {
        assert_eq!(
            detect_language(&PathBuf::from("src/main.m")),
            Some(Language::Objc)
        );
    }

    #[test]
    fn test_detect_go() {
        assert_eq!(
            detect_language(&PathBuf::from("main.go")),
            Some(Language::Go)
        );
    }

    #[test]
    fn test_detect_c() {
        assert_eq!(detect_language(&PathBuf::from("main.c")), Some(Language::C));
        assert_eq!(
            detect_language(&PathBuf::from("include/app.h")),
            Some(Language::C)
        );
    }

    #[test]
    fn test_detect_cpp() {
        assert_eq!(
            detect_language(&PathBuf::from("main.cpp")),
            Some(Language::Cpp)
        );
        assert_eq!(
            detect_language(&PathBuf::from("include/app.hpp")),
            Some(Language::Cpp)
        );
    }

    #[test]
    fn test_unknown_extension() {
        assert_eq!(detect_language(&PathBuf::from("README.md")), None);
        assert_eq!(detect_language(&PathBuf::from("config.toml")), None);
    }

    #[test]
    fn test_no_extension() {
        assert_eq!(detect_language(&PathBuf::from("Makefile")), None);
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(
            detect_language(&PathBuf::from("main.RS")),
            Some(Language::Rust)
        );
        assert_eq!(
            detect_language(&PathBuf::from("app.TS")),
            Some(Language::TypeScript)
        );
    }

    #[test]
    fn test_language_name() {
        assert_eq!(Language::Rust.name(), "rust");
        assert_eq!(Language::TypeScript.name(), "typescript");
        assert_eq!(Language::Java.name(), "java");
        assert_eq!(Language::CSharp.name(), "csharp");
        assert_eq!(Language::Kotlin.name(), "kotlin");
        assert_eq!(Language::Php.name(), "php");
        assert_eq!(Language::Ruby.name(), "ruby");
        assert_eq!(Language::Swift.name(), "swift");
        assert_eq!(Language::Objc.name(), "objc");
        assert_eq!(Language::Python.name(), "python");
        assert_eq!(Language::C.name(), "c");
        assert_eq!(Language::Cpp.name(), "cpp");
    }

    #[test]
    fn test_language_from_name() {
        assert_eq!(Language::from_name("rust"), Some(Language::Rust));
        assert_eq!(
            Language::from_name("TypeScript"),
            Some(Language::TypeScript)
        );
        assert_eq!(Language::from_name("JAVA"), Some(Language::Java));
        assert_eq!(Language::from_name("c#"), Some(Language::CSharp));
        assert_eq!(Language::from_name("KOTLIN"), Some(Language::Kotlin));
        assert_eq!(Language::from_name("PHP"), Some(Language::Php));
        assert_eq!(Language::from_name("RUBY"), Some(Language::Ruby));
        assert_eq!(Language::from_name("SWIFT"), Some(Language::Swift));
        assert_eq!(Language::from_name("OBJC"), Some(Language::Objc));
        assert_eq!(Language::from_name("objective-c"), Some(Language::Objc));
        assert_eq!(Language::from_name("PYTHON"), Some(Language::Python));
        assert_eq!(Language::from_name("c"), Some(Language::C));
        assert_eq!(Language::from_name("c++"), Some(Language::Cpp));
        assert_eq!(Language::from_name("unknown"), None);
    }

    #[test]
    fn test_all_extensions() {
        let exts = all_extensions();
        assert!(exts.contains(&"rs"));
        assert!(exts.contains(&"ts"));
        assert!(exts.contains(&"java"));
        assert!(exts.contains(&"cs"));
        assert!(exts.contains(&"kt"));
        assert!(exts.contains(&"kts"));
        assert!(exts.contains(&"php"));
        assert!(exts.contains(&"phtml"));
        assert!(exts.contains(&"rb"));
        assert!(exts.contains(&"rake"));
        assert!(exts.contains(&"swift"));
        assert!(exts.contains(&"m"));
        assert!(exts.contains(&"py"));
        assert!(exts.contains(&"go"));
        assert!(exts.contains(&"c"));
        assert!(exts.contains(&"h"));
        assert!(exts.contains(&"cpp"));
        assert!(exts.contains(&"hpp"));
    }

    #[test]
    fn test_detect_language_with_override_map() {
        let overrides = HashMap::from([
            (".vue".to_string(), "typescript".to_string()),
            ("script".to_string(), "python".to_string()),
        ]);

        assert_eq!(
            detect_language_with_map(&PathBuf::from("src/App.vue"), &overrides),
            Some(Language::TypeScript)
        );
        assert_eq!(
            detect_language_with_map(&PathBuf::from("tasks/build.script"), &overrides),
            Some(Language::Python)
        );
    }
}
