use crate::types::*;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;

/// Block generator for synthetic data
pub struct BlockGenerator {
    pub n_tx: usize,
    pub key_space: usize,
    pub conflict_ratio: f64,
    pub cold_ratio: f64,
    pub seed: u64,
}

impl BlockGenerator {
    pub fn new(
        n_tx: usize,
        key_space: usize,
        conflict_ratio: f64,
        cold_ratio: f64,
        seed: u64,
    ) -> Self {
        Self {
            n_tx,
            key_space,
            conflict_ratio,
            cold_ratio,
            seed,
        }
    }

    /// Generate a synthetic block with controlled conflicts
    /// 
    /// # Algorithm
    /// ```text
    /// For each transaction:
    ///   1. Determine read/write set sizes (1-5 reads, 1-3 writes)
    ///   
    ///   2. Generate keys with controlled conflicts:
    ///      - With probability = conflict_ratio: reuse key from pool
    ///      - Otherwise: generate fresh random key
    ///   
    ///   3. Build program from access sets:
    ///      - SLoad for each read
    ///      - Add/Sub for arithmetic
    ///      - SStore for each write
    ///      - 20% chance: add Keccak operation
    ///      - Padding with NoOps
    ///   
    ///   4. Create transaction with metadata
    /// ```
    /// 
    /// # Example
    /// ```text
    /// conflict_ratio=0.2, n_tx=3
    /// 
    /// tx0: reads={K_pool[5]}, writes={K_new[0]}  <- 20% uses pool key
    /// tx1: reads={K_pool[5]}, writes={K_new[1]}  <- Conflicts with tx0!
    /// tx2: reads={K_new[2]}, writes={K_new[3]}   <- Independent
    /// ```
    pub fn generate(&self) -> Block {
        let mut rng = StdRng::seed_from_u64(self.seed);
        let mut transactions = Vec::with_capacity(self.n_tx);

        // Generate key pool for creating conflicts
        let key_pool: Vec<Key> = (0..self.key_space)
            .map(|i| {
                let addr_val = (i % 256) as u8;
                let slot_val = (i / 256) as u8;
                Key::new([addr_val; 20], [slot_val; 32])
            })
            .collect();

        tracing::info!(
            "Generating block: {} txs, {} key space, {:.1}% conflict ratio, {:.1}% cold ratio, seed={}",
            self.n_tx,
            self.key_space,
            self.conflict_ratio * 100.0,
            self.cold_ratio * 100.0,
            self.seed
        );

        for tx_id in 0..self.n_tx {
            // Determine read/write set sizes
            let read_count = rng.gen_range(1..=5);
            let write_count = rng.gen_range(1..=3);

            let mut reads = Vec::new();
            let mut writes = Vec::new();

            // Generate reads with controlled conflicts
            for _ in 0..read_count {
                let key = if rng.gen::<f64>() < self.conflict_ratio && !key_pool.is_empty() {
                    // Use existing key from pool (creates conflict)
                    key_pool[rng.gen_range(0..key_pool.len())]
                } else {
                    // Generate new unique key
                    Key::new(rng.gen::<[u8; 20]>(), rng.gen::<[u8; 32]>())
                };
                reads.push(key);
            }

            // Generate writes with controlled conflicts
            for _ in 0..write_count {
                let key = if rng.gen::<f64>() < self.conflict_ratio && !key_pool.is_empty() {
                    // Use existing key from pool (creates conflict)
                    key_pool[rng.gen_range(0..key_pool.len())]
                } else {
                    // Generate new unique key
                    Key::new(rng.gen::<[u8; 20]>(), rng.gen::<[u8; 32]>())
                };
                writes.push(key);
            }

            // Generate program from reads/writes
            let mut program = Vec::new();

            // Add reads
            for key in &reads {
                program.push(MicroOp::SLoad(*key));
            }

            // Add some arithmetic operations
            if !reads.is_empty() {
                program.push(MicroOp::Add(U256::from_u64(rng.gen_range(1..100))));
            }

            // Add writes
            for key in &writes {
                let value = U256::from_u64(rng.gen_range(1..1000));
                program.push(MicroOp::SStore(*key, value));
            }

            // Optionally add keccak
            if rng.gen::<f64>() < 0.2 {
                let data: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
                program.push(MicroOp::Keccak(data));
            }

            // Add some noops for padding
            for _ in 0..rng.gen_range(0..3) {
                program.push(MicroOp::NoOp);
            }

            // Create transaction
            let tx = Transaction {
                id: tx_id as u64,
                reads,
                writes,
                gas_hint: 100000,
                metadata: TransactionMetadata {
                    program,
                    access_list: vec![],
                    blob_size: if rng.gen::<f64>() < 0.1 {
                        rng.gen_range(1000..100000)
                    } else {
                        0
                    },
                    nonce: tx_id as u64,
                    from: rng.gen::<[u8; 20]>(),
                },
            };

            transactions.push(tx);
        }

        let block = Block::new(1, transactions);
        tracing::info!("Generated block with {} transactions", block.transactions.len());
        block
    }

