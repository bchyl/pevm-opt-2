use clap::{Parser, Subcommand};
use crate::evm::execute_serial;
use crate::generator::BlockGenerator;
use crate::metrics::MetricsCollector;
use crate::scheduler::{AccessListBuilder, HeuristicOracle, MIScheduler, ParallelExecutor};
use crate::storage::MemoryStore;
use crate::types::Block;
use std::time::Instant;

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
    /// Generate synthetic block
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

    /// Execute block
    Execute {
        #[arg(long)]
        input: String,

        #[arg(long, default_value = "parallel")]
        mode: String, // "serial" | "parallel"
    },

    /// Benchmark block execution
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

        Commands::Benchmark { input, preset, output } => handle_benchmark(input, preset, &output),
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
    println!("Generating synthetic block...");
    println!("  Transactions: {}", n_tx);
    println!("  Key Space:    {}", key_space);
    println!("  Conflict:     {:.1}%", conflict_ratio * 100.0);
    println!("  Cold Ratio:   {:.1}%", cold_ratio * 100.0);
    println!("  Seed:         {}", seed);

    let generator = BlockGenerator::new(n_tx, key_space, conflict_ratio, cold_ratio, seed);
    let block = generator.generate();

    let json = serde_json::to_string_pretty(&block)?;
    std::fs::write(output, json)?;

    println!("\n✅ Generated block with {} transactions → {}", block.transactions.len(), output);
    Ok(())
}

fn handle_execute(input: &str, mode: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Loading block from {}...", input);
    let json = std::fs::read_to_string(input)?;
    let block: Block = serde_json::from_str(&json)?;

    println!("Block loaded: {} transactions", block.transactions.len());

    let storage = MemoryStore::new();

    match mode {
        "serial" => {
            println!("\nExecuting in serial mode...");
            let start = Instant::now();
            let (_, results, total_gas) = execute_serial(&block, storage);
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;

            println!("\n✅ Serial execution complete:");
            println!("  Time:         {:.2} ms", elapsed);
            println!("  Transactions: {}", results.len());
            println!("  Total Gas:    {}", total_gas);
            println!("  Success Rate: {:.1}%", 
                results.iter().filter(|r| r.success).count() as f64 / results.len() as f64 * 100.0);
        }

        "parallel" => {
            println!("\nExecuting in parallel mode...");
            
            let scheduler = MIScheduler::new(1000);
            let access_builder = AccessListBuilder::new(Box::new(HeuristicOracle::new()));
            let mut executor = ParallelExecutor::new(scheduler, access_builder, storage);

            let start = Instant::now();
            let (_, results, total_gas, waves) = executor.execute_parallel(&block);
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;

            println!("\n✅ Parallel execution complete:");
            println!("  Time:         {:.2} ms", elapsed);
            println!("  Transactions: {}", results.len());
            println!("  Total Gas:    {}", total_gas);
            println!("  Waves:        {}", waves.len());
            println!("  Avg Wave Size: {:.1}", block.transactions.len() as f64 / waves.len() as f64);
            println!("  Success Rate: {:.1}%",
                results.iter().filter(|r| r.success).count() as f64 / results.len() as f64 * 100.0);
        }

        _ => {
            return Err(format!("Unknown mode: {}. Use 'serial' or 'parallel'", mode).into());
        }
    }

    Ok(())
}

fn handle_benchmark(
    input: Option<String>,
    preset: Option<String>,
    output: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Running benchmark...\n");

    // Get block
    let block = if let Some(input_path) = input {
        println!("Loading block from {}...", input_path);
        let json = std::fs::read_to_string(&input_path)?;
        serde_json::from_str(&json)?
    } else if let Some(preset_name) = preset {
        println!("Using preset: {}", preset_name);
        let generator = match preset_name.as_str() {
            "small" => BlockGenerator::small(),
            "medium" => BlockGenerator::medium(),
            "large" => BlockGenerator::large(),
            _ => return Err(format!("Unknown preset: {}. Use 'small', 'medium', or 'large'", preset_name).into()),
        };
        generator.generate()
    } else {
        println!("Using default preset: medium");
        BlockGenerator::medium().generate()
    };

    println!("Block: {} transactions\n", block.transactions.len());

    // Serial execution
    println!("🔄 Running serial execution...");
    let storage1 = MemoryStore::new();
    let start = Instant::now();
    let serial_result = execute_serial(&block, storage1);
    let serial_time_ms = start.elapsed().as_secs_f64() * 1000.0;
    println!("   Completed in {:.2} ms", serial_time_ms);

    // Parallel execution
    println!("🔄 Running parallel execution...");
    let storage2 = MemoryStore::new();
    let scheduler = MIScheduler::new(1000);
    let access_builder = AccessListBuilder::new(Box::new(HeuristicOracle::new()));
    let mut executor = ParallelExecutor::new(scheduler, access_builder, storage2);

    let start = Instant::now();
    let parallel_result = executor.execute_parallel(&block);
    let parallel_time_ms = start.elapsed().as_secs_f64() * 1000.0;
    println!("   Completed in {:.2} ms\n", parallel_time_ms);

    // Collect metrics
    let collector = MetricsCollector::new();
    let metrics = collector.collect(
        &block,
        &serial_result,
        serial_time_ms,
        &parallel_result,
        parallel_time_ms,
        executor.access_builder(),
    );

    // Print metrics
    collector.print_metrics(&metrics);

    // Export to JSON
    collector.export_json(&metrics, output)?;
    println!("📊 Metrics exported to {}\n", output);

    Ok(())
}

