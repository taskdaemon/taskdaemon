//! TaskDaemon - Ralph Wiggum Loop Orchestrator
//!
//! CLI entry point for launching and managing concurrent loops.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use clap::Parser;
use eyre::{Context, Result};
use tracing::{info, warn};

use taskdaemon::cli::{Cli, Command, OutputFormat};
use taskdaemon::config::Config;
use taskdaemon::daemon::DaemonManager;
use taskdaemon::r#loop::LoopTypeLoader;
use taskdaemon::state::StateManager;
use taskdaemon::tui;

fn setup_logging(verbose: bool) -> Result<()> {
    // Create log directory
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("taskdaemon")
        .join("logs");

    fs::create_dir_all(&log_dir).context("Failed to create log directory")?;

    // Setup tracing subscriber
    let level = if verbose { tracing::Level::DEBUG } else { tracing::Level::INFO };

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive(level.into()))
        .init();

    info!("Logging initialized (verbose: {})", verbose);
    Ok(())
}

fn get_log_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("taskdaemon")
        .join("logs")
        .join("taskdaemon.log")
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let cli = Cli::parse();

    // Setup logging
    setup_logging(cli.verbose).context("Failed to setup logging")?;

    // Load configuration
    let config = Config::load(cli.config.as_ref()).context("Failed to load configuration")?;

    info!(
        "TaskDaemon loaded config: provider={}, model={}",
        config.llm.provider, config.llm.model
    );

    // Dispatch command
    match cli.command {
        Some(Command::Start { foreground }) => cmd_start(&config, foreground).await,
        Some(Command::Stop) => cmd_stop().await,
        Some(Command::Status { detailed, format }) => cmd_status(detailed, format).await,
        Some(Command::Tui) => cmd_tui(&config).await,
        Some(Command::Logs { follow, lines }) => cmd_logs(follow, lines).await,
        Some(Command::Run {
            loop_type,
            task,
            max_iterations,
        }) => cmd_run(&config, &loop_type, &task, max_iterations).await,
        Some(Command::RunDaemon) => cmd_run_daemon(&config).await,
        Some(Command::ListLoops) => cmd_list_loops(&config).await,
        Some(Command::Metrics { loop_type, format }) => cmd_metrics(loop_type.as_deref(), format).await,
        None => {
            // Default: show status
            cmd_status(false, OutputFormat::Text).await
        }
    }
}

/// Start the daemon
async fn cmd_start(config: &Config, foreground: bool) -> Result<()> {
    let daemon = DaemonManager::new();

    if daemon.is_running() {
        println!("TaskDaemon is already running (PID: {})", daemon.running_pid().unwrap());
        return Ok(());
    }

    if foreground {
        println!("Starting TaskDaemon in foreground mode...");
        run_daemon(config).await
    } else {
        let pid = daemon.start()?;
        println!("TaskDaemon started (PID: {})", pid);
        Ok(())
    }
}

/// Stop the daemon
async fn cmd_stop() -> Result<()> {
    let daemon = DaemonManager::new();

    if !daemon.is_running() {
        println!("TaskDaemon is not running");
        return Ok(());
    }

    let pid = daemon.running_pid().unwrap();
    daemon.stop()?;
    println!("TaskDaemon stopped (was PID: {})", pid);
    Ok(())
}

