//! Event Bus - central pub/sub system for TaskDaemon events
//!
//! The EventBus uses tokio broadcast channels to deliver events to all subscribers
//! with minimal latency. Components emit events, consumers (TUI, loggers) subscribe.

use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::debug;

use super::types::Event;

/// Default channel capacity (events)
/// At ~100 tokens/second, this provides ~100 seconds of buffer
pub const DEFAULT_CHANNEL_CAPACITY: usize = 10_000;

/// Central event bus for TaskDaemon activity streaming
///
/// Every significant action in TD emits an event to this bus.
/// All consumers (TUI, file logger, database) subscribe to receive events.
pub struct EventBus {
    tx: broadcast::Sender<Event>,
    #[allow(dead_code)]
    channel_capacity: usize,
}

impl EventBus {
    /// Create a new event bus with the given capacity
    pub fn new(capacity: usize) -> Self {
        debug!(capacity, "EventBus::new: creating event bus");
        let (tx, _) = broadcast::channel(capacity);
        Self {
            tx,
            channel_capacity: capacity,
        }
    }

    /// Create a new event bus with default capacity
    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_CHANNEL_CAPACITY)
    }

    /// Emit an event to all subscribers
    ///
    /// This is fire-and-forget: if there are no subscribers, the event is dropped.
    /// If the channel is full, oldest events are dropped.
    pub fn emit(&self, event: Event) {
        debug!(
            event_type = event.event_type(),
            execution_id = event.execution_id(),
            "EventBus::emit"
        );
        // Ignore send errors (no subscribers is OK)
        let _ = self.tx.send(event);
    }

    /// Subscribe to receive events
    ///
    /// Returns a receiver that will receive all events emitted after subscription.
    /// Note: Events emitted before subscription are not received.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        debug!("EventBus::subscribe: new subscriber");
        self.tx.subscribe()
    }

    /// Create an emitter handle for a specific execution
    ///
    /// The emitter provides convenience methods for emitting events
    /// and automatically includes the execution ID.
    pub fn emitter_for(&self, execution_id: impl Into<String>) -> EventEmitter {
        let execution_id = execution_id.into();
        debug!(%execution_id, "EventBus::emitter_for: creating emitter");
        EventEmitter {
            tx: self.tx.clone(),
            execution_id,
        }
    }

    /// Get the number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::with_default_capacity()
    }
}

/// Handle for components to emit events without owning the bus
///
/// EventEmitter is cheap to clone and provides convenience methods
/// for emitting events with a pre-set execution ID.
#[derive(Clone)]
pub struct EventEmitter {
    tx: broadcast::Sender<Event>,
    execution_id: String,
}

