//! State manager messages
//!
//! Commands and responses for the actor pattern.

use serde_json::Value;
use thiserror::Error;
use tokio::sync::oneshot;

use crate::domain::{LoopExecution, Plan, Spec};

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
    // Plan operations
    CreatePlan {
        plan: Plan,
        reply: oneshot::Sender<StateResponse<String>>,
    },
    GetPlan {
        id: String,
        reply: oneshot::Sender<StateResponse<Option<Plan>>>,
    },
    UpdatePlan {
        plan: Plan,
        reply: oneshot::Sender<StateResponse<()>>,
    },
    ListPlans {
        status_filter: Option<String>,
        reply: oneshot::Sender<StateResponse<Vec<Plan>>>,
    },

    // Spec operations
    CreateSpec {
        spec: Spec,
        reply: oneshot::Sender<StateResponse<String>>,
    },
    GetSpec {
        id: String,
        reply: oneshot::Sender<StateResponse<Option<Spec>>>,
    },
    UpdateSpec {
        spec: Spec,
        reply: oneshot::Sender<StateResponse<()>>,
    },
    ListSpecs {
        parent_filter: Option<String>,
        status_filter: Option<String>,
        reply: oneshot::Sender<StateResponse<Vec<Spec>>>,
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

    // Generic operations
    GetGeneric {
        collection: String,
        id: String,
        reply: oneshot::Sender<StateResponse<Option<Value>>>,
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
