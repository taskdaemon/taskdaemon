//! Event types for TaskDaemon activity streaming
//!
//! These events represent all observable activity in TaskDaemon:
//! - Loop lifecycle (start, iteration, complete)
//! - LLM interactions (prompts, streaming tokens, responses)
//! - Tool execution (start, complete)
//! - Validation (start, output lines, complete)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Core event enum - the vocabulary of TD's activity
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TdEvent {
    // === Loop Lifecycle ===
    /// A loop has started execution
    LoopStarted {
        execution_id: String,
        loop_type: String,
        task_description: String,
    },
    /// A phase within a loop has started
    PhaseStarted {
        execution_id: String,
        phase_index: usize,
        phase_name: String,
        total_phases: usize,
    },
    /// An iteration within a loop has started
    IterationStarted { execution_id: String, iteration: u32 },
    /// An iteration has completed
    IterationCompleted {
        execution_id: String,
        iteration: u32,
        outcome: IterationOutcome,
    },
    /// A loop has completed
    LoopCompleted {
        execution_id: String,
        success: bool,
        total_iterations: u32,
    },

    // === LLM Interactions ===
    /// A prompt has been sent to the LLM
    PromptSent {
        execution_id: String,
        iteration: u32,
        /// First 200 chars or structured summary
        prompt_summary: String,
        token_count: u64,
    },
    /// A token has been received from the LLM (streaming)
    TokenReceived {
        execution_id: String,
        iteration: u32,
        token: String,
    },
    /// The LLM response is complete
    ResponseCompleted {
        execution_id: String,
        iteration: u32,
        response_summary: String,
        input_tokens: u64,
        output_tokens: u64,
        has_tool_calls: bool,
    },

    // === Tool Execution ===
    /// A tool call has started
    ToolCallStarted {
        execution_id: String,
        iteration: u32,
        tool_name: String,
        tool_args_summary: String,
    },
    /// A tool call has completed
    ToolCallCompleted {
        execution_id: String,
        iteration: u32,
        tool_name: String,
        success: bool,
        result_summary: String,
        duration_ms: u64,
    },

    // === Validation ===
    /// Validation has started
    ValidationStarted {
        execution_id: String,
        iteration: u32,
        command: String,
    },
    /// A line of validation output (streaming)
    ValidationOutput {
        execution_id: String,
        iteration: u32,
        line: String,
        is_stderr: bool,
    },
    /// Validation has completed
    ValidationCompleted {
        execution_id: String,
        iteration: u32,
        exit_code: i32,
        duration_ms: u64,
    },

    // === Errors & Warnings ===
    /// An error occurred
    Error {
        execution_id: String,
        context: String,
        message: String,
    },
    /// A warning occurred
    Warning {
        execution_id: String,
        context: String,
        message: String,
    },
}

impl TdEvent {
    /// Get the execution ID for this event
    pub fn execution_id(&self) -> &str {
        match self {
            TdEvent::LoopStarted { execution_id, .. }
            | TdEvent::PhaseStarted { execution_id, .. }
            | TdEvent::IterationStarted { execution_id, .. }
            | TdEvent::IterationCompleted { execution_id, .. }
            | TdEvent::LoopCompleted { execution_id, .. }
            | TdEvent::PromptSent { execution_id, .. }
            | TdEvent::TokenReceived { execution_id, .. }
            | TdEvent::ResponseCompleted { execution_id, .. }
            | TdEvent::ToolCallStarted { execution_id, .. }
            | TdEvent::ToolCallCompleted { execution_id, .. }
            | TdEvent::ValidationStarted { execution_id, .. }
            | TdEvent::ValidationOutput { execution_id, .. }
            | TdEvent::ValidationCompleted { execution_id, .. }
            | TdEvent::Error { execution_id, .. }
            | TdEvent::Warning { execution_id, .. } => execution_id,
        }
    }

    /// Get the event type name
    pub fn event_type(&self) -> &'static str {
        match self {
            TdEvent::LoopStarted { .. } => "LoopStarted",
            TdEvent::PhaseStarted { .. } => "PhaseStarted",
            TdEvent::IterationStarted { .. } => "IterationStarted",
            TdEvent::IterationCompleted { .. } => "IterationCompleted",
            TdEvent::LoopCompleted { .. } => "LoopCompleted",
            TdEvent::PromptSent { .. } => "PromptSent",
            TdEvent::TokenReceived { .. } => "TokenReceived",
            TdEvent::ResponseCompleted { .. } => "ResponseCompleted",
            TdEvent::ToolCallStarted { .. } => "ToolCallStarted",
            TdEvent::ToolCallCompleted { .. } => "ToolCallCompleted",
            TdEvent::ValidationStarted { .. } => "ValidationStarted",
            TdEvent::ValidationOutput { .. } => "ValidationOutput",
            TdEvent::ValidationCompleted { .. } => "ValidationCompleted",
            TdEvent::Error { .. } => "Error",
            TdEvent::Warning { .. } => "Warning",
        }
    }
}

/// Outcome of a single iteration
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "outcome_type")]
pub enum IterationOutcome {
    /// Validation passed
    ValidationPassed,
    /// Validation failed with exit code
    ValidationFailed { exit_code: i32 },
    /// Max turns reached within iteration
    MaxTurnsReached,
    /// Tool error occurred
    ToolError { tool: String, error: String },
    /// LLM error occurred
    LlmError { error: String },
}

/// A timestamped event log entry for file persistence
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventLogEntry {
    /// Timestamp of the event
    #[serde(rename = "ts")]
    pub timestamp: DateTime<Utc>,
    /// The event
    pub event: TdEvent,
}

impl EventLogEntry {
    /// Create a new log entry with current timestamp
    pub fn new(event: TdEvent) -> Self {
        Self {
            timestamp: Utc::now(),
            event,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_execution_id() {
        let event = TdEvent::LoopStarted {
            execution_id: "test-123".to_string(),
            loop_type: "plan".to_string(),
            task_description: "Test task".to_string(),
        };
        assert_eq!(event.execution_id(), "test-123");
    }

    #[test]
    fn test_event_type() {
        let event = TdEvent::TokenReceived {
            execution_id: "test-123".to_string(),
            iteration: 1,
            token: "hello".to_string(),
        };
        assert_eq!(event.event_type(), "TokenReceived");
    }

    #[test]
    fn test_event_serialization() {
        let event = TdEvent::IterationCompleted {
            execution_id: "test-123".to_string(),
            iteration: 1,
            outcome: IterationOutcome::ValidationPassed,
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("IterationCompleted"));
        assert!(json.contains("ValidationPassed"));

        // Deserialize back
        let parsed: TdEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.execution_id(), "test-123");
    }

    #[test]
    fn test_event_log_entry() {
        let event = TdEvent::LoopStarted {
            execution_id: "test-123".to_string(),
            loop_type: "plan".to_string(),
            task_description: "Test".to_string(),
        };
        let entry = EventLogEntry::new(event);

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("ts"));
        assert!(json.contains("LoopStarted"));
    }
}
