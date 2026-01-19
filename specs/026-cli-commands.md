# Spec: CLI Commands

**ID:** 026-cli-commands
**Status:** Draft
**Dependencies:** [025-terminal-ui, 014-loop-manager]

## Summary

Implement command-line interface (CLI) commands for TaskDaemon including start, stop, tui, status, and new-plan. The CLI should provide a clean, intuitive interface for daemon management and task creation.

## Acceptance Criteria

1. **Core Commands**
   - `taskdaemon start` - Start daemon process
   - `taskdaemon stop` - Stop daemon gracefully
   - `taskdaemon tui` - Launch terminal UI
   - `taskdaemon status` - Show daemon status
   - `taskdaemon new-plan` - Create new plan

2. **Command Features**
   - Argument parsing and validation
   - Help text and documentation
   - Error handling and user feedback
   - Configuration file support

3. **Daemon Control**
   - PID file management
   - Signal handling
   - Graceful shutdown
   - Health checks

4. **User Experience**
   - Clear error messages
   - Progress indicators
   - Colored output
   - Interactive prompts

## Implementation Phases

### Phase 1: CLI Framework
- Command structure setup
- Argument parsing
- Help system
- Basic commands

### Phase 2: Daemon Commands
- Start/stop implementation
- PID file handling
- Signal management
- Status reporting

### Phase 3: Plan Management
- New plan creation
- Plan templates
- Interactive mode
- Validation

### Phase 4: Advanced Features
- Shell completion
- Configuration management
- Debug commands
- Plugin support

## Technical Details

### Module Structure
```
src/cli/
├── mod.rs
├── app.rs         # CLI app definition
├── commands/      # Command implementations
│   ├── start.rs
│   ├── stop.rs
│   ├── tui.rs
│   ├── status.rs
│   └── plan.rs
├── daemon.rs      # Daemon management
├── config.rs      # CLI configuration
└── output.rs      # Output formatting
```

### CLI Application
```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[clap(name = "taskdaemon")]
#[clap(about = "AI-powered task execution daemon", version)]
pub struct Cli {
    /// Configuration file path
    #[clap(short, long, value_name = "FILE", global = true)]
    pub config: Option<PathBuf>,

    /// Verbosity level (-v, -vv, -vvv)
    #[clap(short, long, parse(from_occurrences), global = true)]
    pub verbose: usize,

    /// Output format
    #[clap(long, value_enum, default_value = "human", global = true)]
    pub output: OutputFormat,

    #[clap(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the TaskDaemon service
    Start {
        /// Run in foreground (don't daemonize)
        #[clap(short, long)]
        foreground: bool,

        /// PID file location
        #[clap(long, value_name = "FILE")]
        pid_file: Option<PathBuf>,

        /// Bind address
        #[clap(long, default_value = "127.0.0.1:3000")]
        bind: String,
    },

    /// Stop the TaskDaemon service
    Stop {
        /// PID file location
        #[clap(long, value_name = "FILE")]
        pid_file: Option<PathBuf>,

        /// Force stop (SIGKILL instead of SIGTERM)
        #[clap(short, long)]
        force: bool,

        /// Timeout in seconds
        #[clap(short, long, default_value = "30")]
        timeout: u64,
    },

    /// Launch the terminal user interface
    Tui {
        /// Daemon connection URL
        #[clap(long, default_value = "http://127.0.0.1:3000")]
        url: String,

        /// Auto-reconnect on disconnect
        #[clap(long)]
        auto_reconnect: bool,
    },

    /// Show daemon status
    Status {
        /// Show detailed status
        #[clap(short, long)]
        detailed: bool,

        /// Watch mode (refresh periodically)
        #[clap(short, long)]
        watch: bool,

        /// Watch interval in seconds
        #[clap(long, default_value = "2")]
        interval: u64,
    },

    /// Create a new plan
    #[clap(name = "new-plan")]
    NewPlan {
        /// Plan name
        name: String,

        /// Plan goal/description
        #[clap(short, long)]
        goal: Option<String>,

        /// Use template
        #[clap(short, long)]
        template: Option<String>,

        /// Interactive mode
        #[clap(short, long)]
        interactive: bool,

        /// Start immediately
        #[clap(short, long)]
        start: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Human,
    Json,
    Yaml,
    Table,
}
```

