use crate::types::AccessSets;
use ahash::{AHashMap, AHashSet};

#[derive(Clone)]
pub struct ConflictGraph {
    nodes: AHashMap<u64, AccessSets>,
    edges: AHashMap<u64, AHashSet<u64>>,
}

impl Default for ConflictGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ConflictGraph {
    pub fn new() -> Self {
        Self {
            nodes: AHashMap::new(),
            edges: AHashMap::new(),
        }
    }

    fn add_edge(&mut self, tx1: u64, tx2: u64) {
        self.edges.entry(tx1).or_default().insert(tx2);
        self.edges.entry(tx2).or_default().insert(tx1);
    }

    pub fn has_conflict(&self, tx1: u64, tx2: u64) -> bool {
        self.edges
            .get(&tx1)
            .is_some_and(|neighbors| neighbors.contains(&tx2))
    }

    pub fn build(transactions: &[(u64, AccessSets)]) -> Self {
        use crate::types::Key;
        let mut graph = Self::new();

        for (id, sets) in transactions {
            graph.nodes.insert(*id, sets.clone());
            graph.edges.entry(*id).or_default();
        }

        let mut key_index: AHashMap<Key, Vec<u64>> = AHashMap::new();
        for (id, sets) in transactions {
            for key in sets.reads.iter().chain(sets.writes.iter()) {
                key_index.entry(*key).or_default().push(*id);
            }
        }

        let mut checked: AHashSet<(u64, u64)> = AHashSet::new();
        for (id, sets) in transactions {
            let mut candidates = AHashSet::new();
            for key in sets.reads.iter().chain(sets.writes.iter()) {
                if let Some(list) = key_index.get(key) {
                    candidates.extend(list);
                }
            }

            for &other in &candidates {
                if other == *id {
                    continue;
                }
                let pair = if *id < other {
                    (*id, other)
                } else {
                    (other, *id)
                };
                if checked.insert(pair) {
                    if let Some(other_sets) = graph.nodes.get(&other) {
                        if sets.has_conflict_with(other_sets) {
                            graph.add_edge(*id, other);
                        }
                    }
                }
            }
        }
        graph
    }
}
