pub mod context;
pub mod gas;
pub mod ops;

pub use context::ExecutionContext;
pub use gas::*;
pub use ops::execute_op;

use crate::storage::KVStore;
use crate::types::{Block, ExecutionResult, Transaction};

/// Execute a single transaction
pub fn execute_transaction<S: KVStore>(
    tx: &Transaction,
    ctx: &mut ExecutionContext<S>,
) -> ExecutionResult {
    tracing::debug!("Executing transaction {}", tx.id);

    // Warm up keys from EIP-2930 access list
    ctx.warm_up_keys(&tx.metadata.access_list);

    // Execute each micro-op in the program
    for (idx, op) in tx.metadata.program.iter().enumerate() {
        match execute_op(op, ctx) {
            Ok(()) => {}
            Err(e) => {
                tracing::error!(
                    "Transaction {} failed at op {}: {}",
                    tx.id,
                    idx,
                    e
                );
                return ExecutionResult::failure(tx.id, e);
            }
        }
    }

    // Return successful result
    ExecutionResult::success(
        tx.id,
        ctx.gas_used,
        ctx.access_sets.clone(),
        ctx.warm_keys.clone(),
        ctx.cold_keys.clone(),
    )
}

/// Execute block serially (baseline for correctness)
pub fn execute_serial<S: KVStore>(
    block: &Block,
    storage: S,
) -> (S, Vec<ExecutionResult>, u64) {
    tracing::info!("Executing block {} serially with {} transactions", 
        block.number, block.transactions.len());

    let mut ctx = ExecutionContext::new(storage);
    let mut results = Vec::with_capacity(block.transactions.len());
    let mut total_gas = 0;

    for tx in &block.transactions {
        // Reset per-transaction state but keep warm keys
        ctx.cold_keys.clear();
        ctx.access_sets = crate::types::AccessSets::new();
        ctx.gas_used = 0;
        ctx.stack.clear();
        
        // Execute transaction
        let result = execute_transaction(tx, &mut ctx);
        total_gas += result.gas_used;
        results.push(result);

        // Warm keys persist between transactions in the same block (EIP-2929)
    }

    let final_storage = ctx.storage;
    
    tracing::info!(
        "Serial execution complete: {} transactions, {} gas used",
        results.len(),
        total_gas
    );

    (final_storage, results, total_gas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;
    use crate::types::{Key, MicroOp, TransactionMetadata, U256};

    fn create_test_transaction(id: u64, program: Vec<MicroOp>) -> Transaction {
        Transaction {
            id,
            reads: vec![],
            writes: vec![],
            gas_hint: 100000,
            metadata: TransactionMetadata {
                program,
                access_list: vec![],
                blob_size: 0,
                nonce: 0,
                from: [0u8; 20],
            },
        }
    }

    #[test]
    fn test_execute_simple_transaction() {
        let key = Key::new([1u8; 20], [1u8; 32]);
        let value = U256::from_u64(42);

        let program = vec![
            MicroOp::SStore(key, value),
            MicroOp::SLoad(key),
        ];

        let tx = create_test_transaction(1, program);
        let storage = MemoryStore::new();
        let mut ctx = ExecutionContext::new(storage);

        let result = execute_transaction(&tx, &mut ctx);

        assert!(result.success);
        assert!(result.gas_used > 0);
        assert_eq!(ctx.storage.get(&key), value);
        assert_eq!(ctx.stack.len(), 1);
        assert_eq!(ctx.stack[0], value);
    }

    #[test]
    fn test_execute_arithmetic() {
        let program = vec![
            MicroOp::Add(U256::from_u64(10)),  // Stack: [] -> won't work
        ];

        let tx = create_test_transaction(1, program);
        let storage = MemoryStore::new();
        let mut ctx = ExecutionContext::new(storage);

        // This should fail because stack is empty
        let result = execute_transaction(&tx, &mut ctx);
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_execute_with_stack() {
        let program = vec![
            MicroOp::SLoad(Key::new([1u8; 20], [1u8; 32])), // Push 0 to stack
            MicroOp::Add(U256::from_u64(42)),                 // Add 42
        ];

        let tx = create_test_transaction(1, program);
        let storage = MemoryStore::new();
        let mut ctx = ExecutionContext::new(storage);

        let result = execute_transaction(&tx, &mut ctx);

        assert!(result.success);
        assert_eq!(ctx.stack.len(), 1);
        assert_eq!(ctx.stack[0], U256::from_u64(42));
    }

    #[test]
    fn test_execute_serial_block() {
        let key1 = Key::new([1u8; 20], [1u8; 32]);
        let key2 = Key::new([2u8; 20], [2u8; 32]);

        let tx1 = create_test_transaction(
            1,
            vec![
                MicroOp::SStore(key1, U256::from_u64(100)),
            ],
        );

        let tx2 = create_test_transaction(
            2,
            vec![
                MicroOp::SLoad(key1),
                MicroOp::Add(U256::from_u64(50)),
                MicroOp::SStore(key2, U256::from_u64(150)), // Should store result
            ],
        );

        let block = Block::new(1, vec![tx1, tx2]);
        let storage = MemoryStore::new();

        let (final_storage, results, total_gas) = execute_serial(&block, storage);

        assert_eq!(results.len(), 2);
        assert!(results[0].success);
        assert!(results[1].success);
        assert!(total_gas > 0);

        // Verify final state
        assert_eq!(final_storage.get(&key1), U256::from_u64(100));
    }

    #[test]
    fn test_warm_keys_persist() {
        let key = Key::new([1u8; 20], [1u8; 32]);

        // Transaction 1: Cold access
        let tx1 = create_test_transaction(
            1,
            vec![MicroOp::SLoad(key)],
        );

        // Transaction 2: Should be warm
        let tx2 = create_test_transaction(
            2,
            vec![MicroOp::SLoad(key)],
        );

        let block = Block::new(1, vec![tx1, tx2]);
        let storage = MemoryStore::new();

        let (_, results, _) = execute_serial(&block, storage);

        // First transaction should have cold access
        assert!(results[0].cold_keys.contains(&key));

        // Second transaction should not record it as cold
        // (it's already warm from tx1)
        assert!(!results[1].cold_keys.contains(&key));
    }
}

