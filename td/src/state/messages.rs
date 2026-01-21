//! State manager messages
//!
//! Commands and responses for the actor pattern.

use serde_json::Value;
use thiserror::Error;
use tokio::sync::oneshot;

use crate::domain::{IterationLog, Loop, LoopExecution};

/// Errors from state operations
#[derive(Debug, Error)]
pub enum StateError {
    #[error("Record not found: {0}")]
    NotFound(String),

    #[error("Store error: {0}")]
    StoreError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    #[error("Channel error")]
    ChannelError,
}

/// Response from state operations
pub type StateResponse<T> = Result<T, StateError>;

/// Commands sent to the StateManager actor
#[derive(Debug)]
pub enum StateCommand {
    // Loop operations (generic work units)
    CreateLoop {
        record: Loop,
        reply: oneshot::Sender<StateResponse<String>>,
    },
    GetLoop {
        id: String,
        reply: oneshot::Sender<StateResponse<Option<Loop>>>,
    },
    UpdateLoop {
        record: Loop,
        reply: oneshot::Sender<StateResponse<()>>,
    },
    ListLoops {
        type_filter: Option<String>,
        status_filter: Option<String>,
        parent_filter: Option<String>,
        reply: oneshot::Sender<StateResponse<Vec<Loop>>>,
    },

    // LoopExecution operations
    CreateExecution {
        execution: LoopExecution,
        reply: oneshot::Sender<StateResponse<String>>,
    },
    GetExecution {
        id: String,
        reply: oneshot::Sender<StateResponse<Option<LoopExecution>>>,
    },
    UpdateExecution {
        execution: LoopExecution,
        reply: oneshot::Sender<StateResponse<()>>,
    },
    ListExecutions {
        status_filter: Option<String>,
        loop_type_filter: Option<String>,
        reply: oneshot::Sender<StateResponse<Vec<LoopExecution>>>,
    },

    // Delete operations
    DeleteLoop {
        id: String,
        reply: oneshot::Sender<StateResponse<()>>,
    },
    DeleteExecution {
        id: String,
        reply: oneshot::Sender<StateResponse<()>>,
    },

    // Generic operations
    GetGeneric {
        collection: String,
        id: String,
        reply: oneshot::Sender<StateResponse<Option<Value>>>,
    },

    // IterationLog operations
    CreateIterationLog {
        log: IterationLog,
        reply: oneshot::Sender<StateResponse<String>>,
    },
    ListIterationLogs {
        execution_id: String,
        reply: oneshot::Sender<StateResponse<Vec<IterationLog>>>,
    },
    GetIterationLog {
        id: String,
        reply: oneshot::Sender<StateResponse<Option<IterationLog>>>,
    },
    DeleteIterationLogs {
        execution_id: String,
        reply: oneshot::Sender<StateResponse<usize>>,
    },

    // Sync operations
    Sync {
        reply: oneshot::Sender<StateResponse<()>>,
    },
    RebuildIndexes {
        reply: oneshot::Sender<StateResponse<usize>>,
    },

    // Shutdown
    Shutdown,
}
