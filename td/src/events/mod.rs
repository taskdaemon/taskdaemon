//! Event Bus Architecture for Live Observability
//!
//! This module provides the event system for real-time visibility into TaskDaemon's
//! agentic loops. Every significant action emits an event. All consumers (TUI, file
//! logger, database) subscribe to the bus.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                       EVENT BUS                              │
//! │            (tokio::sync::broadcast channel)                  │
//! │                                                              │
//! │  Every action emits an event. Every consumer subscribes.    │
//! └─────────────────────────────────────────────────────────────┘
//!         ↑               ↑               ↑               ↑
//!    LLM Client      Tool Executor    Validation      Loop Engine
//!    emits:          emits:           emits:          emits:
//!    - PromptSent    - ToolStarted    - Started       - PhaseStarted
//!    - TokenDelta    - ToolCompleted  - OutputLine    - IterationStarted
//!    - ResponseDone  - ToolFailed     - Completed     - IterationCompleted
//!
//!         ↓               ↓               ↓               ↓
//! ┌───────────┐   ┌───────────┐   ┌───────────┐   ┌───────────┐
//! │ TUI Live  │   │ File Log  │   │ Database  │   │ Metrics   │
//! │ Streaming │   │ .jsonl    │   │ (history) │   │ (future)  │
//! └───────────┘   └───────────┘   └───────────┘   └───────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use td::events::{EventBus, EventEmitter, TdEvent};
//! use std::sync::Arc;
//!
//! // Create event bus (typically at app startup)
//! let event_bus = Arc::new(EventBus::with_default_capacity());
//!
//! // Get emitter for a specific execution
//! let emitter = event_bus.emitter_for("execution-123");
//!
//! // Emit events using convenience methods
//! emitter.loop_started("plan", "Build authentication feature");
//! emitter.iteration_started(1);
//! emitter.prompt_sent(1, "Implement login...", 500);
//! emitter.token_received(1, "I'll");
//!
//! // Subscribe to events (for TUI, loggers, etc.)
//! let mut rx = event_bus.subscribe();
//! while let Ok(event) = rx.recv().await {
//!     println!("Event: {:?}", event);
//! }
//! ```
//!
//! # Event Types
//!
//! See [`TdEvent`] for the complete list of events:
//! - Loop lifecycle: `LoopStarted`, `PhaseStarted`, `IterationStarted`, etc.
//! - LLM interactions: `PromptSent`, `TokenReceived`, `ResponseCompleted`
//! - Tool execution: `ToolCallStarted`, `ToolCallCompleted`
//! - Validation: `ValidationStarted`, `ValidationOutput`, `ValidationCompleted`
//! - Errors: `Error`, `Warning`

mod bus;
mod logger;
mod types;

pub use bus::{DEFAULT_CHANNEL_CAPACITY, EventBus, EventEmitter, create_event_bus};
pub use logger::{EventLogger, read_execution_events, replay_execution_events, spawn_event_logger};
pub use types::{EventLogEntry, IterationOutcome, TdEvent};
