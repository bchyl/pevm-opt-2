use crate::storage::KVStore;
use crate::types::{AccessSets, Key, U256};
use ahash::AHashSet;

/// Execution context for a transaction
pub struct ExecutionContext<S: KVStore> {
    /// Storage backend
    pub storage: S,
    
    /// Keys that have been warmed up (accessed before in this block)
    pub warm_keys: AHashSet<Key>,
    
    /// Keys accessed for the first time (cold access)
    pub cold_keys: AHashSet<Key>,
    
    /// Actual access sets (reads and writes)
    pub access_sets: AccessSets,
    
    /// Total gas used
    pub gas_used: u64,
    
    /// Execution stack for micro-ops
    pub stack: Vec<U256>,
    
    /// Gas limit (for future use)
    pub gas_limit: u64,
}

impl<S: KVStore> ExecutionContext<S> {
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            warm_keys: AHashSet::new(),
            cold_keys: AHashSet::new(),
            access_sets: AccessSets::new(),
            gas_used: 0,
            stack: Vec::new(),
            gas_limit: u64::MAX,
        }
    }

    pub fn with_gas_limit(storage: S, gas_limit: u64) -> Self {
        Self {
            storage,
            warm_keys: AHashSet::new(),
            cold_keys: AHashSet::new(),
            access_sets: AccessSets::new(),
            gas_used: 0,
            stack: Vec::new(),
            gas_limit,
        }
    }

    /// Warm up a key (mark as accessed)
    pub fn warm_up(&mut self, key: Key) {
        self.warm_keys.insert(key);
    }

    /// Warm up multiple keys (e.g., from EIP-2930 access list)
    pub fn warm_up_keys(&mut self, keys: &[Key]) {
        for key in keys {
            self.warm_keys.insert(*key);
        }
    }

    /// Check if a key is warm
    pub fn is_warm(&self, key: &Key) -> bool {
        self.warm_keys.contains(key)
    }

    /// Check if gas limit is exceeded
    pub fn check_gas(&self) -> Result<(), String> {
        if self.gas_used > self.gas_limit {
            Err(format!(
                "Out of gas: used {} > limit {}",
                self.gas_used, self.gas_limit
            ))
        } else {
            Ok(())
        }
    }

    /// Consume gas
    pub fn consume_gas(&mut self, amount: u64) -> Result<(), String> {
        self.gas_used = self.gas_used.checked_add(amount)
            .ok_or_else(|| "Gas overflow".to_string())?;
        self.check_gas()
    }

    /// Reset context for new transaction (keeping warm keys if in same block)
    pub fn reset_for_new_tx(&mut self, keep_warm_keys: bool) {
        if !keep_warm_keys {
            self.warm_keys.clear();
        }
        self.cold_keys.clear();
        self.access_sets = AccessSets::new();
        self.gas_used = 0;
        self.stack.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;

    #[test]
    fn test_context_creation() {
        let storage = MemoryStore::new();
        let ctx = ExecutionContext::new(storage);
        
        assert_eq!(ctx.gas_used, 0);
        assert!(ctx.stack.is_empty());
        assert!(ctx.warm_keys.is_empty());
    }

    #[test]
    fn test_warm_up() {
        let storage = MemoryStore::new();
        let mut ctx = ExecutionContext::new(storage);
        
        let key = Key::new([1u8; 20], [1u8; 32]);
        assert!(!ctx.is_warm(&key));
        
        ctx.warm_up(key);
        assert!(ctx.is_warm(&key));
    }

    #[test]
    fn test_gas_limit() {
        let storage = MemoryStore::new();
        let mut ctx = ExecutionContext::with_gas_limit(storage, 1000);
        
        assert!(ctx.consume_gas(500).is_ok());
        assert_eq!(ctx.gas_used, 500);
        
        assert!(ctx.consume_gas(400).is_ok());
        assert_eq!(ctx.gas_used, 900);
        
        // This should fail
        assert!(ctx.consume_gas(200).is_err());
    }

    #[test]
    fn test_reset() {
        let storage = MemoryStore::new();
        let mut ctx = ExecutionContext::new(storage);
        
        let key = Key::new([1u8; 20], [1u8; 32]);
        ctx.warm_up(key);
        ctx.consume_gas(100).unwrap();
        ctx.stack.push(U256::from_u64(42));
        
        ctx.reset_for_new_tx(true);
        
        assert_eq!(ctx.gas_used, 0);
        assert!(ctx.stack.is_empty());
        assert!(ctx.is_warm(&key)); // Still warm
        
        ctx.reset_for_new_tx(false);
        assert!(!ctx.is_warm(&key)); // No longer warm
    }
}


