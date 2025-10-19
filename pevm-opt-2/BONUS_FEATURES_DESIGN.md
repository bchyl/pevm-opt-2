# Bonus Features Design Document

## 1. Database Backend Comparison


```
┌─────────────────────────────────────────────────────────────────┐
│               Storage Backend Architecture Comparison            │
└─────────────────────────────────────────────────────────────────┘

                        Application Layer
                               │
                               ▼
        ┌──────────────────────────────────────────────┐
        │        StorageBackend Trait (Unified API)    │
        │  • get(key) -> value                         │
        │  • set(key, value)                           │
        │  • batch_get(keys) -> values                 │
        │  • batch_set(kvs)                            │
        │  • get_metrics() -> StorageMetrics           │
        └──────────┬───────────────┬────────────┬──────┘
                   │               │            │
        ┌──────────▼─────┐  ┌─────▼─────┐  ┌──▼────────┐
        │  MemoryMapped   │  │  RocksDB  │  │   Sled    │
        │   (mmap)        │  │  (LSM)    │  │ (B-Tree)  │
        └─────────────────┘  └───────────┘  └───────────┘


┌─────────────────────────────────────────────────────────────────┐
│              1. Memory-mapped File Backend (mmap)                │
└─────────────────────────────────────────────────────────────────┘

    ┌──────────────────────────────────────────────────┐
    │          Application Process                      │
    │  ┌────────────────────────────────────────────┐  │
    │  │  Virtual Memory (process address space)    │  │
    │  │                                            │  │
    │  │  ┌──────────────────────────────────────┐ │  │
    │  │  │  mmap region (file-backed pages)     │ │  │
    │  │  │  ┌────────┬────────┬────────┬─────┐  │ │  │
    │  │  │  │ Page 1 │ Page 2 │ Page 3 │ ... │  │ │  │
    │  │  │  └────────┴────────┴────────┴─────┘  │ │  │
    │  │  └──────────────────────────────────────┘ │  │
    │  └────────────────────────────────────────────┘  │
    └─────────────────────┬────────────────────────────┘
                          │ Page fault
                          ▼
    ┌──────────────────────────────────────────────────┐
    │              Kernel (OS)                          │
    │  ┌────────────────────────────────────────────┐  │
    │  │         Page Cache                         │  │
    │  │  (OS automatically manages caching)        │  │
    │  └────────────────────────────────────────────┘  │
    └─────────────────────┬────────────────────────────┘
                          │ Disk I/O (if not cached)
                          ▼
    ┌──────────────────────────────────────────────────┐
    │              Physical Storage                     │
    │           (SSD/NVMe/Hard Disk)                   │
    └──────────────────────────────────────────────────┘

    Pros:
    • OS-managed page cache (automatic)
    • Zero-copy reads for cached data
    • Simple implementation
    • 500K+ IOPS for hot data
    
    Cons:
    • High memory usage (no compression)
    • Page fault overhead for cold data
    • No built-in consistency guarantees


┌─────────────────────────────────────────────────────────────────┐
│                   2. RocksDB Backend (LSM-Tree)                  │
└─────────────────────────────────────────────────────────────────┘

    Application
         │
         ▼
    ┌────────────────────────────────────────────────┐
    │              RocksDB (LSM-Tree)                │
    │                                                │
    │  ┌──────────────────────────────────────────┐ │
    │  │         MemTable (in-memory)             │ │
    │  │  ┌────────────────────────────────────┐  │ │
    │  │  │  Recent writes (sorted)            │  │ │
    │  │  │  • key1 → value1                   │  │ │
    │  │  │  • key2 → value2                   │  │ │
    │  │  └────────────────────────────────────┘  │ │
    │  └──────────────────────────────────────────┘ │
    │         │ Flush when full                     │
    │         ▼                                      │
    │  ┌──────────────────────────────────────────┐ │
    │  │    Immutable MemTable (frozen)           │ │
    │  └──────────────────────────────────────────┘ │
    │         │ Background compaction               │
    │         ▼                                      │
    │  ┌──────────────────────────────────────────┐ │
    │  │         SST Files (on disk)              │ │
    │  │                                          │ │
    │  │  Level 0:  [SST1][SST2][SST3]           │ │
    │  │            (may overlap)                 │ │
    │  │                                          │ │
    │  │  Level 1:  [SST4──────][SST5──────]     │ │
    │  │            (sorted, no overlap)         │ │
    │  │                                          │ │
    │  │  Level 2:  [SST6──────][SST7─────...    │ │
    │  │            (10x larger than L1)         │ │
    │  │                                          │ │
    │  │  Level N:  [Compressed, cold data]      │ │
    │  └──────────────────────────────────────────┘ │
    │                                                │
    │  ┌──────────────────────────────────────────┐ │
    │  │      Bloom Filters (in-memory)           │ │
    │  │  • Quick negative lookups                │ │
    │  │  • 10 bits per key                       │ │
    │  └──────────────────────────────────────────┘ │
    └────────────────────────────────────────────────┘

    Read Path:
    1. Check MemTable (in-memory)
    2. Check Immutable MemTable
    3. Check Bloom filters → SST files (L0 → LN)
    4. Decompress blocks → Return value
    
    Write Path:
    1. Write to WAL (durability)
    2. Write to MemTable (in-memory)
    3. Return (async flush/compaction)
    
    Pros:
    • Excellent write throughput (100K+ writes/s)
    • Good compression (LZ4/Snappy)
    • Configurable cache sizes
    • Production-proven (many projects)
    
    Cons:
    • Read amplification (multiple levels)
    • Compaction overhead
    • Higher latency vs mmap (P99: 100μs)


┌─────────────────────────────────────────────────────────────────┐
│                    3. Sled Backend (B-Tree)                      │
└─────────────────────────────────────────────────────────────────┘

    ┌────────────────────────────────────────────────┐
    │                  Sled DB                       │
    │                                                │
    │  ┌──────────────────────────────────────────┐ │
    │  │     Lock-free B-Tree (in-memory + disk)  │ │
    │  │                                          │ │
    │  │         [Root Node]                      │ │
    │  │            /    \                        │ │
    │  │      [Branch]  [Branch]                  │ │
    │  │       /  |  \    /  |  \                 │ │
    │  │   [Leaf][L][L][L][L][L]                  │ │
    │  │                                          │ │
    │  │   Each node = 4KB page                   │ │
    │  │   Copy-on-write for updates              │ │
    │  └──────────────────────────────────────────┘ │
    │                                                │
    │  ┌──────────────────────────────────────────┐ │
    │  │      Page Cache (configurable)           │ │
    │  │  • LRU eviction                          │ │
    │  │  • 128MB default                         │ │
    │  └──────────────────────────────────────────┘ │
    │                                                │
    │  ┌──────────────────────────────────────────┐ │
    │  │      Batch Flushing (1s interval)        │ │
    │  │  • Group writes for efficiency           │ │
    │  └──────────────────────────────────────────┘ │
    └────────────────────────────────────────────────┘

    Pros:
    • Rust-native (zero-copy, safe)
    • Simple API
    • Good balanced performance (200K IOPS)
    • Embedded database (no separate process)
    
    Cons:
    • Less mature than RocksDB
    • Limited configuration options
    • Medium latency (P99: 50μs)

```rust
/// Storage backend trait with performance metrics
pub trait StorageBackend: Clone + Send + Sync {
    fn get(&self, key: &Key) -> Result<U256, StorageError>;
    fn set(&mut self, key: Key, value: U256) -> Result<(), StorageError>;
    fn batch_get(&self, keys: &[Key]) -> Result<Vec<U256>, StorageError>;
    fn batch_set(&mut self, kvs: &[(Key, U256)]) -> Result<(), StorageError>;
    
