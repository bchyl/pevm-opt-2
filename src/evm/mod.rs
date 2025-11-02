pub mod context;
pub mod gas;
pub mod ops;

pub use context::ExecutionContext;
pub use gas::*;
pub use ops::execute_op;

use crate::storage::KVStore;
use crate::types::{Block, ExecutionResult, Transaction};
use crate::{debug, error, info};

pub struct SerialExecutionResult<S: KVStore> {
    pub storage: S,
    pub results: Vec<ExecutionResult>,
    pub total_gas: u64,
}

pub fn execute_transaction<S: KVStore>(
    tx: &Transaction,
    ctx: &mut ExecutionContext<S>,
) -> ExecutionResult {
    debug!("Executing transaction {}", tx.id);

    ctx.warm_up_keys(&tx.metadata.access_list);

    for (idx, op) in tx.metadata.program.iter().enumerate() {
        match execute_op(op, ctx) {
            Ok(()) => {}
            Err(e) => {
                error!("Transaction {} failed at op {}: {}", tx.id, idx, e);
                return ExecutionResult::failure(tx.id, e);
            }
        }
    }

    ExecutionResult::success(
        tx.id,
        ctx.gas_used,
        ctx.access_sets.clone(),
        ctx.warm_keys.clone(),
        ctx.cold_keys.clone(),
    )
}

pub fn execute_serial<S: KVStore>(block: &Block, storage: S) -> SerialExecutionResult<S> {
    info!(
        "Executing block {} serially with {} transactions",
        block.number,
        block.transactions.len()
    );

    let mut ctx = ExecutionContext::new(storage);
    let mut results = Vec::with_capacity(block.transactions.len());
    let mut total_gas = 0;

    for tx in &block.transactions {
        ctx.cold_keys.clear();
        ctx.access_sets = crate::types::AccessSets::new();
        ctx.gas_used = 0;
        ctx.stack.clear();

        let result = execute_transaction(tx, &mut ctx);
        total_gas += result.gas_used;
        results.push(result);
    }

    let final_storage = ctx.storage;

    info!(
        "Serial execution complete: {} transactions, {} gas used",
        results.len(),
        total_gas
    );

    SerialExecutionResult {
        storage: final_storage,
        results,
        total_gas,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;
    use crate::types::{Key, MicroOp, TransactionMetadata, U256};

    #[test]
    fn test_block_execution() {
        let key = Key::new([1u8; 20], [1u8; 32]);
        let tx1 = Transaction {
            id: 1,
            reads: vec![],
            writes: vec![],
            gas_hint: 100000,
            metadata: TransactionMetadata {
                program: vec![MicroOp::SStore(key, U256::from_u64(100))],
                access_list: vec![],
                blob_size: 0,
                nonce: 0,
                from: [0u8; 20],
            },
        };
        let block = Block::new(1, vec![tx1]);
        let result = execute_serial(&block, MemoryStore::new());
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.storage.get(&key), U256::from_u64(100));
    }
}
