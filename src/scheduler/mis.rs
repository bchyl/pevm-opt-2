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

    fn find_mis(&self, graph: &ConflictGraph, available: &AHashSet<u64>) -> AHashSet<u64> {
        let mut mis = AHashSet::new();
        let mut remaining = available.clone();

        while !remaining.is_empty() && mis.len() < self.max_wave_size {
            if let Some(&node) = remaining.iter().min_by_key(|&&n| graph.get_neighbors(n).len()) {
                mis.insert(node);
                remaining.remove(&node);
                for neighbor in graph.get_neighbors(node) {
                    remaining.remove(&neighbor);
                }
            } else {
                break;
            }
        }
        mis
    }

    pub fn schedule(&self, block: &Block, access_builder: &AccessListBuilder) -> Vec<Vec<u64>> {
        let access_sets: Vec<(u64, AccessSets)> = block.transactions.iter()
            .filter_map(|tx| access_builder.get_estimated(tx.id).map(|s| (tx.id, s.clone())))
            .collect();

        if access_sets.is_empty() {
            return block.transactions.iter().map(|tx| vec![tx.id]).collect();
        }

        let graph = ConflictGraph::build(&access_sets);
        let mut waves = Vec::new();
        let mut remaining: AHashSet<u64> = block.transactions.iter().map(|tx| tx.id).collect();

        while !remaining.is_empty() {
            let mis = self.find_mis(&graph, &remaining);
            if mis.is_empty() {
                if let Some(&id) = remaining.iter().next() {
                    waves.push(vec![id]);
                    remaining.remove(&id);
                }
            } else {
                let mut wave: Vec<u64> = mis.into_iter().collect();
                wave.sort();
                for id in &wave {
                    remaining.remove(id);
                }
                waves.push(wave);
            }
        }
        waves
    }
}

impl Default for MIScheduler {
    fn default() -> Self {
        Self::new(1000)
    }
}
