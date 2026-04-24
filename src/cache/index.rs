//! Symbol index cache keyed by symbol name.
//!
//! This cache accelerates interface lookups by keeping an in-memory mapping
//! from `symbol.name` to one or more symbol IDs. It also tracks which symbol
//! IDs came from each file so incremental updates can invalidate only the
//! affected entries.

use crate::core::Symbol;
use std::collections::{BTreeSet, HashMap};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexedSymbol {
    name: String,
    file_path: String,
}

#[derive(Debug, Default)]
struct SymbolIndexState {
    name_to_ids: HashMap<String, BTreeSet<String>>,
    file_to_ids: HashMap<String, BTreeSet<String>>,
    id_to_symbol: HashMap<String, IndexedSymbol>,
}

/// Thread-safe in-memory symbol index cache.
#[derive(Debug, Default)]
pub struct SymbolIndexCache {
    inner: RwLock<SymbolIndexState>,
}

impl SymbolIndexCache {
    /// Create an empty symbol index cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or update a single symbol in the index.
    pub fn insert_symbol(&self, symbol: &Symbol) {
        let mut state = self.write_state();
        Self::remove_symbol(&mut state, &symbol.id);
        Self::insert_symbol_locked(&mut state, symbol);
    }

    /// Insert or update a batch of symbols.
    pub fn insert_symbols(&self, symbols: &[Symbol]) {
        let mut state = self.write_state();
        for symbol in symbols {
            Self::remove_symbol(&mut state, &symbol.id);
            Self::insert_symbol_locked(&mut state, symbol);
        }
    }

    /// Replace all cached symbols for a file and return removed symbol IDs.
    pub fn replace_file_symbols(&self, file_path: &str, symbols: &[Symbol]) -> Vec<String> {
        let mut state = self.write_state();
        let removed = Self::invalidate_file_locked(&mut state, file_path);
        for symbol in symbols {
            if symbol.file_path != file_path {
                continue;
            }
            Self::remove_symbol(&mut state, &symbol.id);
            Self::insert_symbol_locked(&mut state, symbol);
        }
        removed
    }

