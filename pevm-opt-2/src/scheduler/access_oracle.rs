use crate::types::{AccessSets, ExecutionResult, Key, MicroOp, Transaction};
use ahash::AHashMap;

/// Trait for access set estimation
pub trait AccessOracle: Send + Sync {
    fn estimate_access_sets(&self, tx: &Transaction) -> AccessSets;
}

/// Heuristic-based pre-execution estimator
/// Uses transaction metadata and static analysis
pub struct HeuristicOracle {
    /// Learned address patterns (address -> typical slots)
    address_patterns: AHashMap<[u8; 20], Vec<[u8; 32]>>,
}

impl HeuristicOracle {
    pub fn new() -> Self {
        Self {
            address_patterns: AHashMap::new(),
        }
    }

    /// Learn patterns from historical execution results
    pub fn learn_from_history(&mut self, results: &[ExecutionResult]) {
        for result in results {
            for key in &result.access_sets.reads {
                self.address_patterns
                    .entry(key.address)
                    .or_default()
                    .push(key.slot);
            }
            for key in &result.access_sets.writes {
                self.address_patterns
                    .entry(key.address)
                    .or_default()
                    .push(key.slot);
            }
        }

        // Deduplicate slots for each address
        for slots in self.address_patterns.values_mut() {
            slots.sort_unstable();
            slots.dedup();
        }
    }

    /// Try to extract address from data (heuristic)
    fn extract_address(data: &[u8]) -> Option<[u8; 20]> {
        if data.len() >= 20 {
            let mut addr = [0u8; 20];
            addr.copy_from_slice(&data[..20]);
            Some(addr)
        } else {
            None
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
        // ============================================================
        // CORE ALGORITHM: Pre-execution Access Set Estimation
        // ============================================================
        //
        // Goal: Predict which storage keys a transaction will access
        //       before executing it (for conflict detection)
        //
        // Challenge: We don't know the exact execution path yet
        // Solution: Use multiple heuristic strategies
        //
        // Strategies (in order of priority):
        // 1. Declared Access Sets - Use tx.reads/writes if provided
        // 2. EIP-2930 Access List - Parse access_list metadata
        // 3. Static Program Analysis - Scan micro-ops for SLoad/SStore
        //
        // Trade-offs:
        // - Over-estimation (false positives): Safe but reduces parallelism
        // - Under-estimation (false negatives): Detected at runtime
        //
        // Performance: O(m) where m = program size
        // ============================================================
        
        let mut sets = AccessSets::new();

        // Strategy 1: Use declared reads/writes (if available)
        // Most accurate when transactions declare their access patterns
        for key in &tx.reads {
            sets.add_read(*key);
        }
        for key in &tx.writes {
            sets.add_write(*key);
        }

        // Strategy 2: Use EIP-2930 access list metadata
        // Conservatively treat all listed keys as potential reads
        // (actual usage determined during execution)
        for key in &tx.metadata.access_list {
            sets.add_read(*key);
        }

        // Strategy 3: Static program analysis
        // Scan micro-operations for storage accesses
        for op in &tx.metadata.program {
            match op {
                MicroOp::SLoad(key) => {
                    sets.add_read(*key);
                }
                MicroOp::SStore(key, _) => {
                    sets.add_write(*key);
                }
                MicroOp::Keccak(data) => {
                    // Heuristic: if keccak data contains an address,
                    // it might be computing a storage key
                    if let Some(addr) = Self::extract_address(data) {
                        if let Some(patterns) = self.address_patterns.get(&addr) {
                            for slot in patterns {
                                let key = Key::new(addr, *slot);
                                sets.add_read(key); // Conservative: assume read
                            }
                        }
                    }
                }
                _ => {} // ADD, SUB, NOOP don't access storage
            }
        }

        sets
    }
}

/// Post-execution oracle (exact)
/// Returns the actual access sets from execution result
pub struct PostExecutionOracle;

impl PostExecutionOracle {
    pub fn new() -> Self {
        Self
    }

    pub fn exact_access_sets(&self, result: &ExecutionResult) -> AccessSets {
        result.access_sets.clone()
    }
}

impl Default for PostExecutionOracle {
    fn default() -> Self {
        Self::new()
    }
}

/// Access list builder
/// Manages both pre-execution estimates and post-execution exact access sets
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

    /// Estimate access sets before execution
    pub fn estimate_before_execution(&mut self, tx: &Transaction) {
        let sets = self.oracle.estimate_access_sets(tx);
        self.estimated.insert(tx.id, sets);
    }

    /// Record actual access sets after execution
    pub fn record_after_execution(&mut self, result: &ExecutionResult) {
        self.exact.insert(result.tx_id, result.access_sets.clone());
    }

    /// Get estimated access sets
    pub fn get_estimated(&self, tx_id: u64) -> Option<&AccessSets> {
        self.estimated.get(&tx_id)
    }

    /// Get exact access sets
    pub fn get_exact(&self, tx_id: u64) -> Option<&AccessSets> {
        self.exact.get(&tx_id)
    }