/// Show daemon status
async fn cmd_status(detailed: bool, format: OutputFormat) -> Result<()> {
    let daemon = DaemonManager::new();
    let status = daemon.status();

    match format {
        OutputFormat::Json => {
            let json = serde_json::json!({
                "running": status.running,
                "pid": status.pid,
                "pid_file": status.pid_file.to_string_lossy()
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
        OutputFormat::Text | OutputFormat::Table => {
            println!("TaskDaemon Status");
            println!("-----------------");
            if status.running {
                println!("Status: running");
                println!("PID: {}", status.pid.unwrap());
            } else {
                println!("Status: stopped");
            }
            println!("PID file: {}", status.pid_file.display());

            if detailed && status.running {
                println!();
                println!("Detailed metrics not available (daemon IPC not implemented)");
                // TODO: Connect to daemon via IPC for detailed metrics
            }
        }
    }

    Ok(())
}

/// Launch the TUI
async fn cmd_tui(config: &Config) -> Result<()> {
    // Initialize StateManager with store path
    let store_path = PathBuf::from(&config.storage.taskstore_dir);

    // Ensure store directory exists
    if !store_path.exists() {
        fs::create_dir_all(&store_path).context("Failed to create TaskStore directory")?;
    }

    let state_manager = StateManager::spawn(&store_path).context("Failed to spawn StateManager")?;

    // Run TUI
    tui::run_with_state(state_manager).await
}

/// Show logs
async fn cmd_logs(follow: bool, lines: usize) -> Result<()> {
    let log_path = get_log_path();

    if !log_path.exists() {
        println!("No log file found at: {}", log_path.display());
        println!("The daemon may not have been started yet.");
        return Ok(());
    }

    if follow {
        println!("Following log file: {} (Ctrl+C to stop)", log_path.display());
        println!();

        // Use tail -f for following
        let mut child = std::process::Command::new("tail")
            .args(["-f", "-n", &lines.to_string()])
            .arg(&log_path)
            .spawn()
            .context("Failed to run tail -f")?;

        child.wait()?;
    } else {
        // Read last N lines
        let file = fs::File::open(&log_path).context("Failed to open log file")?;
        let reader = BufReader::new(file);
        let all_lines: Vec<String> = reader.lines().map_while(Result::ok).collect();

        let start = if all_lines.len() > lines { all_lines.len() - lines } else { 0 };

        for line in &all_lines[start..] {
            println!("{}", line);
        }
    }

    Ok(())
}

/// Run a single loop (for development/testing)
async fn cmd_run(config: &Config, loop_type: &str, task: &str, max_iterations: Option<u32>) -> Result<()> {
    // Load loop types
    let loader = LoopTypeLoader::new(&config.loops)?;

    let loop_def = loader
        .get(loop_type)
        .ok_or_else(|| eyre::eyre!("Unknown loop type: {}", loop_type))?;

    println!("Running {} loop", loop_type);
    println!("  Task: {}", task);
    println!("  Description: {}", loop_def.description);
    if let Some(max) = max_iterations {
        println!("  Max iterations: {}", max);
    } else {
        println!("  Max iterations: {} (default)", loop_def.max_iterations);
    }
    println!();

    // TODO: Full loop execution implementation
    // This would create a LoopExecution, set up the worktree, and run iterations
    println!("Full loop execution not yet implemented.");
    println!("Use the daemon for actual loop orchestration.");

    Ok(())
}

/// Run as the daemon process (internal command)
async fn cmd_run_daemon(config: &Config) -> Result<()> {
    let daemon = DaemonManager::new();
    daemon.register_self()?;

    run_daemon(config).await
}

/// List available loop types
async fn cmd_list_loops(config: &Config) -> Result<()> {
    let loader = LoopTypeLoader::new(&config.loops)?;

    let loop_types: Vec<String> = loader.names().map(|s| s.to_string()).collect();

    if loop_types.is_empty() {
        println!("No loop types found.");
        println!("Loop type paths searched:");
        for path in &config.loops.paths {
            println!("  - {}", path);
        }
        return Ok(());
    }

    println!("Available loop types:");
    println!();

    for name in &loop_types {
        if let Some(def) = loader.get(name) {
            println!("  {}", name);
            println!("    {}", def.description);
            if let Some(extends) = &def.extends {
                println!("    Extends: {}", extends);
            }
            println!("    Max iterations: {}", def.max_iterations);
            println!();
        }
    }

    Ok(())
}

/// Show metrics
async fn cmd_metrics(loop_type: Option<&str>, format: OutputFormat) -> Result<()> {
    // TODO: Connect to daemon or TaskStore for actual metrics

    let metrics = serde_json::json!({
        "status": "Metrics collection not implemented",
        "filter": loop_type,
        "note": "Run the daemon to collect metrics"
    });

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&metrics)?);
        }
        OutputFormat::Text | OutputFormat::Table => {
            println!("TaskDaemon Metrics");
            println!("-----------------");
            if let Some(lt) = loop_type {
                println!("Filter: {}", lt);
            }
            println!();
            println!("Metrics collection not yet implemented.");
            println!("Run the daemon to start collecting metrics.");
        }
    }

    Ok(())
}

/// Run the daemon main loop
async fn run_daemon(config: &Config) -> Result<()> {
    info!("Daemon starting...");

    // Initialize components
    let store_path = PathBuf::from(&config.storage.taskstore_dir);
    if !store_path.exists() {
        fs::create_dir_all(&store_path)?;
    }

    let _state_manager = StateManager::spawn(&store_path)?;

    // Load loop types
    let loader = LoopTypeLoader::new(&config.loops)?;
    info!("Loaded {} loop types", loader.len());

    // TODO: Initialize full orchestration:
    // - Coordinator for inter-loop communication
    // - Scheduler for queue management
    // - MainWatcher for git changes
    // - LoopManager for running loops

    info!("Daemon running. Press Ctrl+C to stop.");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;

    warn!("Shutdown signal received");
    info!("Daemon shutting down...");

    Ok(())
}
