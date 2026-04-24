//! In-memory graph structure

use std::collections::HashMap;

/// In-memory dependency graph
pub struct DependencyGraph {
    /// Symbol ID to symbol name
    symbols: HashMap<String, String>,

    /// Dependencies: from_id -> [(to_id, kind, line)]
    outgoing: HashMap<String, Vec<(String, String, u32)>>,

    /// Reverse dependencies: to_id -> [(from_id, kind, line)]
    incoming: HashMap<String, Vec<(String, String, u32)>>,
}

impl DependencyGraph {
    /// Create an empty graph
    pub fn new() -> Self {
        Self {
            symbols: HashMap::new(),
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
        }
    }

    /// Add a symbol
    pub fn add_symbol(&mut self, id: &str, name: &str) {
        self.symbols.insert(id.to_string(), name.to_string());
    }

    /// Add a dependency
    pub fn add_dependency(&mut self, from: &str, to: &str, kind: &str, line: u32) {
        self.outgoing.entry(from.to_string()).or_default().push((
            to.to_string(),
            kind.to_string(),
            line,
        ));

        self.incoming.entry(to.to_string()).or_default().push((
            from.to_string(),
            kind.to_string(),
            line,
        ));
    }

    /// Get outgoing dependencies (what this symbol depends on)
    pub fn get_dependencies(&self, id: &str) -> Option<&Vec<(String, String, u32)>> {
        self.outgoing.get(id)
    }

    /// Get incoming dependencies (what depends on this symbol)
    pub fn get_dependents(&self, id: &str) -> Option<&Vec<(String, String, u32)>> {
        self.incoming.get(id)
    }

    /// Get all symbol IDs
    pub fn symbol_ids(&self) -> impl Iterator<Item = &String> {
        self.symbols.keys()
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}