    /// Calculate precision and recall
    /// Precision = TP / (TP + FP) - how many estimated accesses were correct
    /// Recall = TP / (TP + FN) - how many actual accesses were predicted
    pub fn calculate_precision_recall(&self) -> (f64, f64, usize, usize) {
        let mut total_precision = 0.0;
        let mut total_recall = 0.0;
        let mut total_fp = 0;
        let mut total_fn = 0;
        let mut count = 0;

        for (tx_id, exact) in &self.exact {
            if let Some(estimated) = self.estimated.get(tx_id) {
                // True positives: correctly estimated accesses
                let tp_reads = estimated.reads.intersection(&exact.reads).count();
                let tp_writes = estimated.writes.intersection(&exact.writes).count();
                let tp = tp_reads + tp_writes;

                // False positives: estimated but not actually accessed
                let fp_reads = estimated.reads.difference(&exact.reads).count();
                let fp_writes = estimated.writes.difference(&exact.writes).count();
                let fp = fp_reads + fp_writes;

                // False negatives: actually accessed but not estimated
                let fn_reads = exact.reads.difference(&estimated.reads).count();
                let fn_writes = exact.writes.difference(&estimated.writes).count();
                let fn_count = fn_reads + fn_writes;

                // Calculate precision
                let precision = if tp + fp > 0 {
                    tp as f64 / (tp + fp) as f64
                } else {
                    1.0 // No estimates made, treat as perfect
                };

                // Calculate recall
                let recall = if tp + fn_count > 0 {
                    tp as f64 / (tp + fn_count) as f64
                } else {
                    1.0 // No actual accesses, treat as perfect
                };

                total_precision += precision;
                total_recall += recall;
                total_fp += fp;
                total_fn += fn_count;
                count += 1;
            }
        }

        let avg_precision = if count > 0 {
            total_precision / count as f64
        } else {
            1.0
        };

        let avg_recall = if count > 0 {
            total_recall / count as f64
        } else {
            1.0
        };

        (avg_precision, avg_recall, total_fp, total_fn)
    }

    /// Clear all data
    pub fn clear(&mut self) {
        self.estimated.clear();
        self.exact.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{TransactionMetadata, U256};
    use ahash::AHashSet;

    fn create_test_tx(id: u64, reads: Vec<Key>, writes: Vec<Key>, program: Vec<MicroOp>) -> Transaction {
        Transaction {
            id,
            reads,
            writes,
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
    fn test_heuristic_oracle_declared_accesses() {
        let oracle = HeuristicOracle::new();
        let key1 = Key::new([1u8; 20], [1u8; 32]);
        let key2 = Key::new([2u8; 20], [2u8; 32]);

        let tx = create_test_tx(
            1,
            vec![key1],
            vec![key2],
            vec![],
        );

        let sets = oracle.estimate_access_sets(&tx);

        assert!(sets.reads.contains(&key1));
        assert!(sets.writes.contains(&key2));
    }

    #[test]
    fn test_heuristic_oracle_program_analysis() {
        let oracle = HeuristicOracle::new();
        let key1 = Key::new([1u8; 20], [1u8; 32]);
        let key2 = Key::new([2u8; 20], [2u8; 32]);

        let tx = create_test_tx(
            1,
            vec![],
            vec![],
            vec![
                MicroOp::SLoad(key1),
                MicroOp::SStore(key2, U256::ZERO),
            ],
        );

        let sets = oracle.estimate_access_sets(&tx);

        assert!(sets.reads.contains(&key1));
        assert!(sets.writes.contains(&key2));
    }

    #[test]
    fn test_access_list_builder_precision_recall() {
        let mut builder = AccessListBuilder::with_heuristic();

        let key1 = Key::new([1u8; 20], [1u8; 32]);
        let key2 = Key::new([2u8; 20], [2u8; 32]);
        let key3 = Key::new([3u8; 20], [3u8; 32]);

        // Create transaction with estimated accesses
        let tx = create_test_tx(
            1,
            vec![key1, key2],  // Estimate: read key1, key2
            vec![],
            vec![],
        );

        builder.estimate_before_execution(&tx);

        // Create execution result with actual accesses
        let mut actual_sets = AccessSets::new();
        actual_sets.add_read(key1);  // TP: correctly estimated
        actual_sets.add_read(key3);  // FN: not estimated but accessed

        let result = ExecutionResult::success(
            1,
            1000,
            actual_sets,
            AHashSet::new(),
            AHashSet::new(),
        );

        builder.record_after_execution(&result);

        let (precision, recall, fp, fn_count) = builder.calculate_precision_recall();

        // Precision: 1/(1+1) = 0.5 (1 correct out of 2 estimated)
        // Recall: 1/(1+1) = 0.5 (1 estimated out of 2 actual)
        assert!((precision - 0.5).abs() < 0.01);
        assert!((recall - 0.5).abs() < 0.01);
        assert_eq!(fp, 1); // key2 was estimated but not accessed
        assert_eq!(fn_count, 1); // key3 was accessed but not estimated
    }
}

