use crate::types::U256;

pub const COLD_SLOAD_COST: u64 = 2100;
pub const WARM_SLOAD_COST: u64 = 100;
pub const COLD_SSTORE_COST: u64 = 20000;
pub const WARM_SSTORE_COST: u64 = 2900;
pub const SSTORE_RESET_COST: u64 = 5000;
pub const SSTORE_SET_COST: u64 = 20000;
pub const ADD_COST: u64 = 3;
pub const SUB_COST: u64 = 3;
pub const KECCAK_BASE_COST: u64 = 30;
pub const KECCAK_WORD_COST: u64 = 6;
pub const NOOP_COST: u64 = 1;

pub fn calculate_sload_gas(is_cold: bool) -> u64 {
    if is_cold {
        COLD_SLOAD_COST
    } else {
        WARM_SLOAD_COST
    }
}

pub fn calculate_sstore_gas(is_cold: bool, current: U256, new_value: U256) -> u64 {
    let is_zero = current == U256::ZERO;
    let new_is_zero = new_value == U256::ZERO;

    match (is_cold, is_zero, new_is_zero) {
        (true, _, true) => SSTORE_RESET_COST,
        (true, _, false) => COLD_SSTORE_COST,
        (false, true, false) => SSTORE_SET_COST,
        (false, false, true) => SSTORE_RESET_COST,
        (false, false, false) => WARM_SSTORE_COST,
        (false, true, true) => WARM_SSTORE_COST,
    }
}

pub fn calculate_keccak_gas(data_len: usize) -> u64 {
    KECCAK_BASE_COST + KECCAK_WORD_COST * data_len.div_ceil(32) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gas_calculations() {
        assert_eq!(calculate_sload_gas(true), COLD_SLOAD_COST);
        assert_eq!(calculate_sload_gas(false), WARM_SLOAD_COST);
        assert_eq!(
            calculate_keccak_gas(32),
            KECCAK_BASE_COST + KECCAK_WORD_COST
        );
    }
}