    // Performance metrics
    fn get_metrics(&self) -> StorageMetrics;
}

#[derive(Debug, Clone)]
pub struct StorageMetrics {
    pub total_reads: u64,
    pub total_writes: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub avg_read_latency_ns: u64,
    pub p99_read_latency_ns: u64,
    pub avg_write_latency_ns: u64,
    pub p99_write_latency_ns: u64,
}
```

### Memory-mapped File Backend

```rust
use memmap2::MmapMut;
use std::fs::OpenOptions;

pub struct MmapStorage {
    /// Memory-mapped file
    mmap: MmapMut,
    
    /// Index: Key -> offset in mmap
    index: AHashMap<Key, u64>,
    
    /// Performance metrics
    metrics: StorageMetrics,
    
    /// Page cache stats
    page_cache_hits: AtomicU64,
}

impl MmapStorage {
    pub fn new(file_path: &str, capacity: usize) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(file_path)?;
        
        // Pre-allocate file
        file.set_len(capacity as u64)?;
        
        let mmap = unsafe { MmapMut::map_mut(&file)? };
        
        Ok(Self {
            mmap,
            index: AHashMap::new(),
            metrics: StorageMetrics::default(),
            page_cache_hits: AtomicU64::new(0),
        })
    }
    
    fn get_impl(&self, key: &Key) -> Result<U256> {
        let start = Instant::now();
        
        let offset = self.index.get(key)
            .ok_or(StorageError::KeyNotFound)?;
        
        // Read from mmap (kernel will use page cache)
        let bytes = &self.mmap[*offset as usize..*offset as usize + 32];
        let value = U256::from_bytes(bytes);
        
        // Update metrics
        let latency = start.elapsed().as_nanos() as u64;
        self.record_read_latency(latency);
        
        Ok(value)
    }
}
```

### RocksDB Backend

```rust
use rocksdb::{DB, Options, WriteBatch};

