use super::{AccessListBuilder, ConflictGraph};
use crate::types::{AccessSets, Block};
use ahash::AHashSet;

/// Maximal Independent Set scheduler
/// Uses greedy algorithm to find independent sets for parallel execution
pub struct MIScheduler {
    /// Maximum number of transactions per wave
    max_wave_size: usize,
}

impl MIScheduler {
    pub fn new(max_wave_size: usize) -> Self {
        Self { max_wave_size }
    }

    /// Find a maximal independent set using greedy algorithm
    /// Strategy: Select node with minimum degree (fewest conflicts)
    /// 
    /// # Example
    /// ```
    /// // Given transactions: [tx1, tx2, tx3]
    /// // Conflicts: tx1 ↔ tx2 (both access key K)
    /// //
    /// // Graph:  tx1 --- tx2
    /// //         tx3 (isolated)
    /// //
    /// // Algorithm:
    /// // 1. Degrees: tx1=1, tx2=1, tx3=0
    /// // 2. Select tx3 (min degree=0) → MIS={tx3}
    /// // 3. Remaining: tx1, tx2
    /// // 4. Select tx1 (degree=1) → MIS={tx3, tx1}
    /// // 5. Remove tx2 (neighbor) → Done
    /// // Result: MIS={tx1, tx3} can execute in parallel
    /// ```
    pub fn find_mis(
        &self,
        graph: &ConflictGraph,
        available: &AHashSet<u64>,
    ) -> AHashSet<u64> {
        // ============================================================
        // CORE ALGORITHM: Greedy Maximal Independent Set (MIS)
        // ============================================================
        //
        // Goal: Find a large set of non-conflicting transactions
        //       that can execute in parallel
        //
        // Strategy: Greedy minimum-degree heuristic
        // - Select nodes with fewest conflicts first
        // - This preserves more options for future selections
        //
        // Algorithm:
        // 1. Start with all available transactions
        // 2. While transactions remain:
        //    a) Select transaction with minimum degree (fewest conflicts)
        //    b) Add it to the independent set
        //    c) Remove it and all its neighbors (conflicting txs)
        // 3. Repeat until no transactions remain or wave is full
        //
        // Complexity: O(n * d) where d = avg degree
        // Approximation: Within 1.5x of optimal MIS (NP-complete)
        // ============================================================
        
        let mut mis = AHashSet::new();
        let mut remaining = available.clone();

        while !remaining.is_empty() && mis.len() < self.max_wave_size {
            // Step 1: Select node with minimum degree (fewest conflicts)
            // Intuition: Low-degree nodes have few conflicts, so removing
            //            them leaves more options for future selections
            let best_node = remaining
                .iter()
                .min_by_key(|&&node| {
                    // Count how many remaining nodes conflict with this one
                    graph.get_neighbors(node).len()
                })
                .copied();

            if let Some(node) = best_node {
                // Step 2: Add selected node to MIS
                mis.insert(node);
                remaining.remove(&node);

                // Step 3: Remove all neighbors (conflicting transactions)
                // These cannot be in the same wave as the selected node
                for neighbor in graph.get_neighbors(node) {
                    remaining.remove(&neighbor);
                }
                
                // Continue building the MIS with remaining nodes
            } else {
                break; // No more nodes available
            }
        }

        mis
        // Returns: Set of transaction IDs that can execute in parallel
        //          (guaranteed to have no conflicts between them)
    }

    /// Schedule transactions into parallel waves
    pub fn schedule(
        &self,
        block: &Block,
        access_builder: &AccessListBuilder,
    ) -> Vec<Vec<u64>> {
        tracing::info!("Scheduling {} transactions", block.transactions.len());

        // Collect all estimated access sets
        let access_sets: Vec<(u64, AccessSets)> = block
            .transactions
            .iter()
            .filter_map(|tx| {
                access_builder
                    .get_estimated(tx.id)
                    .map(|sets| (tx.id, sets.clone()))
            })
            .collect();

        if access_sets.is_empty() {
            tracing::warn!("No access sets available for scheduling");
            // Fallback: serialize all transactions
            return block
                .transactions
                .iter()
                .map(|tx| vec![tx.id])
                .collect();
        }

        // Build conflict graph
        let graph = ConflictGraph::build(&access_sets);
        
        tracing::info!(
            "Built conflict graph: {} nodes, {} edges, {:.2}% conflict rate",
            graph.node_count(),
            graph.edge_count(),
            graph.conflict_rate() * 100.0
        );

        // Use MIS algorithm to partition into waves
        let mut waves = Vec::new();
        let mut remaining: AHashSet<u64> =
            block.transactions.iter().map(|tx| tx.id).collect();

        let mut wave_idx = 0;
        while !remaining.is_empty() {
            let mis = self.find_mis(&graph, &remaining);

            if mis.is_empty() {
                // Fallback: schedule at least one transaction
                if let Some(&tx_id) = remaining.iter().next() {
                    tracing::debug!("Wave {}: fallback to single transaction {}", wave_idx, tx_id);
                    waves.push(vec![tx_id]);
                    remaining.remove(&tx_id);
                }
            } else {
                let mut wave: Vec<u64> = mis.into_iter().collect();
                wave.sort(); // Deterministic ordering

                tracing::debug!("Wave {}: {} transactions", wave_idx, wave.len());

                for tx_id in &wave {
                    remaining.remove(tx_id);
                }

                waves.push(wave);
            }

            wave_idx += 1;
        }

        tracing::info!(
            "Scheduled into {} waves, avg size: {:.1}",
            waves.len(),
            block.transactions.len() as f64 / waves.len() as f64
        );

        waves
    }
}

