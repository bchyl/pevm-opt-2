use crate::types::U256;

/// EIP-2929 Gas costs for storage operations
pub const COLD_SLOAD_COST: u64 = 2100;
pub const WARM_SLOAD_COST: u64 = 100;

pub const COLD_SSTORE_COST: u64 = 20000;
pub const WARM_SSTORE_COST: u64 = 2900;
pub const SSTORE_RESET_COST: u64 = 5000;
pub const SSTORE_SET_COST: u64 = 20000;

/// Other operation costs
pub const ADD_COST: u64 = 3;
pub const SUB_COST: u64 = 3;
pub const KECCAK_BASE_COST: u64 = 30;
pub const KECCAK_WORD_COST: u64 = 6;
pub const NOOP_COST: u64 = 1;

/// Calculate SLOAD gas cost
pub fn calculate_sload_gas(is_cold: bool) -> u64 {
    if is_cold {
        COLD_SLOAD_COST
    } else {
        WARM_SLOAD_COST
    }
}

/// Calculate SSTORE gas cost according to EIP-2929
/// 
/// # Arguments
/// * `is_cold` - Whether the key is being accessed for the first time in this block
/// * `current` - Current value in storage
/// * `new_value` - New value to be stored
/// 
/// # Returns
/// Gas cost for the SSTORE operation
/// 
/// # Example
/// ```
/// // Cold access: First time touching key K in this block
/// calculate_sstore_gas(true, U256::ZERO, U256::from(100))
/// // → 20,000 gas (COLD_SSTORE_COST)
/// 
/// // Warm access: Key K already touched earlier
/// calculate_sstore_gas(false, U256::from(50), U256::from(100))
/// // → 2,900 gas (WARM_SSTORE_COST)
/// 
/// // Storage expansion: 0 → non-zero
/// calculate_sstore_gas(false, U256::ZERO, U256::from(1))
/// // → 20,000 gas (SSTORE_SET_COST)
/// ```
pub fn calculate_sstore_gas(is_cold: bool, current: U256, new_value: U256) -> u64 {
    let is_zero = current == U256::ZERO;
    let new_is_zero = new_value == U256::ZERO;

    match (is_cold, is_zero, new_is_zero) {
        // Cold access cases
        (true, _, true) => SSTORE_RESET_COST,      // Cold, writing zero
        (true, _, false) => COLD_SSTORE_COST,      // Cold, writing non-zero
        
        // Warm access cases
        (false, true, false) => SSTORE_SET_COST,   // Warm, 0 → non-0 (increasing storage)
        (false, false, true) => SSTORE_RESET_COST, // Warm, non-0 → 0 (decreasing storage, will get refund)
        (false, false, false) => WARM_SSTORE_COST, // Warm, non-0 → non-0 (modifying)
        (false, true, true) => WARM_SSTORE_COST,   // Warm, 0 → 0 (no-op, rare)
    }
}

/// Calculate Keccak256 gas cost
/// 
/// # Arguments
/// * `data_len` - Length of data to hash in bytes
/// 
/// # Returns
/// Gas cost for the Keccak operation
pub fn calculate_keccak_gas(data_len: usize) -> u64 {
    // Base cost + cost per word (32 bytes)
    KECCAK_BASE_COST + KECCAK_WORD_COST * data_len.div_ceil(32) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sload_gas() {
        assert_eq!(calculate_sload_gas(true), COLD_SLOAD_COST);
        assert_eq!(calculate_sload_gas(false), WARM_SLOAD_COST);
    }

    #[test]
    fn test_sstore_gas_cold() {
        let zero = U256::ZERO;
        let non_zero = U256::from_u64(1);

        // Cold access, writing zero
        assert_eq!(
            calculate_sstore_gas(true, non_zero, zero),
            SSTORE_RESET_COST
        );

        // Cold access, writing non-zero
        assert_eq!(
            calculate_sstore_gas(true, zero, non_zero),
            COLD_SSTORE_COST
        );
        assert_eq!(
            calculate_sstore_gas(true, non_zero, non_zero),
            COLD_SSTORE_COST
        );
    }

    #[test]
    fn test_sstore_gas_warm() {
        let zero = U256::ZERO;
        let non_zero = U256::from_u64(1);

        // Warm access, 0 → non-0
        assert_eq!(
            calculate_sstore_gas(false, zero, non_zero),
            SSTORE_SET_COST
        );

        // Warm access, non-0 → 0
        assert_eq!(
            calculate_sstore_gas(false, non_zero, zero),
            SSTORE_RESET_COST
        );

        // Warm access, non-0 → non-0
        assert_eq!(
            calculate_sstore_gas(false, non_zero, non_zero),
            WARM_SSTORE_COST
        );
    }

    #[test]
    fn test_keccak_gas() {
        // Empty data
        assert_eq!(calculate_keccak_gas(0), KECCAK_BASE_COST);

        // 1 byte (1 word)
        assert_eq!(
            calculate_keccak_gas(1),
            KECCAK_BASE_COST + KECCAK_WORD_COST
        );

        // 32 bytes (1 word)
        assert_eq!(
            calculate_keccak_gas(32),
            KECCAK_BASE_COST + KECCAK_WORD_COST
        );

        // 33 bytes (2 words)
        assert_eq!(
            calculate_keccak_gas(33),
            KECCAK_BASE_COST + 2 * KECCAK_WORD_COST
        );

        // 64 bytes (2 words)
        assert_eq!(
            calculate_keccak_gas(64),
            KECCAK_BASE_COST + 2 * KECCAK_WORD_COST
        );
    }
}