pub struct RocksDBStorage {
    db: Arc<DB>,
    metrics: StorageMetrics,
    
    /// Write buffer for batching
    write_buffer: Vec<(Key, U256)>,
    batch_size: usize,
}

impl RocksDBStorage {
    pub fn new(path: &str) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        
        // Optimize for write-heavy workload
        opts.set_write_buffer_size(64 * 1024 * 1024);
        opts.set_max_write_buffer_number(3);
        opts.set_target_file_size_base(64 * 1024 * 1024);
        
        // Enable compression for cold data
        opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        
        // Bloom filter for faster lookups
        opts.set_bloom_filter(10, false);
        
        let db = DB::open(&opts, path)?;
        
        Ok(Self {
            db: Arc::new(db),
            metrics: StorageMetrics::default(),
            write_buffer: Vec::new(),
            batch_size: 1000,
        })
    }
    
    fn batch_write(&mut self) -> Result<()> {
        if self.write_buffer.is_empty() {
            return Ok(());
        }
        
        let start = Instant::now();
        let mut batch = WriteBatch::default();
        
        for (key, value) in &self.write_buffer {
            let key_bytes = key.to_bytes();
            let value_bytes = value.to_bytes();
            batch.put(key_bytes, value_bytes);
        }
        
        self.db.write(batch)?;
        self.write_buffer.clear();
        
        let latency = start.elapsed().as_nanos() as u64;
        self.record_write_latency(latency);
        
        Ok(())
    }
}
```

### Sled Backend

```rust
use sled::{Db, Config};

pub struct SledStorage {
    db: Db,
    metrics: StorageMetrics,
}

