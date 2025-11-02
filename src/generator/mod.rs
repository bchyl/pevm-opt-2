use crate::types::*;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

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

    fn generate_program(&self, reads: &[Key], writes: &[Key], rng: &mut StdRng) -> Vec<MicroOp> {
        let mut program = Vec::new();

        for key in reads {
            program.push(MicroOp::SLoad(*key));
        }
        if !reads.is_empty() {
            program.push(MicroOp::Add(U256::from_u64(rng.gen_range(1..100))));
        }
        for key in writes {
            let value = U256::from_u64(rng.gen_range(1..1000));
            program.push(MicroOp::SStore(*key, value));
        }
        if rng.gen::<f64>() < 0.2 {
            let data: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
            program.push(MicroOp::Keccak(data));
        }
        for _ in 0..rng.gen_range(0..3) {
            program.push(MicroOp::NoOp);
        }

        program
    }

    pub fn generate(&self) -> Block {
        let mut rng = StdRng::seed_from_u64(self.seed);
        let mut transactions = Vec::with_capacity(self.n_tx);

        // Generate key pool for creating conflicts
        let key_pool: Vec<Key> = (0..self.key_space)
            .map(|i| {
                let addr_val = (i % 65536) as u16;
                let slot_val = (i / 65536) as u16;

                let mut addr = [0u8; 20];
                let mut slot = [0u8; 32];

                addr[0] = (addr_val & 0xFF) as u8;
                addr[1] = (addr_val >> 8) as u8;

                slot[0] = (slot_val & 0xFF) as u8;
                slot[1] = (slot_val >> 8) as u8;

                Key::new(addr, slot)
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
            let program = self.generate_program(&reads, &writes, &mut rng);

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
        tracing::info!(
            "Generated block with {} transactions",
            block.transactions.len()
        );
        block
    }

    pub fn small() -> Self {
        Self::new(100, 1000, 0.1, 0.3, 42)
    }

    pub fn medium() -> Self {
        Self::new(1000, 10000, 0.2, 0.3, 42)
    }

    pub fn large() -> Self {
        Self::new(5000, 50000, 0.3, 0.4, 42)
    }

    pub fn no_conflicts(n_tx: usize, seed: u64) -> Self {
        Self::new(n_tx, n_tx * 10, 0.0, 0.5, seed)
    }

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
    fn test_generator() {
        let block = BlockGenerator::small().generate();
        assert_eq!(block.transactions.len(), 100);
        let block2 = BlockGenerator::new(50, 500, 0.2, 0.3, 42).generate();
        assert_eq!(block2.transactions.len(), 50);
    }
}
