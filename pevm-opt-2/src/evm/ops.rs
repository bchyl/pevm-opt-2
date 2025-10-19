use crate::evm::{context::ExecutionContext, gas::*};
use crate::storage::KVStore;
use crate::types::{Key, MicroOp, U256};
use blake3;

/// Execute a single micro-operation
pub fn execute_op<S: KVStore>(
    op: &MicroOp,
    ctx: &mut ExecutionContext<S>,
) -> Result<(), String> {
    match op {
        MicroOp::SLoad(key) => execute_sload(*key, ctx),
        MicroOp::SStore(key, value) => execute_sstore(*key, *value, ctx),
        MicroOp::Add(value) => execute_add(*value, ctx),
        MicroOp::Sub(value) => execute_sub(*value, ctx),
        MicroOp::Keccak(data) => execute_keccak(data, ctx),
        MicroOp::NoOp => execute_noop(ctx),
    }
}

/// Execute SLOAD operation
fn execute_sload<S: KVStore>(key: Key, ctx: &mut ExecutionContext<S>) -> Result<(), String> {
    // Check if key is cold (first access in this block)
    let is_cold = !ctx.is_warm(&key);
    
    // Calculate gas cost
    let gas_cost = calculate_sload_gas(is_cold);
    ctx.consume_gas(gas_cost)?;
    
    // Update warm/cold tracking
    if is_cold {
        ctx.cold_keys.insert(key);
        ctx.warm_keys.insert(key);
    }
    
    // Record read access
    ctx.access_sets.add_read(key);
    
    // Load value from storage
    let value = ctx.storage.get(&key);
    
    // Push to stack
    ctx.stack.push(value);
    
    tracing::trace!(
        "SLOAD {} -> {} (gas: {}, cold: {})",
        key,
        value,
        gas_cost,
        is_cold
    );
    
    Ok(())
}

/// Execute SSTORE operation
fn execute_sstore<S: KVStore>(
    key: Key,
    value: U256,
    ctx: &mut ExecutionContext<S>,
) -> Result<(), String> {
    // Check if key is cold
    let is_cold = !ctx.is_warm(&key);
    
    // Get current value for gas calculation
    let current_value = ctx.storage.get(&key);
    
    // Calculate gas cost according to EIP-2929
    let gas_cost = calculate_sstore_gas(is_cold, current_value, value);
    ctx.consume_gas(gas_cost)?;
    
    // Update warm/cold tracking
    if is_cold {
        ctx.cold_keys.insert(key);
        ctx.warm_keys.insert(key);
    }
    
    // Record write access
    ctx.access_sets.add_write(key);
    
    // Store value
    ctx.storage.set(key, value);
    
    tracing::trace!(
        "SSTORE {} <- {} (was: {}, gas: {}, cold: {})",
        key,
        value,
        current_value,
        gas_cost,
        is_cold
    );
    
    Ok(())
}

/// Execute ADD operation (add value to stack top)
fn execute_add<S: KVStore>(value: U256, ctx: &mut ExecutionContext<S>) -> Result<(), String> {
    ctx.consume_gas(ADD_COST)?;
    
    if let Some(a) = ctx.stack.pop() {
        let result = a.add(&value);
        ctx.stack.push(result);
        
        tracing::trace!("ADD {} + {} = {}", a, value, result);
        Ok(())
    } else {
        Err("Stack underflow in ADD".to_string())
    }
}

/// Execute SUB operation (subtract value from stack top)
fn execute_sub<S: KVStore>(value: U256, ctx: &mut ExecutionContext<S>) -> Result<(), String> {
    ctx.consume_gas(SUB_COST)?;
    
    if let Some(a) = ctx.stack.pop() {
        let result = a.sub(&value);
        ctx.stack.push(result);
        
        tracing::trace!("SUB {} - {} = {}", a, value, result);
        Ok(())
    } else {
        Err("Stack underflow in SUB".to_string())
    }
}

