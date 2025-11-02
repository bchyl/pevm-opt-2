use crate::evm::{context::ExecutionContext, gas::*};
use crate::storage::KVStore;
use crate::types::{Key, MicroOp, U256};
use blake3;

pub fn execute_op<S: KVStore>(op: &MicroOp, ctx: &mut ExecutionContext<S>) -> Result<(), String> {
    match op {
        MicroOp::SLoad(key) => execute_sload(*key, ctx),
        MicroOp::SStore(key, value) => execute_sstore(*key, *value, ctx),
        MicroOp::Add(value) => execute_add(*value, ctx),
        MicroOp::Sub(value) => execute_sub(*value, ctx),
        MicroOp::Keccak(data) => execute_keccak(data, ctx),
        MicroOp::NoOp => execute_noop(ctx),
    }
}

fn execute_sload<S: KVStore>(key: Key, ctx: &mut ExecutionContext<S>) -> Result<(), String> {
    let is_cold = !ctx.is_warm(&key);
    ctx.consume_gas(calculate_sload_gas(is_cold))?;

    if is_cold {
        ctx.cold_keys.insert(key);
        ctx.warm_keys.insert(key);
    }

    ctx.access_sets.add_read(key);
    ctx.stack.push(ctx.storage.get(&key));
    Ok(())
}

fn execute_sstore<S: KVStore>(
    key: Key,
    value: U256,
    ctx: &mut ExecutionContext<S>,
) -> Result<(), String> {
    let is_cold = !ctx.is_warm(&key);
    let current_value = ctx.storage.get(&key);
    ctx.consume_gas(calculate_sstore_gas(is_cold, current_value, value))?;

    if is_cold {
        ctx.cold_keys.insert(key);
        ctx.warm_keys.insert(key);
    }

    ctx.access_sets.add_write(key);
    ctx.storage.set(key, value);
    Ok(())
}

fn execute_add<S: KVStore>(value: U256, ctx: &mut ExecutionContext<S>) -> Result<(), String> {
    ctx.consume_gas(ADD_COST)?;
    if let Some(a) = ctx.stack.pop() {
        ctx.stack.push(a.add(&value));
        Ok(())
    } else {
        Err("Stack underflow in ADD".to_string())
    }
}

fn execute_sub<S: KVStore>(value: U256, ctx: &mut ExecutionContext<S>) -> Result<(), String> {
    ctx.consume_gas(SUB_COST)?;
    if let Some(a) = ctx.stack.pop() {
        ctx.stack.push(a.sub(&value));
        Ok(())
    } else {
        Err("Stack underflow in SUB".to_string())
    }
}

fn execute_keccak<S: KVStore>(data: &[u8], ctx: &mut ExecutionContext<S>) -> Result<(), String> {
    ctx.consume_gas(calculate_keccak_gas(data.len()))?;
    let hash = blake3::hash(data);
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&hash.as_bytes()[..32]);
    ctx.stack.push(U256(bytes));
    Ok(())
}

fn execute_noop<S: KVStore>(ctx: &mut ExecutionContext<S>) -> Result<(), String> {
    ctx.consume_gas(NOOP_COST)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;

    #[test]
    fn test_ops() {
        let mut ctx = ExecutionContext::new(MemoryStore::new());
        let key = Key::new([1u8; 20], [1u8; 32]);
        execute_sstore(key, U256::from_u64(42), &mut ctx).unwrap();
        assert_eq!(ctx.storage.get(&key), U256::from_u64(42));
    }
}
