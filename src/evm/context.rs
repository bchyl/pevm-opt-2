use crate::storage::KVStore;
use crate::types::{AccessSets, Key, U256};
use ahash::AHashSet;

pub struct ExecutionContext<S: KVStore> {
    pub storage: S,
    pub warm_keys: AHashSet<Key>,
    pub cold_keys: AHashSet<Key>,
    pub access_sets: AccessSets,
    pub gas_used: u64,
    pub stack: Vec<U256>,
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

    pub fn warm_up(&mut self, key: Key) {
        self.warm_keys.insert(key);
    }

    pub fn warm_up_keys(&mut self, keys: &[Key]) {
        for key in keys {
            self.warm_keys.insert(*key);
        }
    }

    pub fn is_warm(&self, key: &Key) -> bool {
        self.warm_keys.contains(key)
    }

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

    pub fn consume_gas(&mut self, amount: u64) -> Result<(), String> {
        self.gas_used = self
            .gas_used
            .checked_add(amount)
            .ok_or_else(|| "Gas overflow".to_string())?;
        self.check_gas()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;

    #[test]
    fn test_context_operations() {
        let mut ctx = ExecutionContext::with_gas_limit(MemoryStore::new(), 1000);
        let key = Key::new([1u8; 20], [1u8; 32]);
        ctx.warm_up(key);
        assert!(ctx.is_warm(&key));
        assert!(ctx.consume_gas(500).is_ok());
        assert!(ctx.consume_gas(600).is_err());
    }
}
