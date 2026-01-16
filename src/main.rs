//! TaskDaemon - Ralph Wiggum Loop Orchestrator
//!
//! CLI entry point for launching and managing concurrent loops.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use clap::{CommandFactory, FromArgMatches};
use eyre::{Context, Result};
use tracing::{info, warn};

use std::sync::Arc;

use taskdaemon::cli::{Cli, Command, DaemonCommand, OutputFormat, generate_after_help};
use taskdaemon::config::Config;
use taskdaemon::coordinator::Coordinator;
use taskdaemon::daemon::DaemonManager;
use taskdaemon::llm::{AnthropicClient, LlmClient};
use taskdaemon::r#loop::{IterationResult, LoopEngine, LoopLoader, LoopManager, LoopManagerConfig};
use taskdaemon::scheduler::{Scheduler, SchedulerConfig};
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
        Some(Command::Repl { initial_task }) => cmd_repl_interactive(&config, initial_task).await,
        Some(Command::Run {
            loop_type,
            task,
            max_iterations,
        }) => cmd_run(&config, &loop_type, &task, max_iterations).await,
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

/// Run the interactive REPL
async fn cmd_repl_interactive(config: &Config, initial_task: Option<String>) -> Result<()> {
    taskdaemon::repl::run_interactive(config, initial_task).await
}

/// Run a loop to completion (batch mode)
async fn cmd_run(config: &Config, loop_type: &str, task: &str, max_iterations: Option<u32>) -> Result<()> {
    // Validate API key early
    if std::env::var(&config.llm.api_key_env).is_err() {
        return Err(eyre::eyre!(
            "LLM API key not found. Set the {} environment variable.",
            config.llm.api_key_env
        ));
    }

    // Load loop types
    let loader = LoopLoader::new(&config.loops)?;
    let _loop_def = loader
        .get(loop_type)
        .ok_or_else(|| eyre::eyre!("Unknown loop type: {}", loop_type))?;

    // Get loop config from the loader
    let all_configs = loader.to_configs();
    let mut loop_config = all_configs
        .get(loop_type)
        .cloned()
        .ok_or_else(|| eyre::eyre!("Failed to build config for loop type: {}", loop_type))?;

    // Override max_iterations if specified
    if let Some(max) = max_iterations {
        loop_config.max_iterations = max;
    }

    // Inject task into prompt template context
    loop_config.prompt_template = loop_config.prompt_template.replace("{{task}}", task);

    println!("Running {} loop", loop_type);
    println!("  Task: {}", task);
    println!("  Max iterations: {}", loop_config.max_iterations);
    println!();

    // Use current directory as worktree (REPL runs in place)
    let worktree = std::env::current_dir()?;

    // Create LLM client
    let llm: Arc<dyn LlmClient> =
        Arc::new(AnthropicClient::from_config(&config.llm).context("Failed to create LLM client")?);

    // Create and run engine (no coordinator for REPL mode)
    let exec_id = format!("repl-{}", std::process::id());
    let mut engine = LoopEngine::new(exec_id, loop_config, llm, worktree);

    // Run with progress output
    println!("Starting iterations...\n");

    match engine.run().await? {
        IterationResult::Complete { iterations } => {
            println!("\n✓ Loop completed successfully after {} iterations", iterations);
        }
        IterationResult::Error { message, .. } => {
            println!("\n✗ Loop failed: {}", message);
            std::process::exit(1);
        }
        IterationResult::Interrupted { reason } => {
            println!("\n⚠ Loop interrupted: {}", reason);
        }
        IterationResult::Continue { .. } => {
            // Shouldn't happen, but handle gracefully
            println!("\nLoop finished with continue status");
        }
        IterationResult::RateLimited { retry_after } => {
            println!("\n⚠ Rate limited, retry after {:?}", retry_after);
        }
    }

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
    let loader = LoopLoader::new(&config.loops)?;

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

/// Show metrics from the daemon's TaskStore
async fn cmd_metrics(loop_type: Option<&str>, format: OutputFormat) -> Result<()> {
    let config = Config::load(None)?;
    let store_path = PathBuf::from(&config.storage.taskstore_dir);

    if !store_path.exists() {
        match format {
            OutputFormat::Json => {
                println!(
                    "{}",
                    serde_json::json!({"error": "No TaskStore found. Has the daemon run?"})
                );
            }
            OutputFormat::Text | OutputFormat::Table => {
                println!("No TaskStore found. Has the daemon run?");
            }
        }
        return Ok(());
    }

    let state = StateManager::spawn(&store_path)?;
    let metrics = state.get_metrics().await?;

    // Note: loop_type filter not yet implemented - would need to filter executions by type
    if loop_type.is_some() {
        warn!("Loop type filter not yet implemented for metrics");
    }

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&metrics)?);
        }
        OutputFormat::Text | OutputFormat::Table => {
            println!("TaskDaemon Metrics");
            println!("-----------------");
            println!("Total executions: {}", metrics.total_executions);
            println!("  Running:   {}", metrics.running);
            println!("  Pending:   {}", metrics.pending);
            println!("  Completed: {}", metrics.completed);
            println!("  Failed:    {}", metrics.failed);
            println!("  Paused:    {}", metrics.paused);
            println!("  Stopped:   {}", metrics.stopped);
            println!();
            println!("Total iterations: {}", metrics.total_iterations);
        }
    }

    Ok(())
}

