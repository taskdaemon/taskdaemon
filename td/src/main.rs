//! TaskDaemon - Ralph Wiggum Loop Orchestrator
//!
//! CLI entry point for launching and managing concurrent loops.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use clap::{CommandFactory, FromArgMatches};
use eyre::{Context, Result};
use tracing::{debug, info, warn};

use std::sync::Arc;

use taskdaemon::cli::{Cli, Command, DaemonCommand, ExecCommand, OutputFormat, generate_after_help};
use taskdaemon::config::Config;
use taskdaemon::coordinator::Coordinator;
use taskdaemon::daemon::DaemonManager;
use taskdaemon::ipc;
use taskdaemon::llm::{LlmClient, create_client};
use taskdaemon::r#loop::{IterationResult, LoopEngine, LoopLoader, LoopManager, LoopManagerConfig};
use taskdaemon::scheduler::{Scheduler, SchedulerConfig};
use taskdaemon::state::StateManager;
use taskdaemon::tui;
use taskdaemon::watcher::{MainWatcher, WatcherConfig};

fn setup_logging(cli_log_level: Option<&str>, config_log_level: Option<&str>) -> Result<()> {
    // Note: Can't log params here since logging isn't initialized yet
    // Create log directory
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("taskdaemon")
        .join("logs");

    fs::create_dir_all(&log_dir).context("Failed to create log directory")?;

    // Determine log level with priority: CLI --log-level > config file > default (INFO)
    let level_str = cli_log_level.or(config_log_level);
    let level = if let Some(s) = level_str {
        debug!(level_str = %s, "setup_logging: level_str is Some");
        match s.to_uppercase().as_str() {
            "TRACE" => {
                debug!("setup_logging: matched TRACE level");
                tracing::Level::TRACE
            }
            "DEBUG" => {
                debug!("setup_logging: matched DEBUG level");
                tracing::Level::DEBUG
            }
            "INFO" => {
                debug!("setup_logging: matched INFO level");
                tracing::Level::INFO
            }
            "WARN" | "WARNING" => {
                debug!("setup_logging: matched WARN level");
                tracing::Level::WARN
            }
            "ERROR" => {
                debug!("setup_logging: matched ERROR level");
                tracing::Level::ERROR
            }
            _ => {
                debug!(level = %s, "setup_logging: unknown level, defaulting to INFO");
                eprintln!("Warning: Unknown log-level '{}', defaulting to INFO", s);
                tracing::Level::INFO
            }
        }
    } else {
        debug!("setup_logging: level_str is None, defaulting to INFO");
        tracing::Level::INFO
    };

    let log_file = fs::File::create(log_dir.join("taskdaemon.log")).context("Failed to create log file")?;

    tracing_subscriber::fmt()
        .with_writer(log_file)
        .with_ansi(false)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive(level.into()))
        .init();

    info!("Logging initialized (level: {:?})", level);
    Ok(())
}

use taskdaemon::cli::get_log_path;

