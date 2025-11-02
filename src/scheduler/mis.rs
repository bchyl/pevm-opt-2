use super::{AccessListBuilder, ConflictGraph};
use crate::types::{AccessSets, Block};
use ahash::AHashSet;

pub struct MIScheduler {
    max_wave_size: usize,
}

impl MIScheduler {
    pub fn new(max_wave_size: usize) -> Self {
        Self { max_wave_size }
    }

    pub fn schedule(&self, block: &Block, access_builder: &AccessListBuilder) -> Vec<Vec<u64>> {
        let access_sets: Vec<(u64, AccessSets)> = block
            .transactions
            .iter()
            .filter_map(|tx| {
                access_builder
                    .get_estimated(tx.id)
                    .map(|s| (tx.id, s.clone()))
            })
            .collect();

        if access_sets.is_empty() {
            return block.transactions.iter().map(|tx| vec![tx.id]).collect();
        }

        let graph = ConflictGraph::build(&access_sets);
        let mut waves = Vec::new();
        let mut processed: AHashSet<u64> = AHashSet::new();

        for tx in &block.transactions {
            if processed.contains(&tx.id) {
                continue;
            }

            let mut wave = vec![tx.id];
            processed.insert(tx.id);

            for next_tx in &block.transactions {
                if processed.contains(&next_tx.id) || next_tx.id <= tx.id {
                    continue;
                }
                if wave.len() >= self.max_wave_size {
                    break;
                }

                let has_conflict = wave
                    .iter()
                    .any(|&w_id| graph.has_conflict(w_id, next_tx.id));
                if !has_conflict {
                    wave.push(next_tx.id);
                    processed.insert(next_tx.id);
                }
            }
            waves.push(wave);
        }
        waves
    }
}