/// Run the daemon main loop
async fn run_daemon(config: &Config) -> Result<()> {
    info!("Daemon starting...");

    // ============================================================
    // EARLY VALIDATION - Fail fast with clear error messages
    // ============================================================

    // Validate LLM API key is set
    if std::env::var(&config.llm.api_key_env).is_err() {
        return Err(eyre::eyre!(
            "LLM API key not found. Set the {} environment variable.",
            config.llm.api_key_env
        ));
    }

    // Validate we're in a git repository
    let repo_root = std::env::current_dir().context("Failed to get current directory")?;
    if !repo_root.join(".git").exists() {
        return Err(eyre::eyre!(
            "Not a git repository: {}. TaskDaemon requires a git repo.",
            repo_root.display()
        ));
    }

    // Ensure worktree directory is creatable
    let worktree_dir = &config.git.worktree_dir;
    if let Err(e) = fs::create_dir_all(worktree_dir) {
        return Err(eyre::eyre!(
            "Cannot create worktree directory {}: {}",
            worktree_dir.display(),
            e
        ));
    }

    info!("Startup validation passed");

    // ============================================================
    // INITIALIZATION
    // ============================================================

    // Initialize components
    let store_path = PathBuf::from(&config.storage.taskstore_dir);
    if !store_path.exists() {
        fs::create_dir_all(&store_path)?;
    }

    let state_manager = StateManager::spawn(&store_path)?;
    info!("StateManager initialized");

    // Load loop types and convert to configs
    let loader = LoopLoader::new(&config.loops)?;
    let loop_configs = loader.to_configs();
    info!(
        "Loaded {} loop types: {:?}",
        loop_configs.len(),
        loop_configs.keys().collect::<Vec<_>>()
    );

    // Initialize coordinator for inter-loop communication (with event persistence)
    let coordinator = Coordinator::with_persistence(Default::default(), &store_path);
    let coordinator_tx = coordinator.sender();

    // Spawn coordinator task
    let coord_handle = tokio::spawn(coordinator.run());
    info!("Coordinator started");

    // Initialize and spawn MainWatcher for git main branch monitoring
    let watcher_config = WatcherConfig::default();
    let main_watcher = MainWatcher::new(watcher_config, repo_root.clone(), coordinator_tx.clone());

    let watcher_handle = tokio::spawn(async move {
        if let Err(e) = main_watcher.run().await {
            tracing::error!(error = %e, "MainWatcher error");
        }
    });
    info!("MainWatcher started");

    // Initialize scheduler for API rate limiting
    let scheduler_config = SchedulerConfig::default();
    let scheduler = Scheduler::new(scheduler_config);
    info!("Scheduler initialized");

    // Create LLM client (reads API key from env var specified in config)
    let llm_client: Arc<dyn LlmClient> =
        Arc::new(AnthropicClient::from_config(&config.llm).context("Failed to create LLM client")?);
    info!("LLM client initialized (model: {})", config.llm.model);

    // Initialize LoopManager for loop orchestration
    let manager_config = LoopManagerConfig {
        max_concurrent_loops: config.concurrency.max_loops as usize,
        poll_interval_secs: 10,
        shutdown_timeout_secs: 60,
        repo_root: repo_root.clone(),
        worktree_dir: config.git.worktree_dir.clone(),
    };

    let mut loop_manager = LoopManager::new(
        manager_config,
        coordinator_tx, // LoopManager gets the sender, not the Coordinator
        scheduler,
        llm_client,
        state_manager.clone(),
        loop_configs,
    );
    info!("LoopManager initialized");

    // Create shutdown channel for LoopManager
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);

    // Spawn LoopManager
    let manager_handle = tokio::spawn(async move {
        if let Err(e) = loop_manager.run(shutdown_rx).await {
            tracing::error!(error = %e, "LoopManager error");
        }
    });
    info!("LoopManager started");

    info!("Daemon running. Press Ctrl+C to stop, SIGHUP to reload config.");

    // Set up signal handlers
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sighup = signal(SignalKind::hangup())?;
        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sigterm = signal(SignalKind::terminate())?;

        loop {
            tokio::select! {
                _ = sighup.recv() => {
                    info!("SIGHUP received - reloading configuration");
                    // Reload loop types (hot-reload)
                    match LoopLoader::new(&config.loops) {
                        Ok(new_loader) => {
                            let new_configs = new_loader.to_configs();
                            info!(
                                "Reloaded {} loop types: {:?}",
                                new_configs.len(),
                                new_configs.keys().collect::<Vec<_>>()
                            );
                            // Note: In a full implementation, this would be sent to LoopManager
                            // via a channel to hot-swap the configs
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to reload loop types");
                        }
                    }
                }
                _ = sigint.recv() => {
                    warn!("SIGINT received");
                    let _ = shutdown_tx.send(()).await;
                    break;
                }
                _ = sigterm.recv() => {
                    warn!("SIGTERM received");
                    let _ = shutdown_tx.send(()).await;
                    break;
                }
            }
        }
    }

    #[cfg(not(unix))]
    {
        // On non-Unix, just wait for Ctrl+C
        tokio::signal::ctrl_c().await?;
        let _ = shutdown_tx.send(()).await;
    }

    info!("Daemon shutting down...");

    // Wait for LoopManager to finish (it handles coordinator shutdown)
    let _ = manager_handle.await;

    // Cleanup - abort watcher task
    watcher_handle.abort();

    // Coordinator was shut down by LoopManager, but abort handle for safety
    coord_handle.abort();

    Ok(())
}
