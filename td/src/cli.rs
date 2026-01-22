//! CLI command definitions and subcommands

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::debug;

/// TaskDaemon - Ralph Wiggum Loop Orchestrator
#[derive(Parser)]
#[command(
    name = "taskdaemon",
    about = "Ralph Wiggum loop orchestrator for concurrent AI workflows",
    version = env!("GIT_DESCRIBE"),
)]
pub struct Cli {
    /// Path to config file
    #[arg(short, long, global = true, help = "Path to config file")]
    pub config: Option<PathBuf>,

    /// Log level (TRACE, DEBUG, INFO, WARN, ERROR)
    #[arg(
        short = 'l',
        long = "log-level",
        global = true,
        help = "Log level (TRACE, DEBUG, INFO, WARN, ERROR)"
    )]
    pub log_level: Option<String>,

    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// CLI subcommands
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage the taskdaemon daemon
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },

    /// Run a loop to completion (batch mode)
    Run {
        /// Loop type to run (plan, spec, phase, ralph)
        #[arg(value_name = "TYPE")]
        loop_type: String,

        /// Task description or file
        task: String,

        /// Maximum iterations
        #[arg(short, long)]
        max_iterations: Option<u32>,
    },

    /// Internal: Run as daemon process (used by `daemon start`)
    #[command(hide = true)]
    RunDaemon,

    /// List available loop types
    Loops,

    /// Show metrics and statistics
    Metrics {
        /// Loop type to filter by
        #[arg(short = 't', long)]
        loop_type: Option<String>,

        /// Output format
        #[arg(short, long, default_value = "text")]
        format: OutputFormat,
    },

    /// Show daemon logs
    Logs {
        /// Follow log output (like tail -f)
        #[arg(short, long)]
        follow: bool,

        /// Number of lines to show
        #[arg(short, long, default_value = "50")]
        lines: usize,
    },

    /// Manage executions (for testing state transitions)
    Exec {
        #[command(subcommand)]
        command: ExecCommand,
    },
}

/// Execution management subcommands
#[derive(Debug, Subcommand)]
pub enum ExecCommand {
    /// List all executions
    List {
        /// Filter by status (draft, pending, running, paused, complete, failed)
        #[arg(short, long)]
        status: Option<String>,
    },

    /// Start a draft execution (draft -> pending)
    Start {
        /// Execution ID (or partial match)
        id: String,
    },

    /// Pause a running execution (running -> paused)
    Pause {
        /// Execution ID (or partial match)
        id: String,
    },

    /// Resume a paused execution (paused -> running)
    Resume {
        /// Execution ID (or partial match)
        id: String,
    },

    /// Set execution status directly (for testing)
    Status {
        /// Execution ID (or partial match)
        id: String,

        /// New status (draft, pending, running, paused, complete, failed)
        status: String,
    },
}

/// Daemon management subcommands
#[derive(Debug, Subcommand)]
pub enum DaemonCommand {
    /// Start the daemon
    Start {
        /// Don't fork to background (run in foreground)
        #[arg(long)]
        foreground: bool,
    },

    /// Stop the daemon
    Stop,

    /// Check daemon status
    Status {
        /// Show detailed loop information
        #[arg(short, long)]
        detailed: bool,

        /// Output format
        #[arg(short, long, default_value = "text")]
        format: OutputFormat,
    },

    /// Ping the daemon to check if it's alive and responsive
    Ping,
}

/// Result of checking a required tool
pub struct ToolCheck {
    pub name: &'static str,
    pub available: bool,
    pub version: Option<String>,
}

impl ToolCheck {
    /// Check if a tool is available and get its version
    pub fn check(name: &'static str, version_args: &[&str]) -> Self {
        debug!(name, ?version_args, "ToolCheck::check: called");
        let result = std::process::Command::new(name).args(version_args).output();

        match result {
            Ok(output) if output.status.success() => {
                debug!(name, "ToolCheck::check: tool available");
                let version_str = String::from_utf8_lossy(&output.stdout);
                let version = parse_version(&version_str);
                Self {
                    name,
                    available: true,
                    version: Some(version),
                }
            }
            _ => {
                debug!(name, "ToolCheck::check: tool not available");
                Self {
                    name,
                    available: false,
                    version: None,
                }
            }
        }
    }
}

/// Parse version from command output (extracts first version-like string)
fn parse_version(output: &str) -> String {
    debug!(%output, "parse_version: called");
    // Look for patterns like "1.2.3" or "v1.2.3"
    for word in output.split_whitespace() {
        let word = word.trim_start_matches('v');
        if word.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            // Take until non-version character
            let version: String = word.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
            if !version.is_empty() {
                debug!(%version, "parse_version: found version");
                return version;
            }
        }
    }
    debug!("parse_version: no version found, returning unknown");
    "unknown".to_string()
}

/// Check all required tools and return their status
pub fn check_required_tools() -> Vec<ToolCheck> {
    debug!("check_required_tools: called");
    let tools = vec![
        ToolCheck::check("bwrap", &["--version"]),
        ToolCheck::check("git", &["--version"]),
    ];
    debug!(count = tools.len(), "check_required_tools: returning tools");
    tools
}

/// Check if the daemon is running (lightweight check for help display)
pub fn is_daemon_running() -> bool {
    debug!("is_daemon_running: called");
    // Use the same path logic as daemon.rs:default_pid_path()
    let pid_file = dirs::runtime_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("taskdaemon")
        .join("taskdaemon.pid");

    if !pid_file.exists() {
        debug!(?pid_file, "is_daemon_running: pid file does not exist");
        return false;
    }

    if let Ok(contents) = std::fs::read_to_string(&pid_file)
        && let Ok(pid) = contents.trim().parse::<u32>()
    {
        // Check if process exists
        let exists = PathBuf::from(format!("/proc/{}", pid)).exists();
        debug!(pid, exists, "is_daemon_running: checked process existence");
        return exists;
    }

    debug!("is_daemon_running: could not read or parse pid file");
    false
}

