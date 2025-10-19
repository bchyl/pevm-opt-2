use crate::types::{Key, U256};

pub mod memory;
pub use memory::MemoryStore;

#[cfg(feature = "db")]
pub mod rocksdb_store;
#[cfg(feature = "db")]
pub mod sled_store;

/// Key-value store trait for EVM storage
pub trait KVStore: Clone + Send + Sync {
    /// Get value for a key (returns U256::ZERO if not found)
    fn get(&self, key: &Key) -> U256;
    
    /// Set value for a key
    fn set(&mut self, key: Key, value: U256);
    
    /// Check if key exists
    fn contains(&self, key: &Key) -> bool;
    
    /// Get all keys
    fn keys(&self) -> Vec<Key>;
    
    /// Get all key-value pairs
    fn iter(&self) -> Vec<(Key, U256)>;
    
    /// Clear all data
    fn clear(&mut self);
    
    /// Get storage size
    fn len(&self) -> usize;
    
    /// Check if empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Key;

    #[test]
    fn test_kv_store_basic_ops() {
        let mut store = MemoryStore::new();
        let key = Key::new([1u8; 20], [1u8; 32]);
        let value = U256::from_u64(100);

        assert_eq!(store.get(&key), U256::ZERO);
        
        store.set(key, value);
        assert_eq!(store.get(&key), value);
        assert!(store.contains(&key));
        
        assert_eq!(store.len(), 1);
    }
}