    /// Return all symbol IDs matching the given symbol name.
    pub fn get(&self, name: &str) -> Vec<String> {
        self.read_state()
            .name_to_ids
            .get(name)
            .map(|ids| ids.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Check whether a symbol name is currently indexed.
    pub fn contains_name(&self, name: &str) -> bool {
        self.read_state().name_to_ids.contains_key(name)
    }

    /// Invalidate every symbol associated with a file and return removed IDs.
    pub fn invalidate_file(&self, file_path: &str) -> Vec<String> {
        let mut state = self.write_state();
        Self::invalidate_file_locked(&mut state, file_path)
    }

    /// Invalidate a set of symbol IDs and return the IDs that were removed.
    pub fn invalidate_symbols(&self, symbol_ids: &[String]) -> Vec<String> {
        let mut state = self.write_state();
        let mut removed = Vec::new();
        for symbol_id in symbol_ids {
            if Self::remove_symbol(&mut state, symbol_id) {
                removed.push(symbol_id.clone());
            }
        }
        removed
    }

    /// Clear the entire index.
    pub fn clear(&self) {
        *self.write_state() = SymbolIndexState::default();
    }

    /// Return the number of indexed symbol names.
    pub fn len_names(&self) -> usize {
        self.read_state().name_to_ids.len()
    }

    /// Return the number of indexed symbols.
    pub fn len_symbols(&self) -> usize {
        self.read_state().id_to_symbol.len()
    }

    /// Return `true` when the index is empty.
    pub fn is_empty(&self) -> bool {
        self.len_symbols() == 0
    }

    fn read_state(&self) -> RwLockReadGuard<'_, SymbolIndexState> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write_state(&self) -> RwLockWriteGuard<'_, SymbolIndexState> {
        self.inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn insert_symbol_locked(state: &mut SymbolIndexState, symbol: &Symbol) {
        state
            .name_to_ids
            .entry(symbol.name.clone())
            .or_default()
            .insert(symbol.id.clone());
        state
            .file_to_ids
            .entry(symbol.file_path.clone())
            .or_default()
            .insert(symbol.id.clone());
        state.id_to_symbol.insert(
            symbol.id.clone(),
            IndexedSymbol {
                name: symbol.name.clone(),
                file_path: symbol.file_path.clone(),
            },
        );
    }

    fn invalidate_file_locked(state: &mut SymbolIndexState, file_path: &str) -> Vec<String> {
        let symbol_ids = state
            .file_to_ids
            .remove(file_path)
            .map(|ids| ids.into_iter().collect::<Vec<_>>())
            .unwrap_or_default();

        for symbol_id in &symbol_ids {
            Self::remove_symbol_from_name_index(state, symbol_id);
        }

        for symbol_id in &symbol_ids {
            state.id_to_symbol.remove(symbol_id);
        }

        symbol_ids
    }

    fn remove_symbol(state: &mut SymbolIndexState, symbol_id: &str) -> bool {
        let Some(indexed) = state.id_to_symbol.remove(symbol_id) else {
            return false;
        };

        if let Some(ids) = state.name_to_ids.get_mut(&indexed.name) {
            ids.remove(symbol_id);
            if ids.is_empty() {
                state.name_to_ids.remove(&indexed.name);
            }
        }

        if let Some(ids) = state.file_to_ids.get_mut(&indexed.file_path) {
            ids.remove(symbol_id);
            if ids.is_empty() {
                state.file_to_ids.remove(&indexed.file_path);
            }
        }

        true
    }

    fn remove_symbol_from_name_index(state: &mut SymbolIndexState, symbol_id: &str) {
        let Some(indexed) = state.id_to_symbol.get(symbol_id) else {
            return;
        };

        if let Some(ids) = state.name_to_ids.get_mut(&indexed.name) {
            ids.remove(symbol_id);
            if ids.is_empty() {
                state.name_to_ids.remove(&indexed.name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Symbol, SymbolKind};

    fn make_symbol(id: &str, name: &str, file_path: &str) -> Symbol {
        let mut symbol = Symbol::new(
            name.to_string(),
            format!("{file_path}::{name}"),
            SymbolKind::Function,
            file_path.to_string(),
            1,
            1,
        );
        symbol.id = id.to_string();
        symbol
    }

    #[test]
    fn test_insert_and_lookup_symbols() {
        let cache = SymbolIndexCache::new();
        let first = make_symbol("s1", "helper", "src/utils.rs");
        let second = make_symbol("s2", "helper", "src/other.rs");

        cache.insert_symbols(&[first, second]);

        assert_eq!(
            cache.get("helper"),
            vec!["s1".to_string(), "s2".to_string()]
        );
        assert!(cache.contains_name("helper"));
        assert_eq!(cache.len_names(), 1);
        assert_eq!(cache.len_symbols(), 2);
    }

    #[test]
    fn test_replace_file_symbols_invalidates_old_entries() {
        let cache = SymbolIndexCache::new();
        let old = make_symbol("s1", "helper", "src/utils.rs");
        let new_symbol = make_symbol("s2", "calculate", "src/utils.rs");

        cache.insert_symbol(&old);
        let removed = cache.replace_file_symbols("src/utils.rs", &[new_symbol]);

        assert_eq!(removed, vec!["s1".to_string()]);
        assert!(cache.get("helper").is_empty());
        assert_eq!(cache.get("calculate"), vec!["s2".to_string()]);
    }

    #[test]
    fn test_replace_file_symbols_ignores_mismatched_files() {
        let cache = SymbolIndexCache::new();
        let old = make_symbol("s1", "helper", "src/utils.rs");
        let wrong_file_symbol = make_symbol("s2", "calculate", "src/other.rs");

        cache.insert_symbol(&old);
        cache.replace_file_symbols("src/utils.rs", &[wrong_file_symbol]);

        assert!(cache.get("calculate").is_empty());
        assert!(!cache.contains_name("helper"));
        assert!(cache.is_empty());
    }

    #[test]
    fn test_invalidate_symbols_updates_indexes() {
        let cache = SymbolIndexCache::new();
        let first = make_symbol("s1", "helper", "src/utils.rs");
        let second = make_symbol("s2", "helper", "src/other.rs");

        cache.insert_symbols(&[first, second]);
        let removed = cache.invalidate_symbols(&["s1".to_string()]);

        assert_eq!(removed, vec!["s1".to_string()]);
        assert_eq!(cache.get("helper"), vec!["s2".to_string()]);
        assert_eq!(cache.len_symbols(), 1);
    }

    #[test]
    fn test_clear_resets_all_state() {
        let cache = SymbolIndexCache::new();
        cache.insert_symbol(&make_symbol("s1", "helper", "src/utils.rs"));

        cache.clear();

        assert!(cache.is_empty());
        assert!(!cache.contains_name("helper"));
    }
}