/// Get the log file path
pub fn get_log_path() -> PathBuf {
    debug!("get_log_path: called");
    let path = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("taskdaemon")
        .join("logs")
        .join("taskdaemon.log");
    debug!(?path, "get_log_path: returning path");
    path
}

/// Generate the after_help text with tool checks and daemon status
pub fn generate_after_help() -> String {
    debug!("generate_after_help: called");
    let tools = check_required_tools();
    let daemon_running = is_daemon_running();
    let log_path = get_log_path();

    let mut help = String::new();

    // Required Tools section
    help.push_str("Required Tools:\n");
    for tool in &tools {
        let icon = if tool.available {
            debug!(name = tool.name, "generate_after_help: tool available");
            "\u{2705}"
        } else {
            debug!(name = tool.name, "generate_after_help: tool not available");
            "\u{274C}"
        };
        let version = tool.version.as_deref().unwrap_or("not found");
        help.push_str(&format!("  {} {:<10} {}\n", icon, tool.name, version));
    }

    // Daemon section
    help.push('\n');
    help.push_str("Daemon:\n");
    let daemon_icon = if daemon_running {
        debug!("generate_after_help: daemon is running");
        "\u{2705}"
    } else {
        debug!("generate_after_help: daemon is stopped");
        "\u{274C}"
    };
    let daemon_status = if daemon_running { "running" } else { "stopped" };
    help.push_str(&format!("  {} {}\n", daemon_icon, daemon_status));

    // Log path
    help.push('\n');
    help.push_str(&format!("Logs are written to: {}\n", log_path.display()));

    debug!("generate_after_help: returning help text");
    help
}

/// Output format for status/metrics commands
#[derive(Clone, Debug, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Table,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        debug!(%s, "OutputFormat::from_str: called");
        match s.to_lowercase().as_str() {
            "text" | "plain" => {
                debug!("OutputFormat::from_str: matched Text");
                Ok(Self::Text)
            }
            "json" => {
                debug!("OutputFormat::from_str: matched Json");
                Ok(Self::Json)
            }
            "table" => {
                debug!("OutputFormat::from_str: matched Table");
                Ok(Self::Table)
            }
            _ => {
                debug!(%s, "OutputFormat::from_str: unknown format");
                Err(format!("Unknown format: {}. Use: text, json, or table", s))
            }
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        debug!(?self, "OutputFormat::fmt: called");
        match self {
            Self::Text => {
                debug!("OutputFormat::fmt: writing text");
                write!(f, "text")
            }
            Self::Json => {
                debug!("OutputFormat::fmt: writing json");
                write!(f, "json")
            }
            Self::Table => {
                debug!("OutputFormat::fmt: writing table");
                write!(f, "table")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse_no_command() {
        let cli = Cli::parse_from(["taskdaemon"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_parse_daemon_start() {
        let cli = Cli::parse_from(["taskdaemon", "daemon", "start"]);
        assert!(matches!(
            cli.command,
            Some(Command::Daemon {
                command: DaemonCommand::Start { foreground: false }
            })
        ));
    }

    #[test]
    fn test_cli_parse_daemon_start_foreground() {
        let cli = Cli::parse_from(["taskdaemon", "daemon", "start", "--foreground"]);
        assert!(matches!(
            cli.command,
            Some(Command::Daemon {
                command: DaemonCommand::Start { foreground: true }
            })
        ));
    }

    #[test]
    fn test_cli_parse_daemon_stop() {
        let cli = Cli::parse_from(["taskdaemon", "daemon", "stop"]);
        assert!(matches!(
            cli.command,
            Some(Command::Daemon {
                command: DaemonCommand::Stop
            })
        ));
    }

    #[test]
    fn test_cli_parse_daemon_status() {
        let cli = Cli::parse_from(["taskdaemon", "daemon", "status"]);
        assert!(matches!(
            cli.command,
            Some(Command::Daemon {
                command: DaemonCommand::Status { .. }
            })
        ));
    }

    #[test]
    fn test_cli_parse_run() {
        let cli = Cli::parse_from(["taskdaemon", "run", "ralph", "Fix the bug"]);
        if let Some(Command::Run {
            loop_type,
            task,
            max_iterations,
        }) = cli.command
        {
            assert_eq!(loop_type, "ralph");
            assert_eq!(task, "Fix the bug");
            assert!(max_iterations.is_none());
        } else {
            panic!("Expected Run command");
        }
    }

    #[test]
    fn test_output_format_from_str() {
        assert!(matches!("text".parse::<OutputFormat>(), Ok(OutputFormat::Text)));
        assert!(matches!("json".parse::<OutputFormat>(), Ok(OutputFormat::Json)));
        assert!(matches!("table".parse::<OutputFormat>(), Ok(OutputFormat::Table)));
        assert!("invalid".parse::<OutputFormat>().is_err());
    }

    #[test]
    fn test_cli_with_config() {
        let cli = Cli::parse_from(["taskdaemon", "-c", "/path/to/config.yml", "daemon", "status"]);
        assert_eq!(cli.config, Some(PathBuf::from("/path/to/config.yml")));
    }

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("git version 2.43.0"), "2.43.0");
        assert_eq!(parse_version("bwrap 0.9.0"), "0.9.0");
        assert_eq!(parse_version("v1.2.3"), "1.2.3");
    }
}