#[tokio::main]
async fn main() -> Result<()> {
    // Build command with dynamic after_help that shows tool checks and daemon status
    let cmd = Cli::command().after_help(generate_after_help());

    // Parse CLI arguments using the modified command
    let cli = Cli::from_arg_matches(&cmd.get_matches())?;

    // Load log level from config file early (before full config load)
    let config_log_level = Config::load_log_level(cli.config.as_ref());

    // Setup logging with priority: CLI > config > INFO default
    setup_logging(cli.log_level.as_deref(), config_log_level.as_deref()).context("Failed to setup logging")?;

    // Load configuration
    let config = Config::load(cli.config.as_ref()).context("Failed to load configuration")?;

    info!("TaskDaemon loaded config: default={}", config.llm.default);

    // Dispatch command
    debug!(command = ?cli.command, "main: dispatching command");
    match cli.command {
        Some(Command::Daemon { command }) => {
            debug!("main: matched Daemon command");
            match command {
                DaemonCommand::Start { foreground } => {
                    debug!(foreground, "main: matched DaemonCommand::Start");
                    cmd_start(&config, foreground).await
                }
                DaemonCommand::Stop => {
                    debug!("main: matched DaemonCommand::Stop");
                    cmd_stop().await
                }
                DaemonCommand::Status { detailed, format } => {
                    debug!(detailed, ?format, "main: matched DaemonCommand::Status");
                    cmd_status(detailed, format).await
                }
                DaemonCommand::Ping => {
                    debug!("main: matched DaemonCommand::Ping");
                    cmd_ping().await
                }
            }
        }
        Some(Command::Run {
            loop_type,
            task,
            max_iterations,
        }) => {
            debug!(%loop_type, %task, ?max_iterations, "main: matched Run command");
            cmd_run(&config, &loop_type, &task, max_iterations).await
        }
        Some(Command::RunDaemon) => {
            debug!("main: matched RunDaemon command");
            cmd_run_daemon(&config).await
        }
        Some(Command::Loops) => {
            debug!("main: matched Loops command");
            cmd_list_loops(&config).await
        }
        Some(Command::Metrics { loop_type, format }) => {
            debug!(?loop_type, ?format, "main: matched Metrics command");
            cmd_metrics(loop_type.as_deref(), format).await
        }
        Some(Command::Logs { follow, lines }) => {
            debug!(follow, lines, "main: matched Logs command");
            cmd_logs(follow, lines).await
        }
        Some(Command::Exec { command }) => {
            debug!(?command, "main: matched Exec command");
            cmd_exec(&config, command).await
        }
        None => {
            debug!("main: no command specified, launching TUI");
            // Default: launch TUI with REPL view
            cmd_tui(&config).await
        }
    }
}

/// Start the daemon
async fn cmd_start(config: &Config, foreground: bool) -> Result<()> {
    debug!(foreground, "cmd_start: called");
    let daemon = DaemonManager::new();

    if daemon.is_running() {
        debug!(pid = ?daemon.running_pid(), "cmd_start: daemon already running");
        if let Some(pid) = daemon.running_pid() {
            println!("TaskDaemon is already running (PID: {})", pid);
        } else {
            println!("TaskDaemon is already running");
        }
        return Ok(());
    }

    if foreground {
        debug!("cmd_start: starting in foreground mode");
        println!("Starting TaskDaemon in foreground mode...");
        run_daemon(config).await
    } else {
        debug!("cmd_start: starting in background mode");
        let pid = daemon.start()?;
        println!("TaskDaemon started (PID: {})", pid);
        Ok(())
    }
}

/// Stop the daemon
///
/// Tries IPC shutdown first for graceful stop, falls back to SIGTERM if IPC fails.
async fn cmd_stop() -> Result<()> {
    debug!("cmd_stop: called");
    let daemon = DaemonManager::new();

    if !daemon.is_running() {
        debug!("cmd_stop: daemon is not running");
        println!("TaskDaemon is not running");
        return Ok(());
    }

    let pid = daemon.running_pid();

    // Try graceful IPC shutdown first
    let client = ipc::DaemonClient::new();
    if client.socket_exists() {
        debug!("cmd_stop: trying IPC shutdown");
        match client.shutdown().await {
            Ok(()) => {
                debug!("cmd_stop: IPC shutdown acknowledged");
                // Wait for process to exit
                let mut attempts = 0;
                while daemon.is_running() && attempts < 50 {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    attempts += 1;
                }
                if !daemon.is_running() {
                    if let Some(pid) = pid {
                        println!("TaskDaemon stopped gracefully via IPC (was PID: {})", pid);
                    } else {
                        println!("TaskDaemon stopped gracefully via IPC");
                    }
                    return Ok(());
                }
                debug!("cmd_stop: IPC shutdown timed out, falling back to SIGTERM");
            }
            Err(e) => {
                debug!(error = %e, "cmd_stop: IPC shutdown failed, falling back to SIGTERM");
            }
        }
    }

    // Fall back to SIGTERM
    debug!("cmd_stop: using SIGTERM");
    daemon.stop()?;
    if let Some(pid) = pid {
        println!("TaskDaemon stopped (was PID: {})", pid);
    } else {
        println!("TaskDaemon stopped");
    }
    Ok(())
}

