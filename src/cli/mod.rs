use crate::evm::execute_serial;
use crate::generator::BlockGenerator;
use crate::metrics::MetricsCollector;
use crate::scheduler::{AccessListBuilder, HeuristicOracle, MIScheduler, ParallelExecutor};
use crate::storage::{KVStore, MemoryStore};
use crate::types::Block;
use clap::{Parser, Subcommand};
use std::time::Instant;
use tracing::{error, info};

fn verify_states<S: KVStore>(state1: &S, state2: &S) -> bool {
    let keys1: std::collections::HashSet<_> = state1.keys().into_iter().collect();
    let keys2: std::collections::HashSet<_> = state2.keys().into_iter().collect();

    if keys1 != keys2 {
        error!(
            "State mismatch: serial {} keys, parallel {} keys",
            keys1.len(),
            keys2.len()
        );
        return false;
    }

    for key in &keys1 {
        let val1 = state1.get(key);
        let val2 = state2.get(key);
        if val1 != val2 {
            error!(
                "Value mismatch at key {:?}: serial={:?}, parallel={:?}",
                key, val1, val2
            );
            return false;
        }
    }

    true
}

#[derive(Parser)]
#[command(name = "pevm-opt-2")]
#[command(about = "Parallel EVM Transaction Scheduler", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Generate {
        #[arg(long, default_value = "1000")]
        n_tx: usize,

        #[arg(long, default_value = "10000")]
        key_space: usize,

        #[arg(long, default_value = "0.2")]
        conflict_ratio: f64,

        #[arg(long, default_value = "0.3")]
        cold_ratio: f64,

        #[arg(long, default_value = "42")]
        seed: u64,

        #[arg(long, default_value = "block.json")]
        output: String,
    },

    Execute {
        #[arg(long)]
        input: String,

        #[arg(long, default_value = "parallel")]
        mode: String, // "serial" | "parallel"
    },

    Benchmark {
        #[arg(long)]
        input: Option<String>,

        #[arg(long)]
        preset: Option<String>, // "small" | "medium" | "large"

        #[arg(long, default_value = "results.json")]
        output: String,
    },
}

pub fn handle_command(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Commands::Generate {
            n_tx,
            key_space,
            conflict_ratio,
            cold_ratio,
            seed,
            output,
        } => handle_generate(n_tx, key_space, conflict_ratio, cold_ratio, seed, &output),

        Commands::Execute { input, mode } => handle_execute(&input, &mode),

        Commands::Benchmark {
            input,
            preset,
            output,
        } => handle_benchmark(input, preset, &output),
    }
}

fn handle_generate(
    n_tx: usize,
    key_space: usize,
    conflict_ratio: f64,
    cold_ratio: f64,
    seed: u64,
    output: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let generator = BlockGenerator::new(n_tx, key_space, conflict_ratio, cold_ratio, seed);
    let block = generator.generate();

    let json = serde_json::to_string_pretty(&block)?;
    std::fs::write(output, json)?;

    info!(
        "Generated {} transactions to {}",
        block.transactions.len(),
        output
    );
    Ok(())
}

fn handle_execute(input: &str, mode: &str) -> Result<(), Box<dyn std::error::Error>> {
    let json = std::fs::read_to_string(input)?;
    let block: Block = serde_json::from_str(&json)?;
    let storage = MemoryStore::new();

    match mode {
        "serial" => {
            let start = Instant::now();
            let result = execute_serial(&block, storage);
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;

            info!(
                "Serial: {:.2} ms, {} txs, {} gas",
                elapsed,
                result.results.len(),
                result.total_gas
            );
        }

        "parallel" => {
            let scheduler = MIScheduler::new(10000);
            let access_builder = AccessListBuilder::new(Box::new(HeuristicOracle::new()));
            let mut executor = ParallelExecutor::new(scheduler, access_builder, storage);

            let start = Instant::now();
            let result = executor.execute_parallel(&block);
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;

            info!(
                "Parallel: {:.2} ms, {} txs, {} waves",
                elapsed,
                result.results.len(),
                result.waves.len()
            );
        }

        _ => return Err(format!("Unknown mode: {}", mode).into()),
    }

    Ok(())
}

fn handle_benchmark(
    input: Option<String>,
    preset: Option<String>,
    output: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let block = if let Some(input_path) = input {
        let json = std::fs::read_to_string(&input_path)?;
        serde_json::from_str(&json)?
    } else if let Some(preset_name) = preset {
        match preset_name.as_str() {
            "small" => BlockGenerator::small(),
            "medium" => BlockGenerator::medium(),
            "large" => BlockGenerator::large(),
            _ => return Err(format!("Unknown preset: {}", preset_name).into()),
        }
        .generate()
    } else {
        BlockGenerator::medium().generate()
    };

    let storage1 = MemoryStore::new();
    let start = Instant::now();
    let serial_result = execute_serial(&block, storage1);
    let serial_time_ms = start.elapsed().as_secs_f64() * 1000.0;

    let storage2 = MemoryStore::new();
    let scheduler = MIScheduler::new(10000);
    let access_builder = AccessListBuilder::new(Box::new(HeuristicOracle::new()));
    let mut executor = ParallelExecutor::new(scheduler, access_builder, storage2);

    let start = Instant::now();
    let parallel_result = executor.execute_parallel(&block);
    let parallel_time_ms = start.elapsed().as_secs_f64() * 1000.0;

    if !verify_states(&serial_result.storage, &parallel_result.storage) {
        return Err("State verification failed".into());
    }

    let collector = MetricsCollector::new();
    let metrics = collector.collect(
        &block,
        &serial_result,
        serial_time_ms,
        &parallel_result,
        parallel_time_ms,
        executor.access_builder(),
    );

    collector.print_metrics(&metrics);
    collector.export_json(&metrics, output)?;

    Ok(())
}
