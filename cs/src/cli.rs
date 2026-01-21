//! CLI argument parsing for contextstore

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "cs")]
#[command(author, version, about = "RLM-style external context store", long_about = None)]
pub struct Cli {
    /// Path to config file
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Ingest files into a new context
    Ingest {
        /// File paths or glob patterns to ingest
        #[arg(required = true)]
        paths: Vec<String>,

        /// Chunk size in bytes (default: 32KB)
        #[arg(short = 's', long)]
        chunk_size: Option<usize>,

        /// Overlap between chunks in bytes (default: 2KB)
        #[arg(short, long)]
        overlap: Option<usize>,
    },

    /// Search within a context
    Search {
        /// Context ID to search
        #[arg(required = true)]
        context_id: String,

        /// Search pattern (regex)
        #[arg(required = true)]
        pattern: String,

        /// Maximum results to return
        #[arg(short, long)]
        max_results: Option<usize>,
    },

    /// Display a chunk's content
    Cat {
        /// Chunk ID to display
        #[arg(required = true)]
        chunk_id: String,
    },

    /// Get a window of text around an offset in a chunk
    Window {
        /// Chunk ID
        #[arg(required = true)]
        chunk_id: String,

        /// Center offset in bytes
        #[arg(required = true)]
        offset: usize,

        /// Radius in bytes (default: 500)
        #[arg(short, long, default_value = "500")]
        radius: usize,
    },

    /// Show statistics for a context
    Stats {
        /// Context ID
        #[arg(required = true)]
        context_id: String,
    },

    /// List all contexts
    List,

    /// Delete a context
    Delete {
        /// Context ID to delete
        #[arg(required = true)]
        context_id: String,
    },
}
