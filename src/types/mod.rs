use ahash::AHashSet;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Key {
    pub address: [u8; 20],
    pub slot: [u8; 32],
}

impl Key {
    pub fn new(address: [u8; 20], slot: [u8; 32]) -> Self {
        Self { address, slot }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "0x{}:0x{}",
            hex::encode(&self.address),
            hex::encode(&self.slot)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct U256(pub [u8; 32]);

impl U256 {
    pub const ZERO: U256 = U256([0u8; 32]);
    pub const ONE: U256 = {
        let mut bytes = [0u8; 32];
        bytes[31] = 1;
        U256(bytes)
    };

    pub fn from_u64(val: u64) -> Self {
        let mut bytes = [0u8; 32];
        bytes[24..32].copy_from_slice(&val.to_be_bytes());
        U256(bytes)
    }

    pub fn to_u64(&self) -> Option<u64> {
        // Check if high bytes are zero
        if self.0[..24].iter().any(|&b| b != 0) {
            return None;
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&self.0[24..32]);
        Some(u64::from_be_bytes(bytes))
    }

    pub fn add(&self, other: &U256) -> U256 {
        let mut result = [0u8; 32];
        let mut carry = 0u16;
        for i in (0..32).rev() {
            let sum = self.0[i] as u16 + other.0[i] as u16 + carry;
            result[i] = (sum & 0xFF) as u8;
            carry = sum >> 8;
        }
        U256(result)
    }

    pub fn sub(&self, other: &U256) -> U256 {
        let mut result = [0u8; 32];
        let mut borrow = 0i16;
        for i in (0..32).rev() {
            let diff = self.0[i] as i16 - other.0[i] as i16 - borrow;
            if diff < 0 {
                result[i] = (diff + 256) as u8;
                borrow = 1;
            } else {
                result[i] = diff as u8;
                borrow = 0;
            }
        }
        U256(result)
    }
}

impl fmt::Display for U256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(&self.0))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MicroOp {
    SLoad(Key),
    SStore(Key, U256),
    Add(U256),
    Sub(U256),
    Keccak(Vec<u8>),
    NoOp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: u64,
    pub reads: Vec<Key>,
    pub writes: Vec<Key>,
    pub gas_hint: u64,
    pub metadata: TransactionMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionMetadata {
    pub program: Vec<MicroOp>,
    pub access_list: Vec<Key>,
    pub blob_size: u64,
    pub nonce: u64,
    pub from: [u8; 20],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub number: u64,
    pub timestamp: u64,
    pub transactions: Vec<Transaction>,
    pub parent_hash: [u8; 32],
}

impl Block {
    pub fn new(number: u64, transactions: Vec<Transaction>) -> Self {
        Self {
            number,
            timestamp: chrono::Utc::now().timestamp() as u64,
            transactions,
            parent_hash: [0u8; 32],
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AccessSets {
    pub reads: AHashSet<Key>,
    pub writes: AHashSet<Key>,
}

impl Serialize for AccessSets {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AccessSets", 2)?;
        let reads_vec: Vec<Key> = self.reads.iter().copied().collect();
        let writes_vec: Vec<Key> = self.writes.iter().copied().collect();
        state.serialize_field("reads", &reads_vec)?;
        state.serialize_field("writes", &writes_vec)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for AccessSets {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct AccessSetsHelper {
            reads: Vec<Key>,
            writes: Vec<Key>,
        }

        let helper = AccessSetsHelper::deserialize(deserializer)?;
        Ok(AccessSets {
            reads: helper.reads.into_iter().collect(),
            writes: helper.writes.into_iter().collect(),
        })
    }
}

impl AccessSets {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_read(&mut self, key: Key) {
        self.reads.insert(key);
    }

    pub fn add_write(&mut self, key: Key) {
        self.writes.insert(key);
    }

    pub fn merge(&mut self, other: &AccessSets) {
        self.reads.extend(&other.reads);
        self.writes.extend(&other.writes);
    }

    pub fn has_conflict_with(&self, other: &AccessSets) -> bool {
        !self.writes.is_disjoint(&other.writes)
            || !self.writes.is_disjoint(&other.reads)
            || !self.reads.is_disjoint(&other.writes)
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub tx_id: u64,
    pub success: bool,
    pub gas_used: u64,
    pub access_sets: AccessSets,
    pub warm_keys: AHashSet<Key>,
    pub cold_keys: AHashSet<Key>,
    pub reverted: bool,
    pub error: Option<String>,
}

impl ExecutionResult {
    pub fn success(
        tx_id: u64,
        gas_used: u64,
        access_sets: AccessSets,
        warm_keys: AHashSet<Key>,
        cold_keys: AHashSet<Key>,
    ) -> Self {
        Self {
            tx_id,
            success: true,
            gas_used,
            access_sets,
            warm_keys,
            cold_keys,
            reverted: false,
            error: None,
        }
    }

    pub fn failure(tx_id: u64, error: String) -> Self {
        Self {
            tx_id,
            success: false,
            gas_used: 0,
            access_sets: AccessSets::new(),
            warm_keys: AHashSet::new(),
            cold_keys: AHashSet::new(),
            reverted: true,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metrics {
    pub waves: usize,
    pub avg_wave_size: f64,
    pub speedup_vs_serial: f64,
    pub conflict_rate: f64,
    pub preexec_precision: f64,
    pub preexec_recall: f64,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub tx_latency_p50: f64,
    pub tx_latency_p95: f64,
    pub tx_latency_p99: f64,
    pub iops: f64,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            waves: 0,
            avg_wave_size: 0.0,
            speedup_vs_serial: 1.0,
            conflict_rate: 0.0,
            preexec_precision: 1.0,
            preexec_recall: 1.0,
            false_positives: 0,
            false_negatives: 0,
            tx_latency_p50: 0.0,
            tx_latency_p95: 0.0,
            tx_latency_p99: 0.0,
            iops: 0.0,
        }
    }
}

mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types() {
        let a = U256::from_u64(100);
        let b = U256::from_u64(50);
        assert_eq!(a.add(&b).to_u64(), Some(150));

        let mut set1 = AccessSets::new();
        set1.add_write(Key::new([1u8; 20], [1u8; 32]));
        let mut set2 = AccessSets::new();
        set2.add_read(Key::new([1u8; 20], [1u8; 32]));
        assert!(set1.has_conflict_with(&set2));
    }
}