impl Default for MIScheduler {
    fn default() -> Self {
        Self::new(1000) // Default max wave size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Key, Transaction, TransactionMetadata};

    fn create_test_tx(id: u64, reads: Vec<Key>, writes: Vec<Key>) -> Transaction {
        Transaction {
            id,
            reads,
            writes,
            gas_hint: 100000,
            metadata: TransactionMetadata {
                program: vec![],
                access_list: vec![],
                blob_size: 0,
                nonce: 0,
                from: [0u8; 20],
            },
        }
    }

    #[test]
    fn test_no_conflicts_single_wave() {
        // Three transactions with different keys
        let tx1 = create_test_tx(
            1,
            vec![],
            vec![Key::new([1u8; 20], [1u8; 32])],
        );
        let tx2 = create_test_tx(
            2,
            vec![],
            vec![Key::new([2u8; 20], [2u8; 32])],
        );
        let tx3 = create_test_tx(
            3,
            vec![],
            vec![Key::new([3u8; 20], [3u8; 32])],
        );

        let block = Block::new(1, vec![tx1, tx2, tx3]);

        let mut access_builder = AccessListBuilder::with_heuristic();
        for tx in &block.transactions {
            access_builder.estimate_before_execution(tx);
        }

        let scheduler = MIScheduler::new(1000);
        let waves = scheduler.schedule(&block, &access_builder);

        // All transactions should be in one wave (no conflicts)
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].len(), 3);
    }

    #[test]
    fn test_full_conflicts_serial() {
        // Three transactions all accessing the same key
        let key = Key::new([1u8; 20], [1u8; 32]);

        let tx1 = create_test_tx(1, vec![], vec![key]);
        let tx2 = create_test_tx(2, vec![], vec![key]);
        let tx3 = create_test_tx(3, vec![], vec![key]);

        let block = Block::new(1, vec![tx1, tx2, tx3]);

        let mut access_builder = AccessListBuilder::with_heuristic();
        for tx in &block.transactions {
            access_builder.estimate_before_execution(tx);
        }

        let scheduler = MIScheduler::new(1000);
        let waves = scheduler.schedule(&block, &access_builder);

        // All transactions should be serialized (full conflicts)
        assert_eq!(waves.len(), 3);
        for wave in &waves {
            assert_eq!(wave.len(), 1);
        }
    }

    #[test]
    fn test_partial_conflicts() {
        let key1 = Key::new([1u8; 20], [1u8; 32]);
        let key2 = Key::new([2u8; 20], [2u8; 32]);

        // tx1 and tx2 conflict, tx3 is independent
        let tx1 = create_test_tx(1, vec![key1], vec![]);
        let tx2 = create_test_tx(2, vec![], vec![key1]); // Conflicts with tx1
        let tx3 = create_test_tx(3, vec![], vec![key2]); // Independent

        let block = Block::new(1, vec![tx1, tx2, tx3]);

        let mut access_builder = AccessListBuilder::with_heuristic();
        for tx in &block.transactions {
            access_builder.estimate_before_execution(tx);
        }

        let scheduler = MIScheduler::new(1000);
        let waves = scheduler.schedule(&block, &access_builder);

        // Should have 2 waves: {tx1, tx3} and {tx2}
        // or {tx2, tx3} and {tx1}
        assert_eq!(waves.len(), 2);
        
        let total_txs: usize = waves.iter().map(|w| w.len()).sum();
        assert_eq!(total_txs, 3);
    }

    #[test]
    fn test_max_wave_size_limit() {
        // Create 10 independent transactions
        let transactions: Vec<Transaction> = (0..10)
            .map(|i| {
                let key = Key::new([i as u8; 20], [i as u8; 32]);
                create_test_tx(i, vec![], vec![key])
            })
            .collect();

        let block = Block::new(1, transactions);

        let mut access_builder = AccessListBuilder::with_heuristic();
        for tx in &block.transactions {
            access_builder.estimate_before_execution(tx);
        }

        // Set max wave size to 5
        let scheduler = MIScheduler::new(5);
        let waves = scheduler.schedule(&block, &access_builder);

        // Should have at least 2 waves due to max size limit
        assert!(waves.len() >= 2);
        
        // No wave should exceed max size
        for wave in &waves {
            assert!(wave.len() <= 5);
        }
    }

    #[test]
    fn test_find_mis_algorithm() {
        let key = Key::new([1u8; 20], [1u8; 32]);

        let mut sets1 = AccessSets::new();
        sets1.add_read(key);

        let mut sets2 = AccessSets::new();
        sets2.add_write(key);

        let mut sets3 = AccessSets::new();
        sets3.add_read(Key::new([2u8; 20], [2u8; 32]));

        let transactions = vec![
            (1, sets1),
            (2, sets2),
            (3, sets3),
        ];

        let graph = ConflictGraph::build(&transactions);
        let available: AHashSet<u64> = vec![1, 2, 3].into_iter().collect();

        let scheduler = MIScheduler::new(1000);
        let mis = scheduler.find_mis(&graph, &available);

        // MIS should include tx1 and tx3 (they don't conflict)
        // or tx2 and tx3
        assert!(mis.len() >= 2);
        
        // Verify it's actually independent
        for &tx1_id in &mis {
            for &tx2_id in &mis {
                if tx1_id != tx2_id {
                    let neighbors1 = graph.get_neighbors(tx1_id);
                    assert!(!neighbors1.contains(&tx2_id));
                }
            }
        }
    }
}


