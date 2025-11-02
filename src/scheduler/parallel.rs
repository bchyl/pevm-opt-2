use super::{AccessListBuilder, MIScheduler};
use crate::evm::{execute_transaction, ExecutionContext};
use crate::storage::{KVStore, MemoryStore};
use crate::types::{Block, ExecutionResult, Key};
use crate::{debug, info};
use ahash::AHashSet;
use rayon::prelude::*;
use std::sync::Arc;

pub struct ParallelExecutionResult {
    pub storage: MemoryStore,
    pub results: Vec<ExecutionResult>,
    pub total_gas: u64,
    pub waves: Vec<Vec<u64>>,
}

pub struct ParallelExecutor {
    scheduler: MIScheduler,
    access_builder: AccessListBuilder,
    storage: MemoryStore,
}

impl ParallelExecutor {
    pub fn new(
        scheduler: MIScheduler,
        access_builder: AccessListBuilder,
        storage: MemoryStore,
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

    pub fn execute_parallel(&mut self, block: &Block) -> ParallelExecutionResult {
        block
            .transactions
            .iter()
            .for_each(|tx| self.access_builder.estimate_before_execution(tx));

        let waves = self.scheduler.schedule(block, &self.access_builder);
        let mut results_map = std::collections::HashMap::new();
        let mut total_gas = 0;
        let mut warm_keys: AHashSet<Key> = AHashSet::new();
        let mut actual_waves = Vec::new();

        use ahash::AHashMap;
        let tx_map: AHashMap<u64, &_> = block.transactions.iter().map(|tx| (tx.id, tx)).collect();

        let mut pending: Vec<u64> = waves.into_iter().flatten().collect();

        while !pending.is_empty() {
            let wave_txs: Vec<_> = pending
                .iter()
                .filter_map(|id| tx_map.get(id).copied())
                .collect();
            pending.clear();

            if wave_txs.is_empty() {
                continue;
            }

            let (committed, conflicting) = if wave_txs.len() == 1 {
                self.execute_single_tx(
                    wave_txs[0],
                    &mut warm_keys,
                    &mut total_gas,
                    &mut results_map,
                );
                (vec![wave_txs[0].id], vec![])
            } else {
                let wave_ids: Vec<u64> = wave_txs.iter().map(|tx| tx.id).collect();
                let conflicting =
                    self.execute_wave(&wave_txs, &mut warm_keys, &mut total_gas, &mut results_map);
                let committed: Vec<u64> = wave_ids
                    .iter()
                    .filter(|id| !conflicting.contains(id))
                    .copied()
                    .collect();
                (committed, conflicting)
            };

            if !committed.is_empty() {
                actual_waves.push(committed);
            }
            if !conflicting.is_empty() {
                info!("Requeueing {} conflicting txs", conflicting.len());
                pending.extend(conflicting);
            }
        }

        let results: Vec<_> = block
            .transactions
            .iter()
            .filter_map(|tx| results_map.remove(&tx.id))
            .collect();

        ParallelExecutionResult {
            storage: self.storage.clone(),
            results,
            total_gas,
            waves: actual_waves,
        }
    }

    fn execute_single_tx(
        &mut self,
        tx: &crate::Transaction,
        warm_keys: &mut AHashSet<Key>,
        total_gas: &mut u64,
        results_map: &mut std::collections::HashMap<u64, ExecutionResult>,
    ) {
        let mut ctx = ExecutionContext::new(self.storage.clone());
        ctx.warm_keys = warm_keys.clone();
        let result = execute_transaction(tx, &mut ctx);

        self.access_builder.record_after_execution(&result);
        if result.success {
            *total_gas += result.gas_used;
            for key in &result.access_sets.writes {
                self.storage.set(*key, ctx.storage.get(key));
            }
            *warm_keys = result.warm_keys.clone();
        }
        results_map.insert(tx.id, result);
    }

    fn execute_wave(
        &mut self,
        wave_txs: &[&crate::Transaction],
        warm_keys: &mut AHashSet<Key>,
        total_gas: &mut u64,
        results_map: &mut std::collections::HashMap<u64, ExecutionResult>,
    ) -> Vec<u64> {
        let storage_arc = Arc::new(self.storage.clone());
        let wave_warm = Arc::new(warm_keys.clone());

        let mut wave_results: Vec<(u64, ExecutionResult, MemoryStore)> = wave_txs
            .par_iter()
            .map(|tx| {
                let mut ctx = ExecutionContext::new((*storage_arc).clone());
                ctx.warm_keys = (*wave_warm).clone();
                let result = execute_transaction(tx, &mut ctx);
                (tx.id, result, ctx.storage)
            })
            .collect();

        wave_results.sort_unstable_by_key(|(tx_id, _, _)| *tx_id);

        let conflicting_txs = self.detect_conflicting_txs(&wave_results);

        if !conflicting_txs.is_empty() {
            debug!(
                "Detected {} conflicting txs in wave of {}, requeueing them",
                conflicting_txs.len(),
                wave_txs.len()
            );

            for (tx_id, result, tx_storage) in wave_results {
                if conflicting_txs.contains(&tx_id) {
                    continue;
                }
                self.access_builder.record_after_execution(&result);
                if result.success {
                    *total_gas += result.gas_used;
                    for key in &result.access_sets.writes {
                        self.storage.set(*key, tx_storage.get(key));
                    }
                    warm_keys.extend(&result.warm_keys);
                }
                results_map.insert(tx_id, result);
            }
            return conflicting_txs;
        }

        for (tx_id, result, tx_storage) in wave_results {
            self.access_builder.record_after_execution(&result);
            if result.success {
                *total_gas += result.gas_used;
                for key in &result.access_sets.writes {
                    self.storage.set(*key, tx_storage.get(key));
                }
                warm_keys.extend(&result.warm_keys);
            }
            results_map.insert(tx_id, result);
        }

        vec![]
    }

    fn detect_conflicting_txs(
        &self,
        wave_results: &[(u64, ExecutionResult, MemoryStore)],
    ) -> Vec<u64> {
        let mut conflicting = AHashSet::new();
        let mut committed_writes: AHashSet<Key> = AHashSet::new();
        let mut committed_reads: AHashSet<Key> = AHashSet::new();

        for (tx_id, result, _) in wave_results {
            let tx_writes: AHashSet<Key> = result.access_sets.writes.iter().copied().collect();
            let tx_reads: AHashSet<Key> = result.access_sets.reads.iter().copied().collect();

            let has_ww = tx_writes.iter().any(|k| committed_writes.contains(k));
            let has_rw = tx_reads.iter().any(|k| committed_writes.contains(k));
            let has_wr = tx_writes.iter().any(|k| committed_reads.contains(k));

            if has_ww || has_rw || has_wr {
                debug!(
                    "TX {} conflicts: WW={}, RW={}, WR={}",
                    tx_id, has_ww, has_rw, has_wr
                );
                conflicting.insert(*tx_id);
            } else {
                committed_writes.extend(&tx_writes);
                committed_reads.extend(&tx_reads);
            }
        }

        let result: Vec<u64> = conflicting.into_iter().collect();
        debug!("Total conflicting txs: {}", result.len());
        result
    }
}