    /// Preset: Small block (100 txs, low conflicts)
    pub fn small() -> Self {
        Self::new(100, 1000, 0.1, 0.3, 42)
    }

    /// Preset: Medium block (1000 txs, moderate conflicts)
    pub fn medium() -> Self {
        Self::new(1000, 10000, 0.2, 0.3, 42)
    }

    /// Preset: Large block (5000 txs, high conflicts)
    pub fn large() -> Self {
        Self::new(5000, 50000, 0.3, 0.4, 42)
    }

    /// Preset: No conflicts (for testing max parallelism)
    pub fn no_conflicts(n_tx: usize, seed: u64) -> Self {
        Self::new(n_tx, n_tx * 10, 0.0, 0.5, seed)
    }

    /// Preset: Full conflicts (for testing serial execution)
    pub fn full_conflicts(n_tx: usize, seed: u64) -> Self {
        Self::new(n_tx, 1, 1.0, 0.5, seed)
    }
}

impl Default for BlockGenerator {
    fn default() -> Self {
        Self::medium()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_small_block() {
        let generator = BlockGenerator::small();
        let block = generator.generate();

        assert_eq!(block.transactions.len(), 100);
        
        // All transactions should have valid IDs
        for (i, tx) in block.transactions.iter().enumerate() {
            assert_eq!(tx.id, i as u64);
            assert!(!tx.metadata.program.is_empty());
        }
    }

    #[test]
    fn test_generate_deterministic() {
        let gen1 = BlockGenerator::new(50, 500, 0.2, 0.3, 42);
        let gen2 = BlockGenerator::new(50, 500, 0.2, 0.3, 42);

        let block1 = gen1.generate();
        let block2 = gen2.generate();

        // Same seed should produce same block
        assert_eq!(block1.transactions.len(), block2.transactions.len());
        
        for (tx1, tx2) in block1.transactions.iter().zip(block2.transactions.iter()) {
            assert_eq!(tx1.id, tx2.id);
            assert_eq!(tx1.reads.len(), tx2.reads.len());
            assert_eq!(tx1.writes.len(), tx2.writes.len());
        }
    }

    #[test]
    fn test_no_conflicts_preset() {
        let generator = BlockGenerator::no_conflicts(50, 42);
        let block = generator.generate();

        assert_eq!(block.transactions.len(), 50);
        
        // Verify low conflict potential (different keys)
        let mut all_keys: ahash::AHashSet<crate::types::Key> = ahash::AHashSet::new();
        for tx in &block.transactions {
            for key in &tx.reads {
                all_keys.insert(*key);
            }
            for key in &tx.writes {
                all_keys.insert(*key);
            }
        }
        
        // Should have many unique keys
        assert!(all_keys.len() > 40);
    }

    #[test]
    fn test_full_conflicts_preset() {
        let generator = BlockGenerator::full_conflicts(50, 42);
        let block = generator.generate();

        assert_eq!(block.transactions.len(), 50);
    }

    #[test]
    fn test_preset_scenarios() {
        let small = BlockGenerator::small();
        let medium = BlockGenerator::medium();
        let large = BlockGenerator::large();

        assert_eq!(small.n_tx, 100);
        assert_eq!(medium.n_tx, 1000);
        assert_eq!(large.n_tx, 5000);

        // All should generate valid blocks
        assert_eq!(small.generate().transactions.len(), 100);
        assert_eq!(medium.generate().transactions.len(), 1000);
    }
}

