//! Event Bus - central pub/sub system for TaskDaemon events
//!
//! The EventBus uses tokio broadcast channels to deliver events to all subscribers
//! with minimal latency. Components emit events, consumers (TUI, loggers) subscribe.

use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::debug;

use super::types::TdEvent;

/// Default channel capacity (events)
/// At ~100 tokens/second, this provides ~100 seconds of buffer
pub const DEFAULT_CHANNEL_CAPACITY: usize = 10_000;

/// Central event bus for TaskDaemon activity streaming
///
/// Every significant action in TD emits an event to this bus.
/// All consumers (TUI, file logger, database) subscribe to receive events.
pub struct EventBus {
    tx: broadcast::Sender<TdEvent>,
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
    pub fn emit(&self, event: TdEvent) {
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
    pub fn subscribe(&self) -> broadcast::Receiver<TdEvent> {
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
    tx: broadcast::Sender<TdEvent>,
    execution_id: String,
}

impl EventEmitter {
    /// Get the execution ID this emitter is bound to
    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }

    /// Emit a raw event
    pub fn emit(&self, event: TdEvent) {
        debug!(event_type = event.event_type(), "EventEmitter::emit");
        let _ = self.tx.send(event);
    }

    // === Convenience methods ===

    /// Emit a loop started event
    pub fn loop_started(&self, loop_type: &str, task_description: &str) {
        self.emit(TdEvent::LoopStarted {
            execution_id: self.execution_id.clone(),
            loop_type: loop_type.to_string(),
            task_description: task_description.to_string(),
        });
    }

    /// Emit a phase started event
    pub fn phase_started(&self, phase_index: usize, phase_name: &str, total_phases: usize) {
        self.emit(TdEvent::PhaseStarted {
            execution_id: self.execution_id.clone(),
            phase_index,
            phase_name: phase_name.to_string(),
            total_phases,
        });
    }

    /// Emit an iteration started event
    pub fn iteration_started(&self, iteration: u32) {
        self.emit(TdEvent::IterationStarted {
            execution_id: self.execution_id.clone(),
            iteration,
        });
    }

    /// Emit an iteration completed event
    pub fn iteration_completed(&self, iteration: u32, outcome: super::types::IterationOutcome) {
        self.emit(TdEvent::IterationCompleted {
            execution_id: self.execution_id.clone(),
            iteration,
            outcome,
        });
    }

    /// Emit a loop completed event
    pub fn loop_completed(&self, success: bool, total_iterations: u32) {
        self.emit(TdEvent::LoopCompleted {
            execution_id: self.execution_id.clone(),
            success,
            total_iterations,
        });
    }

    /// Emit a prompt sent event
    pub fn prompt_sent(&self, iteration: u32, summary: &str, token_count: u64) {
        self.emit(TdEvent::PromptSent {
            execution_id: self.execution_id.clone(),
            iteration,
            prompt_summary: summary.to_string(),
            token_count,
        });
    }

    /// Emit a token received event (streaming)
    pub fn token_received(&self, iteration: u32, token: &str) {
        self.emit(TdEvent::TokenReceived {
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
        self.emit(TdEvent::ResponseCompleted {
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
        self.emit(TdEvent::ToolCallStarted {
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
        self.emit(TdEvent::ToolCallCompleted {
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
        self.emit(TdEvent::ValidationStarted {
            execution_id: self.execution_id.clone(),
            iteration,
            command: command.to_string(),
        });
    }

    /// Emit a validation output line event (streaming)
    pub fn validation_output(&self, iteration: u32, line: &str, is_stderr: bool) {
        self.emit(TdEvent::ValidationOutput {
            execution_id: self.execution_id.clone(),
            iteration,
            line: line.to_string(),
            is_stderr,
        });
    }

    /// Emit a validation completed event
    pub fn validation_completed(&self, iteration: u32, exit_code: i32, duration_ms: u64) {
        self.emit(TdEvent::ValidationCompleted {
            execution_id: self.execution_id.clone(),
            iteration,
            exit_code,
            duration_ms,
        });
    }

    /// Emit an error event
    pub fn error(&self, context: &str, message: &str) {
        self.emit(TdEvent::Error {
            execution_id: self.execution_id.clone(),
            context: context.to_string(),
            message: message.to_string(),
        });
    }

    /// Emit a warning event
    pub fn warning(&self, context: &str, message: &str) {
        self.emit(TdEvent::Warning {
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

        bus.emit(TdEvent::LoopStarted {
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
        bus.emit(TdEvent::LoopStarted {
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
            TdEvent::LoopStarted {
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

        bus.emit(TdEvent::LoopStarted {
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
}