impl SledStorage {
    pub fn new(path: &str) -> Result<Self> {
        let config = Config::new()
            .path(path)
            .cache_capacity(128 * 1024 * 1024) // 128MB cache
            .flush_every_ms(Some(1000)) // Batch writes
            .mode(sled::Mode::HighThroughput);
        
        let db = config.open()?;
        
        Ok(Self {
            db,
            metrics: StorageMetrics::default(),
        })
    }
}
```

## 2. JIT Compilation Path (Wasm)

```
┌─────────────────────────────────────────────────────────────────┐
│                  JIT Compilation Architecture                    │
└─────────────────────────────────────────────────────────────────┘

    Transaction with MicroOps
           │
           ▼
    ┌──────────────────────────────────────────────┐
    │      Step 1: MicroOp → Wasm Bytecode         │
    │                                              │
    │  MicroOp::SLoad(key)                         │
    │      ↓                                       │
    │  (i64.const key)                             │
    │  (call $sload)                               │
    │                                              │
    │  MicroOp::Add(value)                         │
    │      ↓                                       │
    │  (local.get $stack_top)                      │
    │  (i64.const value)                           │
    │  (i64.add)                                   │
    │  (local.set $stack_top)                      │
    │                                              │
    │  MicroOp::SStore(key, val)                   │
    │      ↓                                       │
    │  (i64.const key)                             │
    │  (i64.const val)                             │
    │  (call $sstore)                              │
    └──────────────┬───────────────────────────────┘
                   │
                   ▼
    ┌──────────────────────────────────────────────┐
    │      Step 2: Wasm Module Creation            │
    │                                              │
    │  ┌────────────────────────────────────────┐ │
    │  │  (module                               │ │
    │  │    (import "env" "sload"               │ │
    │  │      (func $sload (param i64) (result i64))) │
    │  │    (import "env" "sstore"              │ │
    │  │      (func $sstore (param i64 i64)))   │ │
    │  │    (import "env" "gas"                 │ │
    │  │      (func $gas (param i64)))          │ │
    │  │                                        │ │
    │  │    (func $execute (result i32)         │ │
    │  │      (local $stack_top i64)            │ │
    │  │      ;; Generated code here            │ │
    │  │      (i32.const 0))                    │ │
    │  │                                        │ │
    │  │    (export "execute" (func $execute))  │ │
    │  │  )                                     │ │
    │  └────────────────────────────────────────┘ │
    └──────────────┬───────────────────────────────┘
                   │
                   ▼
    ┌──────────────────────────────────────────────┐
    │      Step 3: Cranelift JIT Compilation       │
    │                                              │
    │  Wasm Bytecode                               │
    │       ↓                                      │
    │  [Cranelift IR]                              │
    │       ↓                                      │
    │  [Optimizations]                             │
    │   • Constant folding                         │
    │   • Dead code elimination                    │
    │   • Register allocation                      │
    │       ↓                                      │
    │  [Native Machine Code]                       │
    │   x86_64 / ARM64 / ...                       │
    └──────────────┬───────────────────────────────┘
                   │
                   ▼
    ┌──────────────────────────────────────────────┐
    │      Step 4: Execute with Host Functions     │
    │                                              │
    │  Native Code ←─┬─→ Host Functions           │
    │                │   • sload(key) → value      │
    │                │   • sstore(key, val)        │
    │                │   • gas(amount)             │
    │                │   • keccak(data) → hash     │
    │                │                             │
    │                └─→ ExecutionContext          │
    │                    • storage                 │
    │                    • gas_used                │
    │                    • stack                   │
    │                    • warm_keys               │
    └──────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────┐
│                    Compilation Pipeline Detail                   │
└─────────────────────────────────────────────────────────────────┘

    MicroOps                 Wasm                 Cranelift IR         Native
    --------                 ----                 ------------         ------
    
    SLoad(K1)               i64.const K1         v0 = iconst K1       mov rax, K1
      ↓                     call $sload          v1 = call sload      call [sload]
    stack.push       →         ↓            →     local.set $s0  →    mov [rbp-8], rax
                            local.set $s0         
                            
    Add(10)                 local.get $s0        v2 = load $s0        mov rax, [rbp-8]
      ↓                     i64.const 10         v3 = iconst 10       add rax, 10
    stack.top+=10    →      i64.add        →     v4 = iadd v2, v3 →   mov [rbp-8], rax
                            local.set $s0        store $s0, v4
                            
    SStore(K2,v)            i64.const K2         v5 = iconst K2       mov rdi, K2
      ↓                     local.get $s0        v6 = load $s0        mov rsi, [rbp-8]
    storage[K2]=v    →      call $sstore   →     call sstore     →    call [sstore]


