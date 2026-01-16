//! REPL session management

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;

use colored::Colorize;
use eyre::Result;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use tokio::sync::mpsc;

use crate::llm::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmClient, Message, MessageContent, StopReason, StreamChunk,
};
use crate::tools::{ToolContext, ToolExecutor};

/// Interactive REPL session
pub struct ReplSession {
    llm: Arc<dyn LlmClient>,
    tool_executor: ToolExecutor,
    conversation: Vec<Message>,
    system_prompt: String,
    worktree: PathBuf,
}

impl ReplSession {
    /// Create a new REPL session
    pub fn new(llm: Arc<dyn LlmClient>, worktree: PathBuf) -> Self {
        let system_prompt = format!(
            r#"You are an AI coding assistant in an interactive REPL.

You have access to the following tools:
- read_file: Read file contents
- write_file: Create or overwrite a file
- edit_file: Make targeted edits to a file
- list_directory: List files in a directory
- glob: Find files matching a pattern
- grep: Search file contents with regex
- run_command: Execute shell commands

Guidelines:
- Execute tools directly when needed - don't ask for permission
- Be concise in your responses
- Show file contents when asked to read files
- Explain what you're doing before executing tools

Working directory: {}"#,
            worktree.display()
        );

        // Create executor with standard tools but only expose REPL-appropriate ones
        let tool_executor = ToolExecutor::standard();

        Self {
            llm,
            tool_executor,
            conversation: Vec::new(),
            system_prompt,
            worktree,
        }
    }