/// Execute KECCAK operation (hash data and push result to stack)
fn execute_keccak<S: KVStore>(data: &[u8], ctx: &mut ExecutionContext<S>) -> Result<(), String> {
    let gas_cost = calculate_keccak_gas(data.len());
    ctx.consume_gas(gas_cost)?;
    
    // Use blake3 as a fast hash function (in production, use Keccak256)
    let hash = blake3::hash(data);
    let hash_bytes = hash.as_bytes();
    
    // Convert to U256
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&hash_bytes[..32]);
    let result = U256(bytes);
    
    ctx.stack.push(result);
    
    tracing::trace!("KECCAK ({} bytes) = {}", data.len(), result);
    
    Ok(())
}

/// Execute NOOP operation
fn execute_noop<S: KVStore>(ctx: &mut ExecutionContext<S>) -> Result<(), String> {
    ctx.consume_gas(NOOP_COST)?;
    tracing::trace!("NOOP");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;

    #[test]
    fn test_sload() {
        let storage = MemoryStore::new();
        let mut ctx = ExecutionContext::new(storage);
        let key = Key::new([1u8; 20], [1u8; 32]);

        // First access should be cold
        execute_sload(key, &mut ctx).unwrap();
        assert_eq!(ctx.gas_used, COLD_SLOAD_COST);
        assert_eq!(ctx.stack.len(), 1);
        assert_eq!(ctx.stack[0], U256::ZERO);
        assert!(ctx.cold_keys.contains(&key));
        assert!(ctx.warm_keys.contains(&key));

        // Second access should be warm
        ctx.stack.clear();
        ctx.gas_used = 0;
        execute_sload(key, &mut ctx).unwrap();
        assert_eq!(ctx.gas_used, WARM_SLOAD_COST);
    }

    #[test]
    fn test_sstore() {
        let storage = MemoryStore::new();
        let mut ctx = ExecutionContext::new(storage);
        let key = Key::new([1u8; 20], [1u8; 32]);
        let value = U256::from_u64(42);

        // First write should be cold
        execute_sstore(key, value, &mut ctx).unwrap();
        assert!(ctx.gas_used > 0);
        assert_eq!(ctx.storage.get(&key), value);
        assert!(ctx.access_sets.writes.contains(&key));
    }

    #[test]
    fn test_add() {
        let storage = MemoryStore::new();
        let mut ctx = ExecutionContext::new(storage);
        
        ctx.stack.push(U256::from_u64(10));
        execute_add(U256::from_u64(32), &mut ctx).unwrap();
        
        assert_eq!(ctx.stack.len(), 1);
        assert_eq!(ctx.stack[0], U256::from_u64(42));
        assert_eq!(ctx.gas_used, ADD_COST);
    }

    #[test]
    fn test_sub() {
        let storage = MemoryStore::new();
        let mut ctx = ExecutionContext::new(storage);
        
        ctx.stack.push(U256::from_u64(100));
        execute_sub(U256::from_u64(58), &mut ctx).unwrap();
        
        assert_eq!(ctx.stack.len(), 1);
        assert_eq!(ctx.stack[0], U256::from_u64(42));
        assert_eq!(ctx.gas_used, SUB_COST);
    }

    #[test]
    fn test_keccak() {
        let storage = MemoryStore::new();
        let mut ctx = ExecutionContext::new(storage);
        
        let data = b"hello world";
        execute_keccak(data, &mut ctx).unwrap();
        
        assert_eq!(ctx.stack.len(), 1);
        assert!(ctx.gas_used > KECCAK_BASE_COST);
    }

    #[test]
    fn test_noop() {
        let storage = MemoryStore::new();
        let mut ctx = ExecutionContext::new(storage);
        
        execute_noop(&mut ctx).unwrap();
        assert_eq!(ctx.gas_used, NOOP_COST);
    }

    #[test]
    fn test_stack_underflow() {
        let storage = MemoryStore::new();
        let mut ctx = ExecutionContext::new(storage);
        
        // Stack is empty, ADD should fail
        let result = execute_add(U256::from_u64(1), &mut ctx);
        assert!(result.is_err());
    }
}


