use super::KVStore;
use crate::types::{Key, U256};
use ahash::AHashMap;
use std::sync::{Arc, Mutex, PoisonError};

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

    fn keys(&self) -> Vec<Key> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .copied()
            .collect()
    }

    fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_store_operations() {
        let mut store = MemoryStore::new();
        let key = Key::new([1u8; 20], [1u8; 32]);

        store.set(key, U256::from_u64(100));
        assert_eq!(store.get(&key), U256::from_u64(100));
        assert_eq!(store.len(), 1);
    }
}