    /// Run the REPL main loop
    pub async fn run(&mut self, initial_task: Option<String>) -> Result<()> {
        self.print_welcome();

        // If initial task provided, process it first
        if let Some(task) = initial_task {
            println!("{} {}", ">".bright_green(), task);
            self.process_user_input(&task).await?;
        }

        // Create readline editor for proper line editing
        let mut rl = DefaultEditor::new().map_err(|e| eyre::eyre!("Failed to initialize readline: {}", e))?;

        // Main REPL loop
        loop {
            // Read user input with readline (handles backspace, arrows, etc.)
            let readline = rl.readline(&format!("{} ", ">".bright_green()));

            match readline {
                Ok(line) => {
                    let input = line.trim();
                    if input.is_empty() {
                        continue;
                    }

                    // Add to history
                    let _ = rl.add_history_entry(input);

                    // Handle slash commands
                    if input.starts_with('/') {
                        match self.handle_slash_command(input) {
                            SlashResult::Continue => continue,
                            SlashResult::Quit => break,
                        }
                    } else {
                        // Process as LLM input
                        self.process_user_input(input).await?;
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    // Ctrl+C - just show new prompt
                    println!("^C");
                    continue;
                }
                Err(ReadlineError::Eof) => {
                    // Ctrl+D - exit
                    println!();
                    break;
                }
                Err(err) => {
                    return Err(eyre::eyre!("Readline error: {}", err));
                }
            }
        }

        println!("Goodbye!");
        Ok(())
    }

    /// Print welcome message
    fn print_welcome(&self) {
        println!();
        println!("{}", "TaskDaemon Interactive REPL".bright_cyan().bold());
        println!("Working directory: {}", self.worktree.display());
        println!("Type {} for help, {} to quit", "/help".yellow(), "/quit".yellow());
        println!();
    }

    /// Handle slash commands
    fn handle_slash_command(&mut self, input: &str) -> SlashResult {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts.first().copied().unwrap_or("");

        match cmd {
            "/help" | "/h" => {
                self.print_help();
                SlashResult::Continue
            }
            "/quit" | "/q" | "/exit" => SlashResult::Quit,
            "/clear" | "/c" => {
                self.conversation.clear();
                println!("{}", "Conversation cleared.".dimmed());
                SlashResult::Continue
            }
            "/history" => {
                self.print_history();
                SlashResult::Continue
            }
            _ => {
                println!("{} Unknown command: {}", "?".yellow(), cmd);
                println!("Type {} for available commands", "/help".yellow());
                SlashResult::Continue
            }
        }
    }

    /// Print help message
    fn print_help(&self) {
        println!();
        println!("{}", "Available Commands:".bright_cyan());
        println!("  {:14} Show this help", "/help".yellow());
        println!("  {:14} Exit the REPL", "/quit".yellow());
        println!("  {:14} Clear conversation history", "/clear".yellow());
        println!("  {:14} Show conversation history", "/history".yellow());
        println!();
        println!("{}", "Available Tools:".bright_cyan());
        println!("  {:14} Read file contents", "read_file".yellow());
        println!("  {:14} Write content to a file", "write_file".yellow());
        println!("  {:14} Edit file with search/replace", "edit_file".yellow());
        println!("  {:14} List directory contents", "list_directory".yellow());
        println!("  {:14} Find files by pattern", "glob".yellow());
        println!("  {:14} Search file contents", "grep".yellow());
        println!("  {:14} Run a shell command", "run_command".yellow());
        println!();
    }

    /// Print conversation history
    fn print_history(&self) {
        if self.conversation.is_empty() {
            println!("{}", "No conversation history.".dimmed());
            return;
        }

        println!();
        println!("{}", "Conversation History:".bright_cyan());
        for (i, msg) in self.conversation.iter().enumerate() {
            let role = match msg.role {
                crate::llm::Role::User => "User".bright_green(),
                crate::llm::Role::Assistant => "Assistant".bright_blue(),
            };
            let content_preview = match &msg.content {
                MessageContent::Text(text) => {
                    let preview: String = text.chars().take(50).collect();
                    if text.len() > 50 { format!("{}...", preview) } else { preview }
                }
                MessageContent::Blocks(blocks) => format!("[{} blocks]", blocks.len()),
            };
            println!("  {}. {}: {}", i + 1, role, content_preview);
        }
        println!();
    }

    /// Process user input and get LLM response
    async fn process_user_input(&mut self, input: &str) -> Result<()> {
        // Add user message to conversation
        self.conversation.push(Message::user(input));

        // Create tool context
        let ctx = ToolContext::new_unsandboxed(self.worktree.clone(), "repl".to_string());

        // LLM loop - continue until end_turn
        loop {
            let response = self.call_llm_streaming().await?;

            // Handle response based on stop reason
            match response.stop_reason {
                StopReason::EndTurn => {
                    // Done - add assistant response to conversation if there was content
                    if let Some(ref content) = response.content {
                        self.conversation.push(Message::assistant(content));
                    }
                    break;
                }
                StopReason::ToolUse => {
                    // Execute tools and continue
                    if response.tool_calls.is_empty() {
                        // Shouldn't happen, but handle gracefully
                        break;
                    }

                    // Build assistant message with tool uses
                    let mut blocks: Vec<ContentBlock> = Vec::new();
                    if let Some(ref content) = response.content {
                        blocks.push(ContentBlock::text(content));
                    }
                    for tc in &response.tool_calls {
                        blocks.push(ContentBlock::ToolUse {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            input: tc.input.clone(),
                        });
                    }
                    self.conversation.push(Message::assistant_blocks(blocks));

                    // Execute tools and collect results
                    let mut result_blocks: Vec<ContentBlock> = Vec::new();
                    for tc in &response.tool_calls {
                        println!();
                        println!("{} {}", "Tool:".bright_yellow(), tc.name.bright_white());

                        let result = self.tool_executor.execute(tc, &ctx).await;

                        // Print result
                        if result.is_error {
                            println!("{} {}", "Error:".red(), result.content);
                        } else {
                            // Truncate long outputs for display
                            let display_content = if result.content.len() > 2000 {
                                format!("{}... ({} chars total)", &result.content[..2000], result.content.len())
                            } else {
                                result.content.clone()
                            };
                            println!("{}", display_content.dimmed());
                        }

                        result_blocks.push(ContentBlock::tool_result(&tc.id, &result.content, result.is_error));
                    }

                    // Add tool results to conversation
                    self.conversation.push(Message::user_blocks(result_blocks));
                    println!();
                }
                StopReason::MaxTokens => {
                    println!("{}", "\n[Response truncated - max tokens reached]".yellow());
                    if let Some(ref content) = response.content {
                        self.conversation.push(Message::assistant(content));
                    }
                    break;
                }
                StopReason::StopSequence => {
                    if let Some(ref content) = response.content {
                        self.conversation.push(Message::assistant(content));
                    }
                    break;
                }
            }
        }

        println!();
        Ok(())
    }

    /// Call LLM with streaming output
    async fn call_llm_streaming(&self) -> Result<CompletionResponse> {
        // Build request
        let request = CompletionRequest {
            system_prompt: self.system_prompt.clone(),
            messages: self.conversation.clone(),
            tools: self.get_tool_definitions(),
            max_tokens: 4096,
        };

        // Create channel for streaming
        let (tx, mut rx) = mpsc::channel::<StreamChunk>(100);

        // Spawn task to receive and print chunks
        let print_handle = tokio::spawn(async move {
            while let Some(chunk) = rx.recv().await {
                match chunk {
                    StreamChunk::TextDelta(text) => {
                        print!("{}", text);
                        let _ = io::stdout().flush();
                    }
                    StreamChunk::ToolUseStart { name, .. } => {
                        print!("\n{} ", format!("[calling {}]", name).dimmed());
                        let _ = io::stdout().flush();
                    }
                    StreamChunk::ToolUseDelta { .. } => {
                        // Don't print JSON fragments
                    }
                    StreamChunk::ToolUseEnd { .. } => {
                        // Tool call complete
                    }
                    StreamChunk::MessageDone { .. } => {
                        // Message complete
                    }
                    StreamChunk::Error(err) => {
                        eprintln!("\n{} {}", "Stream error:".red(), err);
                    }
                }
            }
        });

        // Call LLM with streaming
        let response = self
            .llm
            .stream(request, tx)
            .await
            .map_err(|e| eyre::eyre!("LLM error: {}", e))?;

        // Wait for print task to finish
        let _ = print_handle.await;

        Ok(response)
    }

    /// Get tool definitions for the REPL
    fn get_tool_definitions(&self) -> Vec<crate::llm::ToolDefinition> {
        // Only expose safe tools for interactive use
        let tool_names = vec![
            "read_file".to_string(),
            "write_file".to_string(),
            "edit_file".to_string(),
            "list_directory".to_string(),
            "glob".to_string(),
            "grep".to_string(),
            "run_command".to_string(),
        ];

        self.tool_executor.definitions_for(&tool_names)
    }
}

/// Result of handling a slash command
enum SlashResult {
    Continue,
    Quit,
}
