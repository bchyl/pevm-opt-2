use clap::Parser;
use pevm_opt_2::cli::{handle_command, Cli};
use tracing_subscriber::EnvFilter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .with_thread_ids(false)
        .with_line_number(false)
        .init();

    // Parse CLI arguments
    let cli = Cli::parse();

    // Handle command
    handle_command(cli)?;

    Ok(())
}