/// Ping the daemon via IPC to check if it's alive and responsive
async fn cmd_ping() -> Result<()> {
    debug!("cmd_ping: called");

    // First check if daemon is running via PID file
    let daemon = DaemonManager::new();
    if !daemon.is_running() {
        debug!("cmd_ping: daemon is not running (no PID)");
        println!("TaskDaemon is not running");
        return Ok(());
    }

    // Try to ping via IPC
    let client = ipc::DaemonClient::new();
    if !client.socket_exists() {
        debug!("cmd_ping: socket does not exist");
        println!("Daemon PID file exists but IPC socket not found");
        println!("The daemon may be starting up or in an inconsistent state");
        return Ok(());
    }

    match client.ping().await {
        Ok(version) => {
            debug!(%version, "cmd_ping: pong received");
            println!("Daemon is alive and responsive");
            println!("Version: {}", version);
        }
        Err(e) => {
            debug!(error = %e, "cmd_ping: ping failed");
            println!("Daemon PID file exists but not responding to IPC");
            println!("Error: {}", e);
            println!("The daemon may be hung or the IPC socket may be stale");
        }
    }

    Ok(())
}

/// Show daemon status
async fn cmd_status(detailed: bool, format: OutputFormat) -> Result<()> {
    debug!(detailed, ?format, "cmd_status: called");
    let daemon = DaemonManager::new();
    let status = daemon.status();

    match format {
        OutputFormat::Json => {
            debug!("cmd_status: format is Json");
            let json = serde_json::json!({
                "running": status.running,
                "pid": status.pid,
                "pid_file": status.pid_file.to_string_lossy()
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
        OutputFormat::Text | OutputFormat::Table => {
            debug!("cmd_status: format is Text or Table");
            println!("TaskDaemon Status");
            println!("-----------------");
            if status.running {
                debug!("cmd_status: daemon is running");
                println!("Status: running");
                if let Some(pid) = status.pid {
                    println!("PID: {}", pid);
                }
            } else {
                debug!("cmd_status: daemon is stopped");
                println!("Status: stopped");
            }
            println!("PID file: {}", status.pid_file.display());

            if detailed && status.running {
                debug!("cmd_status: detailed view requested");
                println!();
                println!("Detailed metrics not available (daemon IPC not implemented)");
                // TODO: Connect to daemon via IPC for detailed metrics
            }
        }
    }

    Ok(())
}

/// Launch the TUI with REPL as default view
async fn cmd_tui(config: &Config) -> Result<()> {
    debug!("cmd_tui: called");

    // Auto-start daemon if not running, or restart if version mismatch
    let daemon = DaemonManager::new();
    if daemon.is_running() {
        if !daemon.version_matches() {
            let daemon_version = daemon.read_version().unwrap_or_else(|| "unknown".to_string());
            info!(
                daemon_version,
                cli_version = taskdaemon::daemon::VERSION,
                "cmd_tui: version mismatch, restarting daemon"
            );
            if let Err(e) = daemon.stop() {
                warn!(error = %e, "cmd_tui: failed to stop old daemon");
            }
            // Start new daemon
            match daemon.start() {
                Ok(pid) => {
                    info!(
                        pid,
                        version = taskdaemon::daemon::VERSION,
                        "cmd_tui: restarted daemon with new version"
                    );
                }
                Err(e) => {
                    warn!(error = %e, "cmd_tui: failed to restart daemon");
                }
            }
        } else {
            debug!(pid = ?daemon.running_pid(), "cmd_tui: daemon already running with matching version");
        }
    } else {
        debug!("cmd_tui: daemon not running, starting it");
        match daemon.start() {
            Ok(pid) => {
                info!(
                    pid,
                    version = taskdaemon::daemon::VERSION,
                    "cmd_tui: started daemon automatically"
                );
            }
            Err(e) => {
                warn!(error = %e, "cmd_tui: failed to start daemon, loops won't run");
            }
        }
    }

    // Initialize StateManager with store path
    let store_path = PathBuf::from(&config.storage.taskstore_dir);

    // Ensure store directory exists
    if !store_path.exists() {
        debug!(?store_path, "cmd_tui: creating TaskStore directory");
        fs::create_dir_all(&store_path).context("Failed to create TaskStore directory")?;
    } else {
        debug!(?store_path, "cmd_tui: TaskStore directory exists");
    }

    let state_manager = StateManager::spawn(&store_path).context("Failed to spawn StateManager")?;

    // Resolve LLM config and create client if API key is available
    let (llm_client, max_tokens): (Option<std::sync::Arc<dyn taskdaemon::LlmClient>>, u32) = match config.llm.resolve()
    {
        Ok(resolved) => {
            let max_tokens = resolved.max_tokens;
            match create_client(&config.llm) {
                Ok(client) => {
                    debug!(
                        default = %config.llm.default,
                        max_tokens,
                        "cmd_tui: LLM client created successfully"
                    );
                    (Some(client), max_tokens)
                }
                Err(e) => {
                    debug!(error = %e, "cmd_tui: failed to create LLM client");
                    info!("LLM client not available ({}). REPL will show an error when used.", e);
                    (None, max_tokens)
                }
            }
        }
        Err(e) => {
            debug!(error = %e, "cmd_tui: failed to resolve LLM config");
            info!("LLM config invalid ({}). REPL will show an error when used.", e);
            (None, 16384) // Fallback default
        }
    };

    // Run TUI with LLM client
    debug!("cmd_tui: launching TUI");
    tui::run_with_state_and_llm(state_manager, llm_client, max_tokens, config.debug.clone()).await
}

/// Show logs
async fn cmd_logs(follow: bool, lines: usize) -> Result<()> {
    debug!(follow, lines, "cmd_logs: called");
    let log_path = get_log_path();

    if !log_path.exists() {
        debug!(?log_path, "cmd_logs: log file does not exist");
        println!("No log file found at: {}", log_path.display());
        println!("The daemon may not have been started yet.");
        return Ok(());
    }

    if follow {
        debug!(?log_path, "cmd_logs: following log file");
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
        debug!(?log_path, lines, "cmd_logs: reading last N lines");
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

/// Run a loop to completion (batch mode)
async fn cmd_run(config: &Config, loop_type: &str, task: &str, max_iterations: Option<u32>) -> Result<()> {
    debug!(%loop_type, %task, ?max_iterations, "cmd_run: called");
    // Validate API key early by resolving the config
    config
        .llm
        .resolve()
        .and_then(|r| r.get_api_key())
        .context("LLM API key not found. Check api-key-env or api-key-file in your config.")?;
    debug!("cmd_run: API key found");

    // Load loop types
    let loader = LoopLoader::new(&config.loops)?;
    let _loop_def = loader
        .get(loop_type)
        .ok_or_else(|| eyre::eyre!("Unknown loop type: {}", loop_type))?;
    debug!(%loop_type, "cmd_run: loop type found");

    // Get loop config from the loader
    let all_configs = loader.to_configs();
    let mut loop_config = all_configs
        .get(loop_type)
        .cloned()
        .ok_or_else(|| eyre::eyre!("Failed to build config for loop type: {}", loop_type))?;

    // Override max_iterations if specified
    if let Some(max) = max_iterations {
        debug!(max, "cmd_run: overriding max_iterations");
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
    debug!(?worktree, "cmd_run: using current directory as worktree");

    // Create LLM client
    let llm: Arc<dyn LlmClient> = create_client(&config.llm).context("Failed to create LLM client")?;
    debug!("cmd_run: LLM client created");

    // Create and run engine (no coordinator for REPL mode)
    let exec_id = format!("repl-{}", std::process::id());
    let mut engine = LoopEngine::new(exec_id.clone(), loop_config, llm, worktree);
    debug!(%exec_id, "cmd_run: engine created");

    // Run with progress output
    println!("Starting iterations...\n");

    let result = engine.run().await?;
    debug!(?result, "cmd_run: engine finished");
    match result {
        IterationResult::Complete { iterations } => {
            debug!(iterations, "cmd_run: loop completed");
            println!("\n✓ Loop completed successfully after {} iterations", iterations);
        }
        IterationResult::Error { message, .. } => {
            debug!(%message, "cmd_run: loop failed");
            println!("\n✗ Loop failed: {}", message);
            std::process::exit(1);
        }
        IterationResult::Interrupted { reason } => {
            debug!(%reason, "cmd_run: loop interrupted");
            println!("\n⚠ Loop interrupted: {}", reason);
        }
        IterationResult::Continue { .. } => {
            debug!("cmd_run: loop finished with continue status");
            // Shouldn't happen, but handle gracefully
            println!("\nLoop finished with continue status");
        }
        IterationResult::RateLimited { retry_after } => {
            debug!(?retry_after, "cmd_run: rate limited");
            println!("\n⚠ Rate limited, retry after {:?}", retry_after);
        }
    }

    Ok(())
}

/// Run as the daemon process (internal command)
async fn cmd_run_daemon(config: &Config) -> Result<()> {
    debug!("cmd_run_daemon: called");
    let daemon = DaemonManager::new();
    daemon.register_self()?;
    debug!("cmd_run_daemon: daemon registered, starting run_daemon");

    run_daemon(config).await
}

/// List available loop types
async fn cmd_list_loops(config: &Config) -> Result<()> {
    debug!("cmd_list_loops: called");
    let loader = LoopLoader::new(&config.loops)?;

    let loop_types: Vec<String> = loader.names().map(|s| s.to_string()).collect();
    debug!(count = loop_types.len(), "cmd_list_loops: found loop types");

    if loop_types.is_empty() {
        debug!("cmd_list_loops: no loop types found");
        println!("No loop types found.");
        println!("Loop type paths searched:");
        for path in &config.loops.paths {
            println!("  - {}", path);
        }
        return Ok(());
    }

    debug!(?loop_types, "cmd_list_loops: listing loop types");
    println!("Available loop types:");
    println!();

    for name in &loop_types {
        if let Some(def) = loader.get(name) {
            debug!(%name, "cmd_list_loops: printing loop type");
            println!("  {}", name);
            println!("    {}", def.description);
            if let Some(extends) = &def.extends {
                debug!(%extends, "cmd_list_loops: loop extends another");
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
    debug!(?loop_type, ?format, "cmd_metrics: called");
    let config = Config::load(None)?;
    let store_path = PathBuf::from(&config.storage.taskstore_dir);

    if !store_path.exists() {
        debug!(?store_path, "cmd_metrics: TaskStore does not exist");
        match format {
            OutputFormat::Json => {
                debug!("cmd_metrics: outputting JSON error");
                println!(
                    "{}",
                    serde_json::json!({"error": "No TaskStore found. Has the daemon run?"})
                );
            }
            OutputFormat::Text | OutputFormat::Table => {
                debug!("cmd_metrics: outputting text error");
                println!("No TaskStore found. Has the daemon run?");
            }
        }
        return Ok(());
    }

    debug!(?store_path, "cmd_metrics: TaskStore exists");
    let state = StateManager::spawn(&store_path)?;
    let metrics = state.get_metrics().await?;
    debug!(?metrics, "cmd_metrics: got metrics");

    // Note: loop_type filter not yet implemented - would need to filter executions by type
    if loop_type.is_some() {
        debug!(
            ?loop_type,
            "cmd_metrics: loop_type filter requested but not implemented"
        );
        warn!("Loop type filter not yet implemented for metrics");
    }

    match format {
        OutputFormat::Json => {
            debug!("cmd_metrics: outputting JSON");
            println!("{}", serde_json::to_string_pretty(&metrics)?);
        }
        OutputFormat::Text | OutputFormat::Table => {
            debug!("cmd_metrics: outputting text/table");
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

/// Handle execution management commands
async fn cmd_exec(config: &Config, command: ExecCommand) -> Result<()> {
    debug!(?command, "cmd_exec: called");
    use taskdaemon::domain::LoopExecutionStatus;

    let store_path = PathBuf::from(&config.storage.taskstore_dir);
    if !store_path.exists() {
        debug!(?store_path, "cmd_exec: TaskStore does not exist");
        eprintln!(
            "No TaskStore found at {:?}. Run the TUI first to create plans.",
            store_path
        );
        return Ok(());
    }

    debug!(?store_path, "cmd_exec: TaskStore exists");
    let state = StateManager::spawn(&store_path)?;

    match command {
        ExecCommand::List { status } => {
            debug!(?status, "cmd_exec: matched List command");
            let executions = state.list_executions(status.clone(), None).await?;
            if executions.is_empty() {
                debug!("cmd_exec: no executions found");
                println!(
                    "No executions found{}",
                    status.map(|s| format!(" with status '{}'", s)).unwrap_or_default()
                );
            } else {
                debug!(count = executions.len(), "cmd_exec: found executions");
                println!("{:<50} {:<10} {:<20}", "ID", "STATUS", "TYPE");
                println!("{}", "-".repeat(80));
                for exec in executions {
                    println!("{:<50} {:<10} {:<20}", exec.id, exec.status, exec.loop_type);
                }
            }
        }
        ExecCommand::Start { id } => {
            debug!(%id, "cmd_exec: matched Start command");
            match state.start_draft(&id).await {
                Ok(()) => {
                    debug!(%id, "cmd_exec: start succeeded");
                    println!("Started execution '{}' (draft -> pending)", id);
                }
                Err(e) => {
                    debug!(%id, error = %e, "cmd_exec: start failed");
                    eprintln!("Failed to start: {}", e);
                }
            }
        }
        ExecCommand::Pause { id } => {
            debug!(%id, "cmd_exec: matched Pause command");
            match state.pause_execution(&id).await {
                Ok(()) => {
                    debug!(%id, "cmd_exec: pause succeeded");
                    println!("Paused execution '{}' (running -> paused)", id);
                }
                Err(e) => {
                    debug!(%id, error = %e, "cmd_exec: pause failed");
                    eprintln!("Failed to pause: {}", e);
                }
            }
        }
        ExecCommand::Resume { id } => {
            debug!(%id, "cmd_exec: matched Resume command");
            match state.resume_execution(&id).await {
                Ok(()) => {
                    debug!(%id, "cmd_exec: resume succeeded");
                    println!("Resumed execution '{}' (paused -> running)", id);
                }
                Err(e) => {
                    debug!(%id, error = %e, "cmd_exec: resume failed");
                    eprintln!("Failed to resume: {}", e);
                }
            }
        }
        ExecCommand::Status { id, status } => {
            debug!(%id, %status, "cmd_exec: matched Status command");
            let new_status = match status.to_lowercase().as_str() {
                "draft" => {
                    debug!("cmd_exec: matched draft status");
                    LoopExecutionStatus::Draft
                }
                "pending" => {
                    debug!("cmd_exec: matched pending status");
                    LoopExecutionStatus::Pending
                }
                "running" => {
                    debug!("cmd_exec: matched running status");
                    LoopExecutionStatus::Running
                }
                "paused" => {
                    debug!("cmd_exec: matched paused status");
                    LoopExecutionStatus::Paused
                }
                "complete" => {
                    debug!("cmd_exec: matched complete status");
                    LoopExecutionStatus::Complete
                }
                "failed" => {
                    debug!("cmd_exec: matched failed status");
                    LoopExecutionStatus::Failed
                }
                "stopped" => {
                    debug!("cmd_exec: matched stopped status");
                    LoopExecutionStatus::Stopped
                }
                _ => {
                    debug!(%status, "cmd_exec: invalid status");
                    eprintln!(
                        "Invalid status '{}'. Valid: draft, pending, running, paused, complete, failed, stopped",
                        status
                    );
                    return Ok(());
                }
            };

            match state.get_execution(&id).await? {
                Some(mut exec) => {
                    debug!(%id, ?new_status, "cmd_exec: found execution, updating status");
                    let old_status = exec.status;
                    exec.set_status(new_status);
                    state.update_execution(exec).await?;
                    println!("Set execution '{}' status: {} -> {}", id, old_status, new_status);
                }
                None => {
                    debug!(%id, "cmd_exec: execution not found");
                    eprintln!("Execution '{}' not found", id);
                }
            }
        }
    }

    Ok(())
}

/// Run the daemon main loop
async fn run_daemon(config: &Config) -> Result<()> {
    debug!("run_daemon: called");
    info!("Daemon starting...");

    // ============================================================
    // EARLY VALIDATION - Fail fast with clear error messages
    // ============================================================

    // Validate LLM API key is set by resolving the config
    config
        .llm
        .resolve()
        .and_then(|r| r.get_api_key())
        .context("LLM API key not found. Check api-key-env or api-key-file in your config.")?;
    debug!("run_daemon: API key found");

    // Validate we're in a git repository
    let repo_root = std::env::current_dir().context("Failed to get current directory")?;
    if !repo_root.join(".git").exists() {
        debug!(?repo_root, "run_daemon: not a git repository");
        return Err(eyre::eyre!(
            "Not a git repository: {}. TaskDaemon requires a git repo.",
            repo_root.display()
        ));
    }
    debug!(?repo_root, "run_daemon: git repository found");

    // Ensure worktree directory is creatable
    let worktree_dir = &config.git.worktree_dir;
    if let Err(e) = fs::create_dir_all(worktree_dir) {
        debug!(?worktree_dir, error = %e, "run_daemon: cannot create worktree directory");
        return Err(eyre::eyre!(
            "Cannot create worktree directory {}: {}",
            worktree_dir.display(),
            e
        ));
    }
    debug!(?worktree_dir, "run_daemon: worktree directory created/exists");

    info!("Startup validation passed");

    // ============================================================
    // INITIALIZATION
    // ============================================================

    // Initialize components
    let store_path = PathBuf::from(&config.storage.taskstore_dir);
    if !store_path.exists() {
        debug!(?store_path, "run_daemon: creating store directory");
        fs::create_dir_all(&store_path)?;
    } else {
        debug!(?store_path, "run_daemon: store directory exists");
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
    let type_loader = std::sync::Arc::new(std::sync::RwLock::new(loader));

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

    // Create LLM client (reads API key from env var or file specified in config)
    let llm_client: Arc<dyn LlmClient> = create_client(&config.llm).context("Failed to create LLM client")?;
    info!("LLM client initialized ({})", config.llm.default);

    // Initialize LoopManager for loop orchestration
    // poll_interval_secs is 60s (fallback) since event-driven pickup handles immediate work
    let manager_config = LoopManagerConfig {
        max_concurrent_loops: config.concurrency.max_loops as usize,
        poll_interval_secs: 60,
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
        type_loader,
    );
    info!("LoopManager initialized");

    // Create IPC listener for cross-process wake-up
    let (ipc_listener, socket_path) = ipc::create_listener()?;
    info!(?socket_path, "IPC socket listening");

    // Create shutdown channel for LoopManager
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);

    // Spawn LoopManager with IPC listener
    let manager_handle = tokio::spawn(async move {
        if let Err(e) = loop_manager.run(shutdown_rx, Some(ipc_listener)).await {
            tracing::error!(error = %e, "LoopManager error");
        }
    });
    info!("LoopManager started");

    info!("Daemon running. Press Ctrl+C to stop, SIGHUP to reload config.");

    // Set up signal handlers
    debug!("run_daemon: setting up signal handlers");
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sighup = signal(SignalKind::hangup())?;
        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sigterm = signal(SignalKind::terminate())?;

        loop {
            tokio::select! {
                _ = sighup.recv() => {
                    debug!("run_daemon: SIGHUP received");
                    info!("SIGHUP received - reloading configuration");
                    // Reload loop types (hot-reload)
                    match LoopLoader::new(&config.loops) {
                        Ok(new_loader) => {
                            debug!("run_daemon: loop types reloaded successfully");
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
                            debug!(error = %e, "run_daemon: failed to reload loop types");
                            tracing::error!(error = %e, "Failed to reload loop types");
                        }
                    }
                }
                _ = sigint.recv() => {
                    debug!("run_daemon: SIGINT received, initiating shutdown");
                    warn!("SIGINT received");
                    let _ = shutdown_tx.send(()).await;
                    break;
                }
                _ = sigterm.recv() => {
                    debug!("run_daemon: SIGTERM received, initiating shutdown");
                    warn!("SIGTERM received");
                    let _ = shutdown_tx.send(()).await;
                    break;
                }
            }
        }
    }

    #[cfg(not(unix))]
    {
        debug!("run_daemon: waiting for ctrl_c (non-Unix)");
        // On non-Unix, just wait for Ctrl+C
        tokio::signal::ctrl_c().await?;
        debug!("run_daemon: ctrl_c received, initiating shutdown");
        let _ = shutdown_tx.send(()).await;
    }

    info!("Daemon shutting down...");
    debug!("run_daemon: waiting for LoopManager to finish");

    // Wait for LoopManager to finish (it handles coordinator shutdown)
    let _ = manager_handle.await;
    debug!("run_daemon: LoopManager finished");

    // Cleanup - remove IPC socket
    debug!("run_daemon: cleaning up IPC socket");
    ipc::cleanup_socket(&socket_path);

    // Cleanup - abort watcher task
    debug!("run_daemon: aborting watcher task");
    watcher_handle.abort();

    // Coordinator was shut down by LoopManager, but abort handle for safety
    debug!("run_daemon: aborting coordinator handle");
    coord_handle.abort();

    debug!("run_daemon: shutdown complete");
    Ok(())
}
