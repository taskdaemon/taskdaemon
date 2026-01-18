//! Conversation logging for REPL debugging
//!
//! When enabled via config (debug.log-conversations = true), logs all
//! REPL conversations to JSONL files in ~/.taskdaemon/conversations/

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, warn};

/// Entry in the conversation log
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationEntry {
    /// Timestamp of the entry
    pub timestamp: DateTime<Utc>,
    /// Type of entry
    pub entry_type: EntryType,
    /// Mode (Chat or Plan)
    pub mode: String,
}

/// Type of conversation entry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum EntryType {
    /// User sent a message
    UserMessage { content: String },
    /// Assistant responded
    AssistantMessage { content: String },
    /// Tool was called
    ToolCall { name: String, input: String },
    /// Tool returned a result
    ToolResult { name: String, output: String },
    /// Error occurred
    Error { message: String },
    /// Session started
    SessionStart,
    /// Session ended
    SessionEnd,
}

/// Logger for REPL conversations
pub struct ConversationLogger {
    /// Writer for the current log file
    writer: Option<BufWriter<File>>,
    /// Path to the current log file
    log_path: Option<PathBuf>,
    /// Current mode
    current_mode: String,
}

impl ConversationLogger {
    /// Create a new conversation logger (disabled)
    pub fn disabled() -> Self {
        Self {
            writer: None,
            log_path: None,
            current_mode: "Chat".to_string(),
        }
    }

    /// Create a new conversation logger (enabled)
    pub fn enabled() -> Self {
        let mut logger = Self {
            writer: None,
            log_path: None,
            current_mode: "Chat".to_string(),
        };

        if let Err(e) = logger.start_session() {
            error!("Failed to start conversation logging: {}", e);
        }

        logger
    }

    /// Check if logging is enabled
    pub fn is_enabled(&self) -> bool {
        self.writer.is_some()
    }

    /// Start a new logging session
    fn start_session(&mut self) -> std::io::Result<()> {
        // Create conversations directory
        let conv_dir = Self::conversations_dir();
        fs::create_dir_all(&conv_dir)?;

        // Generate timestamped filename
        let timestamp = Utc::now().format("%Y-%m-%dT%H-%M-%S");
        let filename = format!("conversation-{}.jsonl", timestamp);
        let log_path = conv_dir.join(&filename);

        // Open file for writing
        let file = OpenOptions::new().create(true).append(true).open(&log_path)?;

        let writer = BufWriter::new(file);
        self.writer = Some(writer);
        self.log_path = Some(log_path.clone());

        debug!("Started conversation logging to: {}", log_path.display());

        // Log session start
        self.log_entry(EntryType::SessionStart);

        Ok(())
    }

    /// Get the conversations directory
    fn conversations_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".taskdaemon")
            .join("conversations")
    }

    /// Set the current mode
    pub fn set_mode(&mut self, mode: &str) {
        self.current_mode = mode.to_string();
    }

    /// Log a user message
    pub fn log_user_message(&mut self, content: &str) {
        self.log_entry(EntryType::UserMessage {
            content: content.to_string(),
        });
    }

    /// Log an assistant message
    pub fn log_assistant_message(&mut self, content: &str) {
        self.log_entry(EntryType::AssistantMessage {
            content: content.to_string(),
        });
    }

    /// Log a tool call
    pub fn log_tool_call(&mut self, name: &str, input: &str) {
        self.log_entry(EntryType::ToolCall {
            name: name.to_string(),
            input: input.to_string(),
        });
    }

    /// Log a tool result
    pub fn log_tool_result(&mut self, name: &str, output: &str) {
        self.log_entry(EntryType::ToolResult {
            name: name.to_string(),
            output: output.to_string(),
        });
    }

    /// Log an error
    pub fn log_error(&mut self, message: &str) {
        self.log_entry(EntryType::Error {
            message: message.to_string(),
        });
    }

    /// Log an entry
    fn log_entry(&mut self, entry_type: EntryType) {
        let Some(writer) = &mut self.writer else {
            return;
        };

        let entry = ConversationEntry {
            timestamp: Utc::now(),
            entry_type,
            mode: self.current_mode.clone(),
        };

        match serde_json::to_string(&entry) {
            Ok(json) => {
                if let Err(e) = writeln!(writer, "{}", json) {
                    warn!("Failed to write conversation entry: {}", e);
                }
                // Flush after each entry for real-time debugging
                if let Err(e) = writer.flush() {
                    warn!("Failed to flush conversation log: {}", e);
                }
            }
            Err(e) => {
                warn!("Failed to serialize conversation entry: {}", e);
            }
        }
    }
}

impl Drop for ConversationLogger {
    fn drop(&mut self) {
        if self.writer.is_some() {
            self.log_entry(EntryType::SessionEnd);
            if let Some(path) = &self.log_path {
                debug!("Conversation log saved to: {}", path.display());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversation_entry_serialization() {
        let entry = ConversationEntry {
            timestamp: Utc::now(),
            entry_type: EntryType::UserMessage {
                content: "Hello".to_string(),
            },
            mode: "Chat".to_string(),
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("UserMessage"));
        assert!(json.contains("Hello"));
        assert!(json.contains("Chat"));
    }

    #[test]
    fn test_entry_types() {
        let entries = vec![
            EntryType::UserMessage {
                content: "test".to_string(),
            },
            EntryType::AssistantMessage {
                content: "response".to_string(),
            },
            EntryType::ToolCall {
                name: "read".to_string(),
                input: "{\"path\": \"/tmp/test\"}".to_string(),
            },
            EntryType::ToolResult {
                name: "read".to_string(),
                output: "file contents".to_string(),
            },
            EntryType::Error {
                message: "something went wrong".to_string(),
            },
            EntryType::SessionStart,
            EntryType::SessionEnd,
        ];

        for entry_type in entries {
            let entry = ConversationEntry {
                timestamp: Utc::now(),
                entry_type,
                mode: "Chat".to_string(),
            };
            // Should serialize without error
            let json = serde_json::to_string(&entry).unwrap();
            assert!(!json.is_empty());
        }
    }

    #[test]
    fn test_disabled_logger() {
        let logger = ConversationLogger::disabled();
        assert!(!logger.is_enabled());
    }
}
