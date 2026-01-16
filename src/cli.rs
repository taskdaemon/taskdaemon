//! CLI command definitions and subcommands

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// TaskDaemon - Ralph Wiggum Loop Orchestrator
#[derive(Parser)]
#[command(
    name = "taskdaemon",
    about = "Ralph Wiggum loop orchestrator for concurrent AI workflows",
    version = env!("GIT_DESCRIBE"),
    after_help = "Logs are written to: ~/.local/share/taskdaemon/logs/taskdaemon.log"
)]
pub struct Cli {
    /// Path to config file
    #[arg(short, long, global = true, help = "Path to config file")]
    pub config: Option<PathBuf>,

    /// Enable verbose output
    #[arg(short, long, global = true, help = "Enable verbose output")]
    pub verbose: bool,

    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// CLI subcommands
#[derive(Subcommand)]
pub enum Command {
    /// Start the daemon in the background
    Start {
        /// Don't fork to background (run in foreground)
        #[arg(long)]
        foreground: bool,
    },

    /// Stop the running daemon
    Stop,

    /// Show daemon status and running loops
    Status {
        /// Show detailed loop information
        #[arg(short, long)]
        detailed: bool,

        /// Output format
        #[arg(short, long, default_value = "text")]
        format: OutputFormat,
    },

    /// Launch the interactive TUI
    Tui,

    /// Show daemon logs
    Logs {
        /// Follow log output (like tail -f)
        #[arg(short, long)]
        follow: bool,

        /// Number of lines to show
        #[arg(short, long, default_value = "50")]
        lines: usize,
    },

    /// Run a single loop (for development/testing)
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

    /// Internal: Run as daemon process (used by `start`)
    #[command(hide = true)]
    RunDaemon,

    /// List available loop types
    ListLoops,

    /// Show metrics and statistics
    Metrics {
        /// Loop type to filter by
        #[arg(short = 't', long)]
        loop_type: Option<String>,

        /// Output format
        #[arg(short, long, default_value = "text")]
        format: OutputFormat,
    },
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
        match s.to_lowercase().as_str() {
            "text" | "plain" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            "table" => Ok(Self::Table),
            _ => Err(format!("Unknown format: {}. Use: text, json, or table", s)),
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text => write!(f, "text"),
            Self::Json => write!(f, "json"),
            Self::Table => write!(f, "table"),
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
    fn test_cli_parse_start() {
        let cli = Cli::parse_from(["taskdaemon", "start"]);
        assert!(matches!(cli.command, Some(Command::Start { foreground: false })));
    }

    #[test]
    fn test_cli_parse_start_foreground() {
        let cli = Cli::parse_from(["taskdaemon", "start", "--foreground"]);
        assert!(matches!(cli.command, Some(Command::Start { foreground: true })));
    }

    #[test]
    fn test_cli_parse_stop() {
        let cli = Cli::parse_from(["taskdaemon", "stop"]);
        assert!(matches!(cli.command, Some(Command::Stop)));
    }

    #[test]
    fn test_cli_parse_status() {
        let cli = Cli::parse_from(["taskdaemon", "status"]);
        assert!(matches!(cli.command, Some(Command::Status { .. })));
    }

    #[test]
    fn test_cli_parse_tui() {
        let cli = Cli::parse_from(["taskdaemon", "tui"]);
        assert!(matches!(cli.command, Some(Command::Tui)));
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
        let cli = Cli::parse_from(["taskdaemon", "-c", "/path/to/config.yml", "status"]);
        assert_eq!(cli.config, Some(PathBuf::from("/path/to/config.yml")));
    }
}
