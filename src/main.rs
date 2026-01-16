//! TaskDaemon - Ralph Wiggum Loop Orchestrator
//!
//! CLI entry point for launching and managing concurrent loops.

// Phase 1 infrastructure - these types are used in later phases when CLI is wired up
#![allow(dead_code)]

use clap::Parser;
use eyre::{Context, Result};
use std::fs;
use std::path::PathBuf;
use tracing::info;

// Use the library crate
use taskdaemon::cli::Cli;
use taskdaemon::config::Config;

fn setup_logging() -> Result<()> {
    // Create log directory
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("taskdaemon")
        .join("logs");

    fs::create_dir_all(&log_dir).context("Failed to create log directory")?;

    // Setup tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    info!("Logging initialized");
    Ok(())
}

fn main() -> Result<()> {
    // Setup logging first
    setup_logging().context("Failed to setup logging")?;

    // Parse CLI arguments
    let cli = Cli::parse();

    // Load configuration
    let config = Config::load(cli.config.as_ref()).context("Failed to load configuration")?;

    info!(
        "TaskDaemon started with config: provider={}, model={}",
        config.llm.provider, config.llm.model
    );

    // TODO: Implement CLI command dispatch in later phases
    // For now, just print config info
    println!("TaskDaemon v{}", env!("CARGO_PKG_VERSION"));
    println!("  LLM: {} ({})", config.llm.provider, config.llm.model);
    println!("  Max loops: {}", config.concurrency.max_loops);
    println!("  Validation: {}", config.validation.command);

    Ok(())
}
