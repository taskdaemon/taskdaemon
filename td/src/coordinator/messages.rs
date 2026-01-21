//! Message types for the Coordinator

use eyre::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// Messages sent to loops from the Coordinator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoordMessage {
    /// Notification broadcast to subscribers
    Notification {
        #[serde(rename = "from-exec-id")]
        from_exec_id: String,
        #[serde(rename = "event-type")]
        event_type: String,
        data: serde_json::Value,
    },

    /// Query from another loop
    Query {
        #[serde(rename = "query-id")]
        query_id: String,
        #[serde(rename = "from-exec-id")]
        from_exec_id: String,
        question: String,
    },

    /// Shared data from another loop
    Share {
        #[serde(rename = "from-exec-id")]
        from_exec_id: String,
        #[serde(rename = "share-type")]
        share_type: String,
        data: serde_json::Value,
    },

    /// Request to stop gracefully
    Stop {
        #[serde(rename = "from-exec-id")]
        from_exec_id: String,
        reason: String,
    },
}

/// Internal requests to the Coordinator task
#[derive(Debug)]
pub enum CoordRequest {
    /// Register a new loop execution
    Register {
        exec_id: String,
        tx: mpsc::Sender<CoordMessage>,
    },

    /// Unregister a loop execution
    Unregister { exec_id: String },

    /// Broadcast an alert to subscribers
    Alert {
        from_exec_id: String,
        event_type: String,
        data: serde_json::Value,
    },

    /// Send a query to a specific execution
    Query {
        query_id: String,
        from_exec_id: String,
        target_exec_id: String,
        question: String,
        reply_tx: oneshot::Sender<Result<String>>,
        timeout: Duration,
    },

    /// Reply to a query
    QueryReply { query_id: String, answer: String },

    /// Cancel a pending query
    QueryCancel { query_id: String },

    /// Share data with a specific execution
    Share {
        from_exec_id: String,
        target_exec_id: String,
        share_type: String,
        data: serde_json::Value,
    },

    /// Subscribe to an event type
    Subscribe { exec_id: String, event_type: String },

    /// Unsubscribe from an event type
    Unsubscribe { exec_id: String, event_type: String },

    /// Request an execution to stop
    Stop {
        from_exec_id: String,
        target_exec_id: String,
        reason: String,
    },

    /// Query timeout notification (internal)
    QueryTimeout { query_id: String },

    /// Get current metrics
    GetMetrics {
        reply_tx: oneshot::Sender<CoordinatorMetrics>,
    },

    /// Shutdown the coordinator
    Shutdown,
}

/// Payload structure for queries (for persistence)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPayload {
    #[serde(rename = "query-id")]
    pub query_id: String,
    pub question: String,
    #[serde(rename = "timeout-ms")]
    pub timeout_ms: u64,
}

/// Payload structure for alerts (for persistence)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertPayload {
    #[serde(rename = "event-type")]
    pub event_type: String,
    pub data: serde_json::Value,
}

/// Payload structure for shares (for persistence)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharePayload {
    #[serde(rename = "share-type")]
    pub share_type: String,
    pub data: serde_json::Value,
}

/// Coordinator metrics for observability
#[derive(Debug, Clone, Default)]
pub struct CoordinatorMetrics {
    pub registered_executions: usize,
    pub pending_queries: usize,
    pub total_subscriptions: usize,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub query_timeouts: u64,
    pub rate_limit_violations: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coord_message_serialization() {
        let msg = CoordMessage::Notification {
            from_exec_id: "exec-001".to_string(),
            event_type: "phase_complete".to_string(),
            data: serde_json::json!({"phase": "Phase 1"}),
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("from-exec-id"));
        assert!(json.contains("event-type"));

        let deserialized: CoordMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            CoordMessage::Notification {
                from_exec_id,
                event_type,
                ..
            } => {
                assert_eq!(from_exec_id, "exec-001");
                assert_eq!(event_type, "phase_complete");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_query_payload_serialization() {
        let payload = QueryPayload {
            query_id: "query-123".to_string(),
            question: "What is the API URL?".to_string(),
            timeout_ms: 30000,
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("query-id"));
        assert!(json.contains("timeout-ms"));
    }
}