impl EventEmitter {
    /// Get the execution ID this emitter is bound to
    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }

    /// Emit a raw event
    pub fn emit(&self, event: Event) {
        debug!(event_type = event.event_type(), "EventEmitter::emit");
        let _ = self.tx.send(event);
    }

    // === Convenience methods ===

    /// Emit a loop started event
    pub fn loop_started(&self, loop_type: &str, task_description: &str) {
        self.emit(Event::LoopStarted {
            execution_id: self.execution_id.clone(),
            loop_type: loop_type.to_string(),
            task_description: task_description.to_string(),
        });
    }

    /// Emit a phase started event
    pub fn phase_started(&self, phase_index: usize, phase_name: &str, total_phases: usize) {
        self.emit(Event::PhaseStarted {
            execution_id: self.execution_id.clone(),
            phase_index,
            phase_name: phase_name.to_string(),
            total_phases,
        });
    }

    /// Emit an iteration started event
    pub fn iteration_started(&self, iteration: u32) {
        self.emit(Event::IterationStarted {
            execution_id: self.execution_id.clone(),
            iteration,
        });
    }

    /// Emit an iteration completed event
    pub fn iteration_completed(&self, iteration: u32, outcome: super::types::IterationOutcome) {
        self.emit(Event::IterationCompleted {
            execution_id: self.execution_id.clone(),
            iteration,
            outcome,
        });
    }

    /// Emit a loop completed event
    pub fn loop_completed(&self, success: bool, total_iterations: u32) {
        self.emit(Event::LoopCompleted {
            execution_id: self.execution_id.clone(),
            success,
            total_iterations,
        });
    }

    /// Emit a prompt sent event
    pub fn prompt_sent(&self, iteration: u32, summary: &str, token_count: u64) {
        self.emit(Event::PromptSent {
            execution_id: self.execution_id.clone(),
            iteration,
            prompt_summary: summary.to_string(),
            token_count,
        });
    }

    /// Emit a token received event (streaming)
    pub fn token_received(&self, iteration: u32, token: &str) {
        self.emit(Event::TokenReceived {
            execution_id: self.execution_id.clone(),
            iteration,
            token: token.to_string(),
        });
    }

    /// Emit a response completed event
    pub fn response_completed(
        &self,
        iteration: u32,
        summary: &str,
        input_tokens: u64,
        output_tokens: u64,
        has_tool_calls: bool,
    ) {
        self.emit(Event::ResponseCompleted {
            execution_id: self.execution_id.clone(),
            iteration,
            response_summary: summary.to_string(),
            input_tokens,
            output_tokens,
            has_tool_calls,
        });
    }

    /// Emit a tool call started event
    pub fn tool_call_started(&self, iteration: u32, tool_name: &str, args_summary: &str) {
        self.emit(Event::ToolCallStarted {
            execution_id: self.execution_id.clone(),
            iteration,
            tool_name: tool_name.to_string(),
            tool_args_summary: args_summary.to_string(),
        });
    }

    /// Emit a tool call completed event
    pub fn tool_call_completed(
        &self,
        iteration: u32,
        tool_name: &str,
        success: bool,
        result_summary: &str,
        duration_ms: u64,
    ) {
        self.emit(Event::ToolCallCompleted {
            execution_id: self.execution_id.clone(),
            iteration,
            tool_name: tool_name.to_string(),
            success,
            result_summary: result_summary.to_string(),
            duration_ms,
        });
    }

    /// Emit a validation started event
    pub fn validation_started(&self, iteration: u32, command: &str) {
        self.emit(Event::ValidationStarted {
            execution_id: self.execution_id.clone(),
            iteration,
            command: command.to_string(),
        });
    }

    /// Emit a validation output line event (streaming)
    pub fn validation_output(&self, iteration: u32, line: &str, is_stderr: bool) {
        self.emit(Event::ValidationOutput {
            execution_id: self.execution_id.clone(),
            iteration,
            line: line.to_string(),
            is_stderr,
        });
    }

    /// Emit a validation completed event
    pub fn validation_completed(&self, iteration: u32, exit_code: i32, duration_ms: u64) {
        self.emit(Event::ValidationCompleted {
            execution_id: self.execution_id.clone(),
            iteration,
            exit_code,
            duration_ms,
        });
    }

    /// Emit an error event
    pub fn error(&self, context: &str, message: &str) {
        self.emit(Event::Error {
            execution_id: self.execution_id.clone(),
            context: context.to_string(),
            message: message.to_string(),
        });
    }

    /// Emit a warning event
    pub fn warning(&self, context: &str, message: &str) {
        self.emit(Event::Warning {
            execution_id: self.execution_id.clone(),
            context: context.to_string(),
            message: message.to_string(),
        });
    }
}