### Start Command
```rust
pub async fn handle_start_command(
    foreground: bool,
    pid_file: Option<PathBuf>,
    bind: String,
    config: &Config,
) -> Result<(), Error> {
    let pid_file = pid_file.unwrap_or_else(|| config.default_pid_file());

    // Check if already running
    if daemon::is_running(&pid_file)? {
        return Err(Error::AlreadyRunning);
    }

    if foreground {
        // Run in foreground
        info!("Starting TaskDaemon in foreground mode...");
        run_daemon(bind, config).await
    } else {
        // Daemonize
        info!("Starting TaskDaemon...");

        let daemonize = Daemonize::new()
            .pid_file(&pid_file)
            .chown_pid_file(true)
            .working_directory(&config.work_dir)
            .umask(0o027);

        match daemonize.start() {
            Ok(_) => {
                // Now running as daemon
                run_daemon(bind, config).await
            }
            Err(e) => {
                error!("Failed to daemonize: {}", e);
                Err(Error::DaemonizeError(e))
            }
        }
    }
}

async fn run_daemon(bind: String, config: &Config) -> Result<(), Error> {
    // Initialize logging
    init_logging(config)?;

    // Create application
    let app = TaskDaemonApp::new(config.clone()).await?;

    // Set up signal handlers
    let shutdown = setup_signal_handlers();

    // Start server
    info!("TaskDaemon listening on {}", bind);

    tokio::select! {
        result = app.run(&bind) => {
            result?;
        }
        _ = shutdown => {
            info!("Received shutdown signal");
            app.shutdown().await?;
        }
    }

    Ok(())
}
```

### Stop Command
```rust
pub async fn handle_stop_command(
    pid_file: Option<PathBuf>,
    force: bool,
    timeout: u64,
    config: &Config,
) -> Result<(), Error> {
    let pid_file = pid_file.unwrap_or_else(|| config.default_pid_file());

    // Read PID
    let pid = daemon::read_pid(&pid_file)?
        .ok_or(Error::NotRunning)?;

    info!("Stopping TaskDaemon (PID: {})...", pid);

    // Send signal
    let signal = if force { Signal::SIGKILL } else { Signal::SIGTERM };
    daemon::send_signal(pid, signal)?;

    // Wait for shutdown
    let start = Instant::now();
    let timeout_duration = Duration::from_secs(timeout);

    while daemon::is_process_running(pid)? {
        if start.elapsed() > timeout_duration {
            if !force {
                warn!("Timeout waiting for graceful shutdown, forcing...");
                daemon::send_signal(pid, Signal::SIGKILL)?;
                sleep(Duration::from_secs(1)).await;
            }

            if daemon::is_process_running(pid)? {
                return Err(Error::ShutdownTimeout);
            }
        }

        sleep(Duration::from_millis(100)).await;
    }

    // Clean up PID file
    if pid_file.exists() {
        fs::remove_file(&pid_file)?;
    }

    success!("TaskDaemon stopped successfully");
    Ok(())
}
```

### Status Command
```rust
pub async fn handle_status_command(
    detailed: bool,
    watch: bool,
    interval: u64,
    output_format: OutputFormat,
    config: &Config,
) -> Result<(), Error> {
    if watch {
        loop {
            clear_screen()?;
            show_status(detailed, output_format, config).await?;
            sleep(Duration::from_secs(interval)).await;
        }
    } else {
        show_status(detailed, output_format, config).await
    }
}

async fn show_status(
    detailed: bool,
    format: OutputFormat,
    config: &Config,
) -> Result<(), Error> {
    let client = DaemonClient::new(&config.daemon_url)?;

    match client.get_status().await {
        Ok(status) => {
            match format {
                OutputFormat::Human => print_human_status(&status, detailed),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&status)?),
                OutputFormat::Yaml => println!("{}", serde_yaml::to_string(&status)?),
                OutputFormat::Table => print_table_status(&status),
            }
            Ok(())
        }
        Err(e) => {
            if e.is_connection_error() {
                error!("TaskDaemon is not running or not reachable");
                Err(Error::NotRunning)
            } else {
                Err(e.into())
            }
        }
    }
}

fn print_human_status(status: &DaemonStatus, detailed: bool) {
    println!("{}", "TaskDaemon Status".bold().underline());
    println!();

    println!("  {}: {}", "State".bold(), status.state.to_string().green());
    println!("  {}: {}", "Version".bold(), status.version);
    println!("  {}: {}", "Uptime".bold(), format_duration(status.uptime));
    println!();

    println!("  {}", "Active Loops:".bold());
    println!("    Plans:      {}", status.active_plans);
    println!("    Specs:      {}", status.active_specs);
    println!("    Phases:     {}", status.active_phases);
    println!("    Total:      {}", status.total_active);
    println!();

    if detailed {
        println!("  {}", "Resource Usage:".bold());
        println!("    CPU:        {:.1}%", status.cpu_usage);
        println!("    Memory:     {}", format_bytes(status.memory_usage));
        println!("    Disk:       {}", format_bytes(status.disk_usage));
        println!();

        println!("  {}", "Performance:".bold());
        println!("    Requests/s: {:.1}", status.requests_per_second);
        println!("    Avg Latency: {:.1}ms", status.avg_latency_ms);
        println!();

        if !status.recent_errors.is_empty() {
            println!("  {} ({}):", "Recent Errors".bold().red(), status.recent_errors.len());
            for error in status.recent_errors.iter().take(5) {
                println!("    - {}: {}", error.timestamp.format("%H:%M:%S"), error.message);
            }
        }
    }
}
```

