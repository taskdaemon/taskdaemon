//! Interactive REPL for TaskDaemon
//!
//! Provides a real interactive REPL experience with streaming output,
//! tool execution, and slash commands.

mod session;

pub use session::ReplSession;

use std::sync::Arc;

use eyre::Result;

use crate::config::Config;
use crate::llm::{AnthropicClient, LlmClient};

/// Run the interactive REPL
///
/// This is the main entry point for `taskdaemon repl`.
pub async fn run_interactive(config: &Config, initial_task: Option<String>) -> Result<()> {
    // Validate API key early
    if std::env::var(&config.llm.api_key_env).is_err() {
        return Err(eyre::eyre!(
            "LLM API key not found. Set the {} environment variable.",
            config.llm.api_key_env
        ));
    }

    // Create LLM client
    let llm: Arc<dyn LlmClient> = Arc::new(
        AnthropicClient::from_config(&config.llm).map_err(|e| eyre::eyre!("Failed to create LLM client: {}", e))?,
    );

    // Use current directory as worktree
    let worktree = std::env::current_dir()?;

    // Create and run session
    let mut session = ReplSession::new(llm, worktree);
    session.run(initial_task).await
}
