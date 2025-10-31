use super::{AccessListBuilder, MIScheduler};
use crate::evm::{execute_transaction, ExecutionContext};
use crate::storage::KVStore;
use crate::types::{Block, ExecutionResult};
use rayon::prelude::*;

/// Parallel executor using Rayon
pub struct ParallelExecutor<S: KVStore> {
    scheduler: MIScheduler,
    access_builder: AccessListBuilder,
    storage: S,
}

impl<S: KVStore> ParallelExecutor<S> {
    pub fn new(
        scheduler: MIScheduler,
        access_builder: AccessListBuilder,
        storage: S,
    ) -> Self {
        Self {
            scheduler,
            access_builder,
            storage,
        }
    }

    pub fn access_builder(&self) -> &AccessListBuilder {
        &self.access_builder
    }

    pub fn execute_parallel(
        &mut self,
        block: &Block,
    ) -> (S, Vec<ExecutionResult>, u64, Vec<Vec<u64>>) {
        for tx in &block.transactions {
            self.access_builder.estimate_before_execution(tx);
        }

        let waves = self.scheduler.schedule(block, &self.access_builder);
        let mut all_results = Vec::with_capacity(block.transactions.len());
        let mut total_gas = 0;
        
        use ahash::AHashMap;
        let tx_map: AHashMap<u64, &_> = block.transactions.iter().map(|tx| (tx.id, tx)).collect();

        for wave_ids in &waves {
            let wave_txs: Vec<&_> = wave_ids.iter().filter_map(|id| tx_map.get(id).copied()).collect();

            let results: Vec<ExecutionResult> = wave_txs.par_iter().map(|tx| {
                let mut ctx = ExecutionContext::new(self.storage.clone());
                execute_transaction(tx, &mut ctx)
            }).collect();

            for r in &results {
                self.access_builder.record_after_execution(r);
                if r.success { total_gas += r.gas_used; }
            }
            all_results.extend(results);
        }

        (self.storage.clone(), all_results, total_gas, waves)
    }
}

