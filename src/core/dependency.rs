//! Dependency relationship definitions

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Dependency kind (relationship type)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DependencyKind {
    /// Function/method call
    Call,
    /// Class inheritance
    Inherit,
    /// Interface implementation
    Implement,
    /// Type usage
    TypeUse,
    /// Import relationship
    Import,
}

impl DependencyKind {
    /// Convert to string for storage
    pub fn as_str(&self) -> &'static str {
        match self {
            DependencyKind::Call => "call",
            DependencyKind::Inherit => "inherit",
            DependencyKind::Implement => "implement",
            DependencyKind::TypeUse => "type_use",
            DependencyKind::Import => "import",
        }
    }
}

impl FromStr for DependencyKind {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "call" => Ok(DependencyKind::Call),
            "inherit" => Ok(DependencyKind::Inherit),
            "implement" => Ok(DependencyKind::Implement),
            "type_use" => Ok(DependencyKind::TypeUse),
            "import" => Ok(DependencyKind::Import),
            _ => Err("unknown dependency kind"),
        }
    }
}

/// A dependency relationship between two symbols
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    /// Unique identifier
    pub id: String,

    /// Source symbol ID (the symbol that depends)
    pub from_symbol: String,

    /// Target symbol ID (the symbol being depended on)
    pub to_symbol: String,

    /// File where the dependency occurs
    pub from_file: String,

    /// Line number where the dependency occurs
    pub from_line: u32,

    /// Dependency kind
    pub kind: DependencyKind,
}

impl Dependency {
    /// Create a new dependency
    pub fn new(
        from_symbol: String,
        to_symbol: String,
        from_file: String,
        from_line: u32,
        kind: DependencyKind,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            from_symbol,
            to_symbol,
            from_file,
            from_line,
            kind,
        }
    }
}

/// Symbol diff for incremental updates
#[derive(Debug, Clone, Default)]
pub struct SymbolDiff {
    /// Added symbols
    pub added: Vec<String>,

    /// Removed symbol IDs
    pub removed: Vec<String>,

    /// Modified symbols
    pub modified: Vec<String>,
}
