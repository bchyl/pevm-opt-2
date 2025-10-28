use super::KVStore;
use crate::types::{Key, U256};
use ahash::AHashMap;
use std::sync::{Arc, Mutex, PoisonError};

/// In-memory key-value store implementation
#[derive(Clone)]
pub struct MemoryStore {
    inner: Arc<Mutex<AHashMap<Key, U256>>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(AHashMap::new())),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AHashMap::with_capacity(capacity))),
        }
    }

    /// Create a new store from an existing map
    pub fn from_map(map: AHashMap<Key, U256>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(map)),
        }
    }

    /// Get a snapshot of the current state
    pub fn snapshot(&self) -> Result<AHashMap<Key, U256>, String> {
        self.inner
            .lock()
            .map(|guard| guard.clone())
            .map_err(|e| format!("Mutex lock error: {}", e))
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl KVStore for MemoryStore {
    fn get(&self, key: &Key) -> U256 {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .copied()
            .unwrap_or(U256::ZERO)
    }

    fn set(&mut self, key: Key, value: U256) {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, value);
    }

    fn contains(&self, key: &Key) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(key)
    }

    fn keys(&self) -> Vec<Key> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .copied()
            .collect()
    }

    fn iter(&self) -> Vec<(Key, U256)> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect()
    }

    fn clear(&mut self) {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

// PartialEq removed to avoid potential deadlocks with Arc<Mutex<_>>

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let size = self
            .inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        f.debug_struct("MemoryStore")
            .field("size", &size)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_store_operations() {
        let mut store = MemoryStore::new();
        let key1 = Key::new([1u8; 20], [1u8; 32]);
        let key2 = Key::new([2u8; 20], [2u8; 32]);
        let value1 = U256::from_u64(100);
        let value2 = U256::from_u64(200);

        // Test set and get
        store.set(key1, value1);
        store.set(key2, value2);
        assert_eq!(store.get(&key1), value1);
        assert_eq!(store.get(&key2), value2);

        // Test contains
        assert!(store.contains(&key1));
        assert!(store.contains(&key2));

        // Test len
        assert_eq!(store.len(), 2);

        // Test keys
        let keys = store.keys();
        assert_eq!(keys.len(), 2);

        // Test clear
        store.clear();
        assert_eq!(store.len(), 0);
        assert!(!store.contains(&key1));
    }

    #[test]
    fn test_memory_store_clone() {
        let mut store1 = MemoryStore::new();
        let key = Key::new([1u8; 20], [1u8; 32]);
        let value = U256::from_u64(42);

        store1.set(key, value);

        let store2 = store1.clone();
        assert_eq!(store2.get(&key), value);

        // Both stores share the same underlying data (Arc)
        // Note: PartialEq removed to avoid deadlocks
    }

    #[test]
    fn test_memory_store_snapshot() {
        let mut store = MemoryStore::new();
        let key = Key::new([1u8; 20], [1u8; 32]);
        let value = U256::from_u64(100);

        store.set(key, value);

        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot.get(&key), Some(&value));
    }
}

