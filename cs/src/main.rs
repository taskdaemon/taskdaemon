use clap::Parser;
use colored::*;
use eyre::{Context, Result};
use log::info;

use contextstore::ContextStore;
use contextstore::cli::Cli;
use contextstore::config::Config;

fn setup_logging() -> Result<()> {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();
    Ok(())
}

fn main() -> Result<()> {
    setup_logging().context("Failed to setup logging")?;

    let cli = Cli::parse();
    let config = Config::load(cli.config.as_ref()).context("Failed to load configuration")?;

    info!("contextstore starting");

    match cli.command {
        contextstore::cli::Command::Ingest {
            paths,
            chunk_size,
            overlap,
        } => {
            let store = ContextStore::open(&config.store_path)?;
            let ctx_id = store.ingest(
                &paths,
                contextstore::IngestOptions {
                    chunk_size: chunk_size.unwrap_or(contextstore::DEFAULT_CHUNK_SIZE),
                    overlap: overlap.unwrap_or(contextstore::DEFAULT_OVERLAP),
                },
            )?;
            println!("{} Ingested to context: {}", "✓".green(), ctx_id.cyan());
        }
        contextstore::cli::Command::Search {
            context_id,
            pattern,
            max_results,
        } => {
            let store = ContextStore::open(&config.store_path)?;
            let matches = store.search(
                &context_id,
                &pattern,
                contextstore::SearchOptions {
                    max_results: max_results.unwrap_or(10),
                    ..Default::default()
                },
            )?;
            for m in matches {
                println!(
                    "{}:{} {}",
                    m.chunk_id.yellow(),
                    m.offset.to_string().dimmed(),
                    m.snippet
                );
            }
        }
        contextstore::cli::Command::Cat { chunk_id } => {
            let store = ContextStore::open(&config.store_path)?;
            let content = store.get_chunk(&chunk_id)?;
            println!("{}", content);
        }
        contextstore::cli::Command::Window {
            chunk_id,
            offset,
            radius,
        } => {
            let store = ContextStore::open(&config.store_path)?;
            let content = store.get_window(&chunk_id, offset, radius)?;
            println!("{}", content);
        }
        contextstore::cli::Command::Stats { context_id } => {
            let store = ContextStore::open(&config.store_path)?;
            let stats = store.stats(&context_id)?;
            println!("Context: {}", context_id.cyan());
            println!("  Chunks: {}", stats.chunk_count);
            println!("  Total bytes: {}", stats.total_bytes);
            println!("  Sources: {}", stats.source_count);
        }
        contextstore::cli::Command::List => {
            let store = ContextStore::open(&config.store_path)?;
            let contexts = store.list_contexts()?;
            if contexts.is_empty() {
                println!("No contexts found");
            } else {
                for ctx in contexts {
                    println!("{}", ctx);
                }
            }
        }
        contextstore::cli::Command::Delete { context_id } => {
            let store = ContextStore::open(&config.store_path)?;
            store.delete(&context_id)?;
            println!("{} Deleted context: {}", "✓".green(), context_id);
        }
    }

    Ok(())
}
