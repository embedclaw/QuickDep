//! TTL-based cache for expensive query results.
//!
//! Query results are cached in memory for a short period to avoid repeated
//! graph traversals. Expired entries are removed lazily on access.

use std::collections::HashMap;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant};

/// Default TTL for cached query results.
pub const DEFAULT_QUERY_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
struct CacheEntry<T> {
    value: T,
    expires_at: Instant,
}

/// Thread-safe TTL cache for query results.
#[derive(Debug)]
pub struct QueryCache<T> {
    ttl: Duration,
    entries: RwLock<HashMap<String, CacheEntry<T>>>,
}

impl<T> QueryCache<T> {
    /// Create a new query cache with the given TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Return the configured entry TTL.
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Insert a value into the cache.
    pub fn insert(&self, key: impl Into<String>, value: T) {
        self.write_entries().insert(
            key.into(),
            CacheEntry {
                value,
                expires_at: Instant::now() + self.ttl,
            },
        );
    }

    /// Remove a specific cache key.
    pub fn invalidate(&self, key: &str) -> Option<T> {
        self.write_entries().remove(key).map(|entry| entry.value)
    }

    /// Remove cache entries that satisfy the predicate and return removed keys.
    pub fn invalidate_where<F>(&self, predicate: F) -> Vec<String>
    where
        F: Fn(&str) -> bool,
    {
        let mut entries = self.write_entries();
        let mut keys = entries
            .keys()
            .filter(|key| predicate(key))
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();

        for key in &keys {
            entries.remove(key);
        }

        keys
    }

    /// Clear the cache.
    pub fn clear(&self) {
        self.write_entries().clear();
    }

    /// Remove expired entries and return the number removed.
    pub fn purge_expired(&self) -> usize {
        let now = Instant::now();
        let mut entries = self.write_entries();
        let before = entries.len();
        entries.retain(|_, entry| entry.expires_at > now);
        before - entries.len()
    }

    /// Return the number of cached entries, including unaccessed expired entries.
    pub fn len(&self) -> usize {
        self.read_entries().len()
    }

    /// Return `true` when the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.read_entries().is_empty()
    }

    fn read_entries(&self) -> RwLockReadGuard<'_, HashMap<String, CacheEntry<T>>> {
        self.entries
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write_entries(&self) -> RwLockWriteGuard<'_, HashMap<String, CacheEntry<T>>> {
        self.entries
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl<T> Default for QueryCache<T> {
    fn default() -> Self {
        Self::new(DEFAULT_QUERY_TTL)
    }
}

impl<T: Clone> QueryCache<T> {
    /// Get a cached value if it exists and has not expired.
    pub fn get(&self, key: &str) -> Option<T> {
        {
            let entries = self.read_entries();
            match entries.get(key) {
                Some(entry) if entry.expires_at > Instant::now() => {
                    return Some(entry.value.clone());
                }
                Some(_) => {}
                None => return None,
            }
        }

        self.write_entries().remove(key);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_cache_hit() {
        let cache = QueryCache::new(Duration::from_secs(1));
        cache.insert("deps:user", vec!["a".to_string(), "b".to_string()]);

        assert_eq!(
            cache.get("deps:user"),
            Some(vec!["a".to_string(), "b".to_string()])
        );
    }

    #[test]
    fn test_cache_expiry() {
        let cache = QueryCache::new(Duration::from_millis(10));
        cache.insert("deps:user", "value".to_string());

        thread::sleep(Duration::from_millis(25));

        assert_eq!(cache.get("deps:user"), None);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_invalidate_where() {
        let cache = QueryCache::new(Duration::from_secs(60));
        cache.insert("deps:user", 1usize);
        cache.insert("deps:team", 2usize);
        cache.insert("search:helper", 3usize);

        let removed = cache.invalidate_where(|key| key.starts_with("deps:"));

        assert_eq!(
            removed,
            vec!["deps:team".to_string(), "deps:user".to_string()]
        );
        assert_eq!(cache.get("search:helper"), Some(3));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_purge_expired() {
        let cache = QueryCache::new(Duration::from_millis(10));
        cache.insert("expired", 1usize);
        cache.insert("fresh", 2usize);

        thread::sleep(Duration::from_millis(20));
        cache.insert("fresh", 2usize);

        assert_eq!(cache.purge_expired(), 1);
        assert_eq!(cache.get("fresh"), Some(2));
    }
}