/// Create an event bus wrapped in an Arc for shared ownership
pub fn create_event_bus() -> Arc<EventBus> {
    Arc::new(EventBus::with_default_capacity())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast::error::TryRecvError;

    #[test]
    fn test_event_bus_creation() {
        let bus = EventBus::new(100);
        assert_eq!(bus.subscriber_count(), 0);
    }

    #[test]
    fn test_event_bus_subscribe() {
        let bus = EventBus::new(100);
        let _rx1 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 1);
        let _rx2 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);
    }

    #[tokio::test]
    async fn test_event_bus_emit_receive() {
        let bus = EventBus::new(100);
        let mut rx = bus.subscribe();

        bus.emit(Event::LoopStarted {
            execution_id: "test-123".to_string(),
            loop_type: "plan".to_string(),
            task_description: "Test task".to_string(),
        });

        let event = rx.recv().await.unwrap();
        assert_eq!(event.execution_id(), "test-123");
        assert_eq!(event.event_type(), "LoopStarted");
    }

    #[tokio::test]
    async fn test_event_bus_no_subscribers() {
        let bus = EventBus::new(100);
        // This should not panic even with no subscribers
        bus.emit(Event::LoopStarted {
            execution_id: "test-123".to_string(),
            loop_type: "plan".to_string(),
            task_description: "Test task".to_string(),
        });
    }

    #[tokio::test]
    async fn test_event_emitter() {
        let bus = EventBus::new(100);
        let mut rx = bus.subscribe();
        let emitter = bus.emitter_for("exec-456");

        emitter.loop_started("plan", "Build something");

        let event = rx.recv().await.unwrap();
        assert_eq!(event.execution_id(), "exec-456");
        match event {
            Event::LoopStarted {
                loop_type,
                task_description,
                ..
            } => {
                assert_eq!(loop_type, "plan");
                assert_eq!(task_description, "Build something");
            }
            _ => panic!("Expected LoopStarted event"),
        }
    }

    #[tokio::test]
    async fn test_event_emitter_convenience_methods() {
        let bus = EventBus::new(100);
        let mut rx = bus.subscribe();
        let emitter = bus.emitter_for("exec-789");

        // Test various convenience methods
        emitter.iteration_started(1);
        emitter.prompt_sent(1, "Hello LLM", 100);
        emitter.token_received(1, "Hello");
        emitter.tool_call_started(1, "read_file", "path: /foo");
        emitter.tool_call_completed(1, "read_file", true, "file contents", 50);
        emitter.validation_started(1, "cargo test");
        emitter.validation_output(1, "running 5 tests", false);
        emitter.validation_completed(1, 0, 1000);
        emitter.iteration_completed(1, super::super::types::IterationOutcome::ValidationPassed);

        // Verify we received 9 events
        for _ in 0..9 {
            let event = rx.recv().await.unwrap();
            assert_eq!(event.execution_id(), "exec-789");
        }

        // No more events
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = EventBus::new(100);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        bus.emit(Event::LoopStarted {
            execution_id: "test".to_string(),
            loop_type: "plan".to_string(),
            task_description: "Test".to_string(),
        });

        // Both subscribers should receive the event
        let event1 = rx1.recv().await.unwrap();
        let event2 = rx2.recv().await.unwrap();

        assert_eq!(event1.execution_id(), "test");
        assert_eq!(event2.execution_id(), "test");
    }

    #[tokio::test]
    async fn test_full_loop_lifecycle_events() {
        // Simulates a complete loop execution and verifies all events are emitted in order
        let bus = EventBus::new(100);
        let mut rx = bus.subscribe();
        let emitter = bus.emitter_for("lifecycle-test");

        // Simulate a full loop: start -> iteration -> prompt -> response -> tool -> validation -> complete
        emitter.loop_started("ralph", "Implement feature X");
        emitter.phase_started(0, "Implementation", 1);
        emitter.iteration_started(1);
        emitter.prompt_sent(1, "Please implement...", 500);
        emitter.token_received(1, "I'll");
        emitter.token_received(1, " start");
        emitter.token_received(1, " by");
        emitter.response_completed(1, "I'll start by...", 500, 100, true);
        emitter.tool_call_started(1, "write_file", "path: src/main.rs");
        emitter.tool_call_completed(1, "write_file", true, "File written", 50);
        emitter.validation_started(1, "cargo test");
        emitter.validation_output(1, "running 3 tests", false);
        emitter.validation_output(1, "test result: ok", false);
        emitter.validation_completed(1, 0, 2000);
        emitter.iteration_completed(1, super::super::types::IterationOutcome::ValidationPassed);
        emitter.loop_completed(true, 1);

        // Collect all events and verify sequence
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event.event_type().to_string());
        }

        assert_eq!(
            events,
            vec![
                "LoopStarted",
                "PhaseStarted",
                "IterationStarted",
                "PromptSent",
                "TokenReceived",
                "TokenReceived",
                "TokenReceived",
                "ResponseCompleted",
                "ToolCallStarted",
                "ToolCallCompleted",
                "ValidationStarted",
                "ValidationOutput",
                "ValidationOutput",
                "ValidationCompleted",
                "IterationCompleted",
                "LoopCompleted",
            ]
        );
    }

    #[tokio::test]
    async fn test_token_streaming_high_volume() {
        // Verify many tokens can be emitted and received without loss
        let bus = EventBus::new(1000);
        let mut rx = bus.subscribe();
        let emitter = bus.emitter_for("stream-test");

        // Emit 100 tokens
        for i in 0..100 {
            emitter.token_received(1, &format!("token{}", i));
        }

        // Verify all 100 received
        let mut count = 0;
        while let Ok(event) = rx.try_recv() {
            assert_eq!(event.event_type(), "TokenReceived");
            count += 1;
        }
        assert_eq!(count, 100);
    }

    #[tokio::test]
    async fn test_lagged_subscriber_continues() {
        // When a subscriber falls behind, it gets a Lagged error but can continue
        let bus = EventBus::new(5); // Very small capacity
        let mut rx = bus.subscribe();

        // Emit more events than capacity
        for i in 0..10 {
            bus.emit(Event::TokenReceived {
                execution_id: "lag-test".to_string(),
                iteration: 1,
                token: format!("t{}", i),
            });
        }

        // First recv may get Lagged error
        let result = rx.recv().await;
        // Either we get an event or a Lagged error - both are acceptable
        match result {
            Ok(event) => assert_eq!(event.event_type(), "TokenReceived"),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                assert!(n > 0, "Should have missed some events");
                // Can still receive subsequent events
                let event = rx.recv().await.unwrap();
                assert_eq!(event.event_type(), "TokenReceived");
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_error_and_warning_events() {
        let bus = EventBus::new(100);
        let mut rx = bus.subscribe();
        let emitter = bus.emitter_for("error-test");

        emitter.warning("validation", "Timeout approaching");
        emitter.error("llm", "Rate limit exceeded");

        let warning = rx.recv().await.unwrap();
        assert_eq!(warning.event_type(), "Warning");
        if let Event::Warning { context, message, .. } = warning {
            assert_eq!(context, "validation");
            assert_eq!(message, "Timeout approaching");
        } else {
            panic!("Expected Warning event");
        }

        let error = rx.recv().await.unwrap();
        assert_eq!(error.event_type(), "Error");
        if let Event::Error { context, message, .. } = error {
            assert_eq!(context, "llm");
            assert_eq!(message, "Rate limit exceeded");
        } else {
            panic!("Expected Error event");
        }
    }

    #[tokio::test]
    async fn test_all_iteration_outcomes() {
        use super::super::types::IterationOutcome;

        let bus = EventBus::new(100);
        let mut rx = bus.subscribe();
        let emitter = bus.emitter_for("outcome-test");

        // Test all outcome variants
        emitter.iteration_completed(1, IterationOutcome::ValidationPassed);
        emitter.iteration_completed(2, IterationOutcome::ValidationFailed { exit_code: 1 });
        emitter.iteration_completed(3, IterationOutcome::MaxTurnsReached);
        emitter.iteration_completed(
            4,
            IterationOutcome::ToolError {
                tool: "write_file".to_string(),
                error: "Permission denied".to_string(),
            },
        );
        emitter.iteration_completed(
            5,
            IterationOutcome::LlmError {
                error: "Context too long".to_string(),
            },
        );

        // Verify all received with correct iterations
        for expected_iter in 1..=5 {
            let event = rx.recv().await.unwrap();
            if let Event::IterationCompleted { iteration, .. } = event {
                assert_eq!(iteration, expected_iter);
            } else {
                panic!("Expected IterationCompleted");
            }
        }
    }

    #[tokio::test]
    async fn test_multiple_executions_interleaved() {
        // Verify events from different executions can be distinguished
        let bus = EventBus::new(100);
        let mut rx = bus.subscribe();

        let emitter_a = bus.emitter_for("exec-A");
        let emitter_b = bus.emitter_for("exec-B");

        // Interleave events from two executions
        emitter_a.iteration_started(1);
        emitter_b.iteration_started(1);
        emitter_a.token_received(1, "Hello from A");
        emitter_b.token_received(1, "Hello from B");
        emitter_a.iteration_completed(1, super::super::types::IterationOutcome::ValidationPassed);
        emitter_b.iteration_completed(
            1,
            super::super::types::IterationOutcome::ValidationFailed { exit_code: 1 },
        );

        // Collect events grouped by execution
        let mut exec_a_events = Vec::new();
        let mut exec_b_events = Vec::new();

        while let Ok(event) = rx.try_recv() {
            match event.execution_id() {
                "exec-A" => exec_a_events.push(event.event_type().to_string()),
                "exec-B" => exec_b_events.push(event.event_type().to_string()),
                _ => panic!("Unexpected execution_id"),
            }
        }

        assert_eq!(
            exec_a_events,
            vec!["IterationStarted", "TokenReceived", "IterationCompleted"]
        );
        assert_eq!(
            exec_b_events,
            vec!["IterationStarted", "TokenReceived", "IterationCompleted"]
        );
    }

    #[tokio::test]
    async fn test_emitter_execution_id_accessor() {
        let bus = EventBus::new(100);
        let emitter = bus.emitter_for("my-execution");
        assert_eq!(emitter.execution_id(), "my-execution");
    }

    #[tokio::test]
    async fn test_validation_stderr_flag() {
        let bus = EventBus::new(100);
        let mut rx = bus.subscribe();
        let emitter = bus.emitter_for("stderr-test");

        emitter.validation_output(1, "stdout line", false);
        emitter.validation_output(1, "stderr line", true);

        let stdout_event = rx.recv().await.unwrap();
        if let Event::ValidationOutput { is_stderr, line, .. } = stdout_event {
            assert!(!is_stderr);
            assert_eq!(line, "stdout line");
        } else {
            panic!("Expected ValidationOutput");
        }

        let stderr_event = rx.recv().await.unwrap();
        if let Event::ValidationOutput { is_stderr, line, .. } = stderr_event {
            assert!(is_stderr);
            assert_eq!(line, "stderr line");
        } else {
            panic!("Expected ValidationOutput");
        }
    }

    #[test]
    fn test_default_channel_capacity() {
        assert_eq!(DEFAULT_CHANNEL_CAPACITY, 10_000);
    }

    #[test]
    fn test_event_bus_default() {
        let bus = EventBus::default();
        assert_eq!(bus.subscriber_count(), 0);
    }

    #[test]
    fn test_create_event_bus_helper() {
        let bus = create_event_bus();
        assert_eq!(bus.subscriber_count(), 0);
    }
}
