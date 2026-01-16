//! TaskDaemon - Ralph Wiggum Loop Orchestrator
//!
//! CLI entry point for launching and managing concurrent loops.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use clap::{CommandFactory, FromArgMatches};
use eyre::{Context, Result};
use tracing::{info, warn};

use taskdaemon::cli::{Cli, Command, DaemonCommand, OutputFormat, generate_after_help};
use taskdaemon::config::Config;
use taskdaemon::coordinator::Coordinator;
use taskdaemon::daemon::DaemonManager;
use taskdaemon::r#loop::LoopTypeLoader;
use taskdaemon::state::StateManager;
use taskdaemon::tui;
use taskdaemon::watcher::{MainWatcher, WatcherConfig};

fn setup_logging(verbose: bool) -> Result<()> {
    // Create log directory
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("taskdaemon")
        .join("logs");

    fs::create_dir_all(&log_dir).context("Failed to create log directory")?;

    // Setup tracing subscriber - write to log file, not stdout/stderr
    let level = if verbose { tracing::Level::DEBUG } else { tracing::Level::INFO };
    let log_file = fs::File::create(log_dir.join("taskdaemon.log")).context("Failed to create log file")?;

    tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_ansi(false)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive(level.into()))
        .init();

    info!("Logging initialized (verbose: {})", verbose);
    Ok(())
}

use taskdaemon::cli::get_log_path;

#[tokio::main]
async fn main() -> Result<()> {
    // Build command with dynamic after_help that shows tool checks and daemon status
    let cmd = Cli::command().after_help(generate_after_help());

    // Parse CLI arguments using the modified command
    let cli = Cli::from_arg_matches(&cmd.get_matches())?;

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
        Some(Command::Daemon { command }) => match command {
            DaemonCommand::Start { foreground } => cmd_start(&config, foreground).await,
            DaemonCommand::Stop => cmd_stop().await,
            DaemonCommand::Status { detailed, format } => cmd_status(detailed, format).await,
        },
        Some(Command::Tui) => cmd_tui(&config).await,
        Some(Command::Repl {
            loop_type,
            task,
            max_iterations,
        }) => cmd_repl(&config, &loop_type, &task, max_iterations).await,
        Some(Command::RunDaemon) => cmd_run_daemon(&config).await,
        Some(Command::ListLoops) => cmd_list_loops(&config).await,
        Some(Command::Metrics { loop_type, format }) => cmd_metrics(loop_type.as_deref(), format).await,
        Some(Command::Logs { follow, lines }) => cmd_logs(follow, lines).await,
        None => {
            // Default: print help with tool status
            print_help_with_status()
        }
    }
}

/// Print help with required tools and daemon status
fn print_help_with_status() -> Result<()> {
    // Print help using clap
    let mut cmd = Cli::command();
    cmd.print_help()?;
    println!();
    println!();

    // Print the after-help content
    print!("{}", generate_after_help());

    Ok(())
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

/// Run a loop interactively (REPL mode)
async fn cmd_repl(config: &Config, loop_type: &str, task: &str, max_iterations: Option<u32>) -> Result<()> {
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

    // Initialize coordinator for inter-loop communication
    let coordinator = Coordinator::new(Default::default());
    let coordinator_tx = coordinator.sender();

    // Spawn coordinator task
    let coord_handle = tokio::spawn(coordinator.run());
    info!("Coordinator started");

    // Initialize and spawn MainWatcher for git main branch monitoring
    let repo_root = std::env::current_dir().context("Failed to get current directory")?;
    let watcher_config = WatcherConfig::default();
    let main_watcher = MainWatcher::new(watcher_config, repo_root, coordinator_tx);

    let watcher_handle = tokio::spawn(async move {
        if let Err(e) = main_watcher.run().await {
            tracing::error!(error = %e, "MainWatcher error");
        }
    });
    info!("MainWatcher started");

    // TODO: Initialize remaining orchestration:
    // - Scheduler for queue management
    // - LoopManager for running loops

    info!("Daemon running. Press Ctrl+C to stop.");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;

    warn!("Shutdown signal received");
    info!("Daemon shutting down...");

    // Cleanup - abort watcher and coordinator tasks
    watcher_handle.abort();
    coord_handle.abort();

    Ok(())
}
