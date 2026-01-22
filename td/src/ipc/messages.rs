//! IPC message types for daemon communication
//!
//! Simple JSON-over-newline protocol. Each message is a single line of JSON followed by `\n`.

use serde::{Deserialize, Serialize};

/// Messages from TUI/CLI to Daemon
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum DaemonMessage {
    /// Notify daemon that an execution is pending and should be picked up
    ExecutionPending { id: String },

    /// Notify daemon that an execution was resumed and should be spawned
    ExecutionResumed { id: String },

    /// Ping to check if daemon is alive
    Ping,

    /// Request daemon to stop gracefully
    Shutdown,
}

/// Responses from Daemon to TUI/CLI
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum DaemonResponse {
    /// Acknowledgment
    Ok,

    /// Pong response to ping
    Pong { version: String },

    /// Error response
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_pending_serialize() {
        let msg = DaemonMessage::ExecutionPending {
            id: "exec-123".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"ExecutionPending","id":"exec-123"}"#);
    }

    #[test]
    fn test_execution_pending_deserialize() {
        let json = r#"{"type":"ExecutionPending","id":"exec-456"}"#;
        let msg: DaemonMessage = serde_json::from_str(json).unwrap();
        assert_eq!(
            msg,
            DaemonMessage::ExecutionPending {
                id: "exec-456".to_string()
            }
        );
    }

    #[test]
    fn test_execution_resumed_serialize() {
        let msg = DaemonMessage::ExecutionResumed {
            id: "exec-789".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"ExecutionResumed","id":"exec-789"}"#);
    }

    #[test]
    fn test_ping_serialize() {
        let msg = DaemonMessage::Ping;
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"Ping"}"#);
    }

    #[test]
    fn test_shutdown_serialize() {
        let msg = DaemonMessage::Shutdown;
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"Shutdown"}"#);
    }

    #[test]
    fn test_ok_response_serialize() {
        let resp = DaemonResponse::Ok;
        let json = serde_json::to_string(&resp).unwrap();
        assert_eq!(json, r#"{"type":"Ok"}"#);
    }

    #[test]
    fn test_pong_response_serialize() {
        let resp = DaemonResponse::Pong {
            version: "1.0.0".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert_eq!(json, r#"{"type":"Pong","version":"1.0.0"}"#);
    }

    #[test]
    fn test_error_response_serialize() {
        let resp = DaemonResponse::Error {
            message: "Something went wrong".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert_eq!(json, r#"{"type":"Error","message":"Something went wrong"}"#);
    }

    #[test]
    fn test_roundtrip_all_messages() {
        let messages = vec![
            DaemonMessage::ExecutionPending { id: "test".to_string() },
            DaemonMessage::ExecutionResumed { id: "test".to_string() },
            DaemonMessage::Ping,
            DaemonMessage::Shutdown,
        ];

        for msg in messages {
            let json = serde_json::to_string(&msg).unwrap();
            let parsed: DaemonMessage = serde_json::from_str(&json).unwrap();
            assert_eq!(msg, parsed);
        }
    }

    #[test]
    fn test_roundtrip_all_responses() {
        let responses = vec![
            DaemonResponse::Ok,
            DaemonResponse::Pong {
                version: "v1.2.3".to_string(),
            },
            DaemonResponse::Error {
                message: "test error".to_string(),
            },
        ];

        for resp in responses {
            let json = serde_json::to_string(&resp).unwrap();
            let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
            assert_eq!(resp, parsed);
        }
    }
}