### New Plan Command
```rust
pub async fn handle_new_plan_command(
    name: String,
    goal: Option<String>,
    template: Option<String>,
    interactive: bool,
    start: bool,
    config: &Config,
) -> Result<(), Error> {
    let client = DaemonClient::new(&config.daemon_url)?;

    // Build plan request
    let mut request = if interactive {
        create_plan_interactively(name).await?
    } else {
        NewPlanRequest {
            name: name.clone(),
            goal: goal.ok_or_else(|| Error::MissingArgument("goal"))?,
            template,
            metadata: HashMap::new(),
        }
    };

    // Apply template if specified
    if let Some(template_name) = &request.template {
        apply_template(&mut request, template_name, config)?;
    }

    // Create plan
    info!("Creating plan '{}'...", request.name);

    let plan = client.create_plan(request).await?;

    success!("Plan created successfully!");
    println!("  ID: {}", plan.id.to_string().dimmed());
    println!("  Name: {}", plan.name.bold());
    println!("  Status: {}", plan.status.to_string().yellow());

    if start {
        info!("Starting plan execution...");
        client.start_plan(plan.id).await?;
        success!("Plan execution started!");
    } else {
        println!();
        println!("To start this plan, run:");
        println!("  {} start-plan {}", "taskdaemon".bold(), plan.id);
    }

    Ok(())
}

async fn create_plan_interactively(default_name: String) -> Result<NewPlanRequest, Error> {
    use dialoguer::{Input, Editor, Confirm, Select};

    println!("{}", "Create New Plan".bold().underline());
    println!();

    let name = Input::<String>::new()
        .with_prompt("Plan name")
        .default(default_name)
        .interact()?;

    let goal = Editor::new()
        .with_prompt("Plan goal (opens editor)")
        .edit()?
        .ok_or(Error::UserCancelled)?;

    let use_template = Confirm::new()
        .with_prompt("Use a template?")
        .default(false)
        .interact()?;

    let template = if use_template {
        let templates = vec!["web-app", "cli-tool", "library", "microservice", "custom"];
        let selection = Select::new()
            .with_prompt("Select template")
            .items(&templates)
            .interact()?;
        Some(templates[selection].to_string())
    } else {
        None
    };

    Ok(NewPlanRequest {
        name,
        goal,
        template,
        metadata: HashMap::new(),
    })
}
```

### Output Formatting
```rust
pub struct OutputFormatter {
    format: OutputFormat,
    colors: bool,
}

impl OutputFormatter {
    pub fn print_success(&self, message: &str) {
        match self.format {
            OutputFormat::Human => {
                if self.colors {
                    println!("{} {}", "✓".green(), message);
                } else {
                    println!("✓ {}", message);
                }
            }
            OutputFormat::Json => {
                println!("{}", json!({ "status": "success", "message": message }));
            }
            _ => println!("{}", message),
        }
    }

    pub fn print_error(&self, error: &Error) {
        match self.format {
            OutputFormat::Human => {
                if self.colors {
                    eprintln!("{} {}", "✗".red(), error);
                } else {
                    eprintln!("✗ {}", error);
                }
            }
            OutputFormat::Json => {
                eprintln!("{}", json!({ "status": "error", "message": error.to_string() }));
            }
            _ => eprintln!("{}", error),
        }
    }
}

// Macros for consistent output
macro_rules! info {
    ($($arg:tt)*) => {
        println!("{} {}", "ℹ".blue(), format!($($arg)*));
    };
}

macro_rules! success {
    ($($arg:tt)*) => {
        println!("{} {}", "✓".green(), format!($($arg)*));
    };
}

macro_rules! error {
    ($($arg:tt)*) => {
        eprintln!("{} {}", "✗".red(), format!($($arg)*));
    };
}

macro_rules! warn {
    ($($arg:tt)*) => {
        eprintln!("{} {}", "⚠".yellow(), format!($($arg)*));
    };
}
```

### Shell Completion
```rust
pub fn generate_completion(shell: Shell) {
    let mut app = Cli::command();
    let name = app.get_name().to_string();

    clap_complete::generate(shell, &mut app, name, &mut io::stdout());
}

// Usage:
// taskdaemon completion bash > /etc/bash_completion.d/taskdaemon
// taskdaemon completion zsh > /usr/share/zsh/site-functions/_taskdaemon
// taskdaemon completion fish > ~/.config/fish/completions/taskdaemon.fish
```

## Notes

- Use exit codes consistently (0 for success, 1 for general error, specific codes for specific errors)
- Support both environment variables and config files for settings
- Implement shell completion for better UX
- Consider adding a `taskdaemon init` command to set up initial configuration