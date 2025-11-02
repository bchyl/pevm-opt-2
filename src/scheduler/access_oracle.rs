use crate::types::{AccessSets, ExecutionResult, MicroOp, Transaction};
use ahash::AHashMap;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

pub trait AccessOracle: Send + Sync {
    fn estimate_access_sets(&self, tx: &Transaction) -> AccessSets;
}

pub struct HeuristicOracle {
    miss_rate: f64,
    rng: std::sync::Mutex<StdRng>,
}

impl HeuristicOracle {
    pub fn new() -> Self {
        Self::with_miss_rate(0.05)
    }

    pub fn with_miss_rate(miss_rate: f64) -> Self {
        Self {
            miss_rate,
            rng: std::sync::Mutex::new(StdRng::seed_from_u64(12345)),
        }
    }
}

impl Default for HeuristicOracle {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessOracle for HeuristicOracle {
    fn estimate_access_sets(&self, tx: &Transaction) -> AccessSets {
        let mut sets = AccessSets::new();
        let mut rng = self.rng.lock().unwrap();

        // tx.reads.iter().for_each(|k|sets.add_read(*k));
        // tx.writes.iter().for_each(|k|sets.add_write(*k));
        tx.metadata.access_list.iter().for_each(|k| {
            if rng.gen::<f64>() >= self.miss_rate {
                sets.add_read(*k);
            }
        });

        for op in &tx.metadata.program {
            if rng.gen::<f64>() >= self.miss_rate {
                match op {
                    MicroOp::SLoad(key) => sets.add_read(*key),
                    MicroOp::SStore(key, _) => sets.add_write(*key),
                    _ => {}
                }
            }
        }
        sets
    }
}

pub struct AccessListBuilder {
    oracle: Box<dyn AccessOracle>,
    estimated: AHashMap<u64, AccessSets>,
    exact: AHashMap<u64, AccessSets>,
}

impl AccessListBuilder {
    pub fn new(oracle: Box<dyn AccessOracle>) -> Self {
        Self {
            oracle,
            estimated: AHashMap::new(),
            exact: AHashMap::new(),
        }
    }

    pub fn with_heuristic() -> Self {
        Self::new(Box::new(HeuristicOracle::new()))
    }

    pub fn estimate_before_execution(&mut self, tx: &Transaction) {
        let sets = self.oracle.estimate_access_sets(tx);
        self.estimated.insert(tx.id, sets);
    }

    pub fn record_after_execution(&mut self, result: &ExecutionResult) {
        self.exact.insert(result.tx_id, result.access_sets.clone());
    }

    pub fn get_estimated(&self, tx_id: u64) -> Option<&AccessSets> {
        self.estimated.get(&tx_id)
    }
}
