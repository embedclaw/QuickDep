//! Symbol definitions

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Symbol kind (interface type)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    EnumVariant,
    Interface,
    Trait,
    TypeAlias,
    Module,
    Constant,
    Variable,
    Property,
    Macro,
}

impl SymbolKind {
    /// Convert to string for storage
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Class => "class",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::EnumVariant => "enum_variant",
            SymbolKind::Interface => "interface",
            SymbolKind::Trait => "trait",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Module => "module",
            SymbolKind::Constant => "constant",
            SymbolKind::Variable => "variable",
            SymbolKind::Property => "property",
            SymbolKind::Macro => "macro",
        }
    }
}

impl FromStr for SymbolKind {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "function" => Ok(SymbolKind::Function),
            "method" => Ok(SymbolKind::Method),
            "class" => Ok(SymbolKind::Class),
            "struct" => Ok(SymbolKind::Struct),
            "enum" => Ok(SymbolKind::Enum),
            "enum_variant" => Ok(SymbolKind::EnumVariant),
            "interface" => Ok(SymbolKind::Interface),
            "trait" => Ok(SymbolKind::Trait),
            "type_alias" => Ok(SymbolKind::TypeAlias),
            "module" => Ok(SymbolKind::Module),
            "constant" => Ok(SymbolKind::Constant),
            "variable" => Ok(SymbolKind::Variable),
            "property" => Ok(SymbolKind::Property),
            "macro" => Ok(SymbolKind::Macro),
            _ => Err("unknown symbol kind"),
        }
    }
}

/// Symbol visibility
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
    Protected,
}

impl Visibility {
    /// Convert to string for storage
    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Private => "private",
            Visibility::Protected => "protected",
        }
    }
}

impl FromStr for Visibility {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "public" => Ok(Visibility::Public),
            "private" => Ok(Visibility::Private),
            "protected" => Ok(Visibility::Protected),
            _ => Err("unknown visibility"),
        }
    }
}

/// Symbol source (origin)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolSource {
    Local,
    External,
    Builtin,
}

impl SymbolSource {
    /// Convert to string for storage
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolSource::Local => "local",
            SymbolSource::External => "external",
            SymbolSource::Builtin => "builtin",
        }
    }
}

impl FromStr for SymbolSource {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "local" => Ok(SymbolSource::Local),
            "external" => Ok(SymbolSource::External),
            "builtin" => Ok(SymbolSource::Builtin),
            _ => Err("unknown symbol source"),
        }
    }
}

/// A symbol (interface) in the codebase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// Unique identifier (UUID)
    pub id: String,

    /// Symbol name
    pub name: String,

    /// Fully qualified name (e.g., "src/utils.rs::helper")
    pub qualified_name: String,

    /// Symbol kind
    pub kind: SymbolKind,

    /// File path (relative to project root)
    pub file_path: String,

    /// Line number (1-based)
    pub line: u32,

    /// Column number (1-based)
    pub column: u32,

    /// Visibility
    pub visibility: Visibility,

    /// Function/method signature (optional)
    pub signature: Option<String>,

    /// Source (local/external/builtin)
    pub source: SymbolSource,
}

impl Symbol {
    /// Create a new symbol
    pub fn new(
        name: String,
        qualified_name: String,
        kind: SymbolKind,
        file_path: String,
        line: u32,
        column: u32,
    ) -> Self {
        Self {
            id: generate_symbol_id(&qualified_name),
            name,
            qualified_name,
            kind,
            file_path,
            line,
            column,
            visibility: Visibility::Private,
            signature: None,
            source: SymbolSource::Local,
        }
    }

    /// Set visibility
    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    /// Set signature
    pub fn with_signature(mut self, signature: String) -> Self {
        self.signature = Some(signature);
        self
    }

    /// Set source
    pub fn with_source(mut self, source: SymbolSource) -> Self {
        self.source = source;
        self
    }
}

/// Summary of a symbol for search results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceSummary {
    pub id: String,
    pub name: String,
    pub file: String,
    pub line: u32,
    pub kind: SymbolKind,
}

fn generate_symbol_id(qualified_name: &str) -> String {
    blake3::hash(qualified_name.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_ids_are_stable_for_same_qualified_name() {
        let first = Symbol::new(
            "helper".into(),
            "src/lib.rs::helper".into(),
            SymbolKind::Function,
            "src/lib.rs".into(),
            1,
            1,
        );
        let second = Symbol::new(
            "helper".into(),
            "src/lib.rs::helper".into(),
            SymbolKind::Function,
            "src/lib.rs".into(),
            20,
            8,
        );

        assert_eq!(first.id, second.id);
    }

    #[test]
    fn test_symbol_ids_change_with_qualified_name() {
        let first = Symbol::new(
            "helper".into(),
            "src/lib.rs::helper".into(),
            SymbolKind::Function,
            "src/lib.rs".into(),
            1,
            1,
        );
        let second = Symbol::new(
            "helper".into(),
            "src/utils.rs::helper".into(),
            SymbolKind::Function,
            "src/utils.rs".into(),
            1,
            1,
        );

        assert_ne!(first.id, second.id);
    }
}
