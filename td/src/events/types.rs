//! Event types for TaskDaemon activity streaming
//!
//! These events represent all observable activity in TaskDaemon:
//! - Loop lifecycle (start, iteration, complete)
//! - LLM interactions (prompts, streaming tokens, responses)
//! - Tool execution (start, complete)
//! - Validation (start, output lines, complete)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Core event enum - the vocabulary of TaskDaemon activity
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Event {
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

impl Event {
    /// Get the execution ID for this event
    pub fn execution_id(&self) -> &str {
        match self {
            Event::LoopStarted { execution_id, .. }
            | Event::PhaseStarted { execution_id, .. }
            | Event::IterationStarted { execution_id, .. }
            | Event::IterationCompleted { execution_id, .. }
            | Event::LoopCompleted { execution_id, .. }
            | Event::PromptSent { execution_id, .. }
            | Event::TokenReceived { execution_id, .. }
            | Event::ResponseCompleted { execution_id, .. }
            | Event::ToolCallStarted { execution_id, .. }
            | Event::ToolCallCompleted { execution_id, .. }
            | Event::ValidationStarted { execution_id, .. }
            | Event::ValidationOutput { execution_id, .. }
            | Event::ValidationCompleted { execution_id, .. }
            | Event::Error { execution_id, .. }
            | Event::Warning { execution_id, .. } => execution_id,
        }
    }

    /// Get the event type name
    pub fn event_type(&self) -> &'static str {
        match self {
            Event::LoopStarted { .. } => "LoopStarted",
            Event::PhaseStarted { .. } => "PhaseStarted",
            Event::IterationStarted { .. } => "IterationStarted",
            Event::IterationCompleted { .. } => "IterationCompleted",
            Event::LoopCompleted { .. } => "LoopCompleted",
            Event::PromptSent { .. } => "PromptSent",
            Event::TokenReceived { .. } => "TokenReceived",
            Event::ResponseCompleted { .. } => "ResponseCompleted",
            Event::ToolCallStarted { .. } => "ToolCallStarted",
            Event::ToolCallCompleted { .. } => "ToolCallCompleted",
            Event::ValidationStarted { .. } => "ValidationStarted",
            Event::ValidationOutput { .. } => "ValidationOutput",
            Event::ValidationCompleted { .. } => "ValidationCompleted",
            Event::Error { .. } => "Error",
            Event::Warning { .. } => "Warning",
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
    pub event: Event,
}

impl EventLogEntry {
    /// Create a new log entry with current timestamp
    pub fn new(event: Event) -> Self {
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
        let event = Event::LoopStarted {
            execution_id: "test-123".to_string(),
            loop_type: "plan".to_string(),
            task_description: "Test task".to_string(),
        };
        assert_eq!(event.execution_id(), "test-123");
    }

    #[test]
    fn test_event_type() {
        let event = Event::TokenReceived {
            execution_id: "test-123".to_string(),
            iteration: 1,
            token: "hello".to_string(),
        };
        assert_eq!(event.event_type(), "TokenReceived");
    }

    #[test]
    fn test_event_serialization() {
        let event = Event::IterationCompleted {
            execution_id: "test-123".to_string(),
            iteration: 1,
            outcome: IterationOutcome::ValidationPassed,
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("IterationCompleted"));
        assert!(json.contains("ValidationPassed"));

        // Deserialize back
        let parsed: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.execution_id(), "test-123");
    }

    #[test]
    fn test_event_log_entry() {
        let event = Event::LoopStarted {
            execution_id: "test-123".to_string(),
            loop_type: "plan".to_string(),
            task_description: "Test".to_string(),
        };
        let entry = EventLogEntry::new(event);

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("ts"));
        assert!(json.contains("LoopStarted"));
    }

    #[test]
    fn test_all_event_types_have_execution_id() {
        // Every event type must have an execution_id - verify they all return it
        let exec_id = "exec-test";

        let events: Vec<Event> = vec![
            Event::LoopStarted {
                execution_id: exec_id.to_string(),
                loop_type: "plan".to_string(),
                task_description: "desc".to_string(),
            },
            Event::PhaseStarted {
                execution_id: exec_id.to_string(),
                phase_index: 0,
                phase_name: "phase".to_string(),
                total_phases: 1,
            },
            Event::IterationStarted {
                execution_id: exec_id.to_string(),
                iteration: 1,
            },
            Event::IterationCompleted {
                execution_id: exec_id.to_string(),
                iteration: 1,
                outcome: IterationOutcome::ValidationPassed,
            },
            Event::LoopCompleted {
                execution_id: exec_id.to_string(),
                success: true,
                total_iterations: 1,
            },
            Event::PromptSent {
                execution_id: exec_id.to_string(),
                iteration: 1,
                prompt_summary: "prompt".to_string(),
                token_count: 100,
            },
            Event::TokenReceived {
                execution_id: exec_id.to_string(),
                iteration: 1,
                token: "tok".to_string(),
            },
            Event::ResponseCompleted {
                execution_id: exec_id.to_string(),
                iteration: 1,
                response_summary: "resp".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                has_tool_calls: false,
            },
            Event::ToolCallStarted {
                execution_id: exec_id.to_string(),
                iteration: 1,
                tool_name: "read".to_string(),
                tool_args_summary: "{}".to_string(),
            },
            Event::ToolCallCompleted {
                execution_id: exec_id.to_string(),
                iteration: 1,
                tool_name: "read".to_string(),
                success: true,
                result_summary: "ok".to_string(),
                duration_ms: 10,
            },
            Event::ValidationStarted {
                execution_id: exec_id.to_string(),
                iteration: 1,
                command: "cargo test".to_string(),
            },
            Event::ValidationOutput {
                execution_id: exec_id.to_string(),
                iteration: 1,
                line: "output".to_string(),
                is_stderr: false,
            },
            Event::ValidationCompleted {
                execution_id: exec_id.to_string(),
                iteration: 1,
                exit_code: 0,
                duration_ms: 1000,
            },
            Event::Error {
                execution_id: exec_id.to_string(),
                context: "ctx".to_string(),
                message: "err".to_string(),
            },
            Event::Warning {
                execution_id: exec_id.to_string(),
                context: "ctx".to_string(),
                message: "warn".to_string(),
            },
        ];

        for event in events {
            assert_eq!(
                event.execution_id(),
                exec_id,
                "Event {} should have correct execution_id",
                event.event_type()
            );
        }
    }

    #[test]
    fn test_all_event_types_serialization_roundtrip() {
        let events: Vec<Event> = vec![
            Event::LoopStarted {
                execution_id: "e1".to_string(),
                loop_type: "plan".to_string(),
                task_description: "desc".to_string(),
            },
            Event::PhaseStarted {
                execution_id: "e1".to_string(),
                phase_index: 0,
                phase_name: "phase".to_string(),
                total_phases: 3,
            },
            Event::IterationStarted {
                execution_id: "e1".to_string(),
                iteration: 1,
            },
            Event::IterationCompleted {
                execution_id: "e1".to_string(),
                iteration: 1,
                outcome: IterationOutcome::ValidationFailed { exit_code: 1 },
            },
            Event::LoopCompleted {
                execution_id: "e1".to_string(),
                success: false,
                total_iterations: 5,
            },
            Event::PromptSent {
                execution_id: "e1".to_string(),
                iteration: 1,
                prompt_summary: "Tell me...".to_string(),
                token_count: 500,
            },
            Event::TokenReceived {
                execution_id: "e1".to_string(),
                iteration: 1,
                token: "Hello".to_string(),
            },
            Event::ResponseCompleted {
                execution_id: "e1".to_string(),
                iteration: 1,
                response_summary: "I will...".to_string(),
                input_tokens: 500,
                output_tokens: 200,
                has_tool_calls: true,
            },
            Event::ToolCallStarted {
                execution_id: "e1".to_string(),
                iteration: 1,
                tool_name: "write_file".to_string(),
                tool_args_summary: "{\"path\": \"/tmp/x\"}".to_string(),
            },
            Event::ToolCallCompleted {
                execution_id: "e1".to_string(),
                iteration: 1,
                tool_name: "write_file".to_string(),
                success: true,
                result_summary: "Written 100 bytes".to_string(),
                duration_ms: 50,
            },
            Event::ValidationStarted {
                execution_id: "e1".to_string(),
                iteration: 1,
                command: "make test".to_string(),
            },
            Event::ValidationOutput {
                execution_id: "e1".to_string(),
                iteration: 1,
                line: "PASS: test_foo".to_string(),
                is_stderr: false,
            },
            Event::ValidationCompleted {
                execution_id: "e1".to_string(),
                iteration: 1,
                exit_code: 0,
                duration_ms: 5000,
            },
            Event::Error {
                execution_id: "e1".to_string(),
                context: "llm".to_string(),
                message: "Rate limited".to_string(),
            },
            Event::Warning {
                execution_id: "e1".to_string(),
                context: "validation".to_string(),
                message: "Slow test".to_string(),
            },
        ];

        for event in events {
            let event_type = event.event_type();
            let json = serde_json::to_string(&event).unwrap_or_else(|_| panic!("Failed to serialize {}", event_type));
            let parsed: Event =
                serde_json::from_str(&json).unwrap_or_else(|_| panic!("Failed to deserialize {}", event_type));
            assert_eq!(parsed.event_type(), event_type);
            assert_eq!(parsed.execution_id(), event.execution_id());
        }
    }

    #[test]
    fn test_all_iteration_outcome_variants_serialize() {
        let outcomes = vec![
            IterationOutcome::ValidationPassed,
            IterationOutcome::ValidationFailed { exit_code: 127 },
            IterationOutcome::MaxTurnsReached,
            IterationOutcome::ToolError {
                tool: "read_file".to_string(),
                error: "File not found".to_string(),
            },
            IterationOutcome::LlmError {
                error: "Context length exceeded".to_string(),
            },
        ];

        for outcome in outcomes {
            let event = Event::IterationCompleted {
                execution_id: "test".to_string(),
                iteration: 1,
                outcome: outcome.clone(),
            };
            let json = serde_json::to_string(&event).unwrap();
            let parsed: Event = serde_json::from_str(&json).unwrap();

            if let Event::IterationCompleted {
                outcome: parsed_outcome,
                ..
            } = parsed
            {
                // Verify the outcome type matches
                let expected_type = match outcome {
                    IterationOutcome::ValidationPassed => "ValidationPassed",
                    IterationOutcome::ValidationFailed { .. } => "ValidationFailed",
                    IterationOutcome::MaxTurnsReached => "MaxTurnsReached",
                    IterationOutcome::ToolError { .. } => "ToolError",
                    IterationOutcome::LlmError { .. } => "LlmError",
                };
                let actual_type = match parsed_outcome {
                    IterationOutcome::ValidationPassed => "ValidationPassed",
                    IterationOutcome::ValidationFailed { .. } => "ValidationFailed",
                    IterationOutcome::MaxTurnsReached => "MaxTurnsReached",
                    IterationOutcome::ToolError { .. } => "ToolError",
                    IterationOutcome::LlmError { .. } => "LlmError",
                };
                assert_eq!(expected_type, actual_type);
            } else {
                panic!("Expected IterationCompleted event");
            }
        }
    }

    #[test]
    fn test_event_log_entry_timestamp() {
        let before = Utc::now();
        let event = Event::LoopStarted {
            execution_id: "ts-test".to_string(),
            loop_type: "plan".to_string(),
            task_description: "Test".to_string(),
        };
        let entry = EventLogEntry::new(event);
        let after = Utc::now();

        assert!(entry.timestamp >= before);
        assert!(entry.timestamp <= after);
    }

    #[test]
    fn test_event_log_entry_roundtrip() {
        let event = Event::ToolCallCompleted {
            execution_id: "roundtrip".to_string(),
            iteration: 5,
            tool_name: "bash".to_string(),
            success: false,
            result_summary: "Command failed".to_string(),
            duration_ms: 123,
        };
        let entry = EventLogEntry::new(event);

        let json = serde_json::to_string(&entry).unwrap();
        let parsed: EventLogEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.event.execution_id(), "roundtrip");
        assert_eq!(parsed.event.event_type(), "ToolCallCompleted");
    }
}
