use crate::types::{Key, U256};

pub mod memory;
pub use memory::MemoryStore;

pub trait KVStore: Clone + Send + Sync {
    fn get(&self, key: &Key) -> U256;
    fn set(&mut self, key: Key, value: U256);
    fn keys(&self) -> Vec<Key>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