┌─────────────────────────────────────────────────────────────────┐
│                      Gas Metering Integration                    │
└─────────────────────────────────────────────────────────────────┘

    Original Wasm:                  Instrumented Wasm:
    
    (func $execute                  (func $execute
      ;; Load                         ;; Gas check
      (i64.const 0x123)                (call $consume_gas (i64.const 2100))
      (call $sload)                    ;; Load
                                       (i64.const 0x123)
      ;; Add                           (call $sload)
      (i64.const 10)                   ;; Gas check
      (i64.add)                        (call $consume_gas (i64.const 3))
                                       ;; Add
      ;; Store                         (i64.const 10)
      (i64.const 0x456)                (i64.add)
      (local.get 0)                    ;; Gas check
      (call $sstore)                   (call $consume_gas (i64.const 20000))
    )                                  ;; Store
                                       (i64.const 0x456)
                                       (local.get 0)
                                       (call $sstore)
                                     )


```rust
use wasmtime::*;

/// JIT-compiled transaction executor
pub struct WasmExecutor {
    engine: Engine,
    linker: Linker<ExecutionContext>,
    module_cache: AHashMap<u64, Module>, // tx_id -> compiled module
}

impl WasmExecutor {
    pub fn new() -> Result<Self> {
        let mut config = Config::new();
        config.strategy(Strategy::Cranelift)?;
        config.cranelift_opt_level(OptLevel::Speed)?;
        
        let engine = Engine::new(&config)?;
        let mut linker = Linker::new(&engine);
        
        // Register host functions (gas metering, storage access)
        Self::register_host_functions(&mut linker)?;
        
        Ok(Self {
            engine,
            linker,
            module_cache: AHashMap::new(),
        })
    }
    
    /// Compile transaction to Wasm module
    pub fn compile_transaction(&mut self, tx: &Transaction) -> Result<Module> {
        // 1. Generate Wasm bytecode from micro-ops
        let wasm_bytes = self.generate_wasm(&tx.metadata.program)?;
        
        // 2. Compile with Cranelift
        let module = Module::new(&self.engine, &wasm_bytes)?;
        
        // 3. Cache compiled module
        self.module_cache.insert(tx.id, module.clone());
        
        Ok(module)
    }
    
    /// Generate Wasm bytecode from micro-ops
    fn generate_wasm(&self, ops: &[MicroOp]) -> Result<Vec<u8>> {
        use wasm_encoder::*;
        
        let mut module = Module::new();
        
        // Type section
        let mut types = TypeSection::new();
        types.function(vec![], vec![ValType::I32]); // Main function
        module.section(&types);
        
        // Function section
        let mut functions = FunctionSection::new();
        functions.function(0); // Use type 0
        module.section(&functions);
        
        // Export section
        let mut exports = ExportSection::new();
        exports.export("execute", ExportKind::Func, 0);
        module.section(&exports);
        
        // Code section
        let mut codes = CodeSection::new();
        let mut func = Function::new(vec![]);
        
        // Translate each micro-op to Wasm instructions
        for op in ops {
            self.emit_wasm_for_op(&mut func, op)?;
        }
        
        func.instruction(&Instruction::I32Const(0)); // Success
        func.instruction(&Instruction::End);
        codes.function(&func);
        module.section(&codes);
        
        Ok(module.finish())
    }
    
    /// Emit Wasm instructions for a micro-op
    fn emit_wasm_for_op(
        &self,
        func: &mut Function,
        op: &MicroOp,
    ) -> Result<()> {
        match op {
            MicroOp::SLoad(key) => {
                // Call host function: sload(key) -> value
                func.instruction(&Instruction::I64Const(key.as_u64()));
                func.instruction(&Instruction::Call(HOST_SLOAD_INDEX));
            }
            
            MicroOp::SStore(key, value) => {
                // Call host function: sstore(key, value)
                func.instruction(&Instruction::I64Const(key.as_u64()));
                func.instruction(&Instruction::I64Const(value.as_u64()));
                func.instruction(&Instruction::Call(HOST_SSTORE_INDEX));
            }
            
            MicroOp::Add(value) => {
                // Stack operation: top += value
                func.instruction(&Instruction::LocalGet(STACK_TOP_LOCAL));
                func.instruction(&Instruction::I64Const(value.as_u64()));
                func.instruction(&Instruction::I64Add);
                func.instruction(&Instruction::LocalSet(STACK_TOP_LOCAL));
            }
            
            MicroOp::Keccak(data) => {
                // Call host function: keccak(data) -> hash
                let data_ptr = self.allocate_data(func, data)?;
                func.instruction(&Instruction::I32Const(data_ptr as i32));
                func.instruction(&Instruction::I32Const(data.len() as i32));
                func.instruction(&Instruction::Call(HOST_KECCAK_INDEX));
            }
            
            _ => {} // Other ops
        }
        
        Ok(())
    }
    
    /// Register host functions for Wasm
    fn register_host_functions(linker: &mut Linker<ExecutionContext>) -> Result<()> {
        // Storage load
        linker.func_wrap("env", "sload",
            |mut caller: Caller<ExecutionContext>, key: i64| -> i64 {
                let ctx = caller.data_mut();
                let key = Key::from_u64(key as u64);
                let value = ctx.storage.get(&key);
                
                // Gas metering
                let is_cold = !ctx.is_warm(&key);
                ctx.consume_gas(calculate_sload_gas(is_cold)).unwrap();
                ctx.warm_up(key);
                
                value.as_u64() as i64
            }
        )?;
        
        // Storage store
        linker.func_wrap("env", "sstore",
            |mut caller: Caller<ExecutionContext>, key: i64, value: i64| {
                let ctx = caller.data_mut();
                let key = Key::from_u64(key as u64);
                let value = U256::from_u64(value as u64);
                
                // Gas metering
                let current = ctx.storage.get(&key);
                let is_cold = !ctx.is_warm(&key);
                ctx.consume_gas(
                    calculate_sstore_gas(is_cold, current, value)
                ).unwrap();
                
                ctx.storage.set(key, value);
                ctx.warm_up(key);
            }
        )?;
        
        // Keccak256
        linker.func_wrap("env", "keccak",
            |mut caller: Caller<ExecutionContext>, ptr: i32, len: i32| -> i64 {
                let ctx = caller.data();
                let memory = caller.get_export("memory")
                    .unwrap()
                    .into_memory()
                    .unwrap();
                
                let data = &memory.data(&caller)[ptr as usize..(ptr + len) as usize];
                let hash = blake3::hash(data);
                
                // Gas metering
                ctx.consume_gas(calculate_keccak_gas(len as usize)).unwrap();
                
                u64::from_be_bytes(hash.as_bytes()[0..8].try_into().unwrap()) as i64
            }
        )?;
        
        Ok(())
    }
    
    /// Execute compiled Wasm module
    pub fn execute_wasm(
        &self,
        module: &Module,
        ctx: ExecutionContext,
    ) -> Result<ExecutionResult> {
        let mut store = Store::new(&self.engine, ctx);
        let instance = self.linker.instantiate(&mut store, module)?;
        
        let execute = instance
            .get_typed_func::<(), i32>(&mut store, "execute")?;
        
        let result = execute.call(&mut store, ())?;
        
        let final_ctx = store.into_data();
        
        Ok(ExecutionResult {
            success: result == 0,
            gas_used: final_ctx.gas_used,
            access_sets: final_ctx.access_sets,
            // ...
        })
    }
}
```

### Gas Preservation

```rust
/// Gas metering wrapper for Wasm
pub struct GasMeteredWasm {
    /// Remaining gas
    gas_remaining: u64,
    
    /// Gas limit
    gas_limit: u64,
}

impl GasMeteredWasm {
    /// Inject gas metering into Wasm module
    pub fn inject_metering(wasm: &[u8], gas_limit: u64) -> Result<Vec<u8>> {
        use wasm_instrument::gas_metering;
        
        let rules = gas_metering::ConstantCostRules::new(1);
        let metered = gas_metering::inject(wasm, rules, "gas")?;
        
        Ok(metered)
    }
}
```

### expect

| Workload | Interpreter | Wasm JIT | Speedup |
|----------|-------------|----------|---------|
| Simple ops | 100 ns/op | 20 ns/op | 5x |
| Complex ops | 500 ns/op | 150 ns/op | 3.3x |
| Memory ops | 200 ns/op | 50 ns/op | 4x |