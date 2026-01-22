//! IPC listener for the daemon side
//!
//! Provides helpers for creating and managing the Unix Domain Socket listener.

use std::path::PathBuf;

use eyre::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, warn};

use super::get_socket_path;
use super::messages::{DaemonMessage, DaemonResponse};

/// Maximum message size (1KB as per design doc)
const MAX_MESSAGE_SIZE: usize = 1024;

/// Create and bind a Unix Domain Socket listener for the daemon
///
/// Handles cleanup of stale socket files from previous runs.
pub fn create_listener() -> Result<(UnixListener, PathBuf)> {
    let socket_path = get_socket_path();
    create_listener_at(&socket_path)
}

/// Create a listener at a specific path (for testing)
pub fn create_listener_at(socket_path: &PathBuf) -> Result<(UnixListener, PathBuf)> {
    debug!(?socket_path, "create_listener: creating IPC socket");

    // Ensure parent directory exists
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create socket directory")?;
    }

    // Clean up stale socket if exists
    if socket_path.exists() {
        debug!(?socket_path, "create_listener: removing stale socket");
        std::fs::remove_file(socket_path).context("Failed to remove stale socket")?;
    }

    // Bind the socket
    let listener = UnixListener::bind(socket_path).context("Failed to bind IPC socket")?;
    debug!(?socket_path, "create_listener: socket bound successfully");

    Ok((listener, socket_path.clone()))
}

/// Remove the socket file on shutdown
pub fn cleanup_socket(socket_path: &PathBuf) {
    if socket_path.exists() {
        debug!(?socket_path, "cleanup_socket: removing socket file");
        if let Err(e) = std::fs::remove_file(socket_path) {
            warn!(?socket_path, error = %e, "Failed to remove socket file");
        }
    }
}

/// Handle a single IPC connection
///
/// Reads a message, processes it, and sends a response.
/// Returns the parsed message for the caller to handle.
pub async fn read_message(stream: &mut UnixStream) -> Result<DaemonMessage> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    // Read with size limit
    let bytes_read = reader
        .read_line(&mut line)
        .await
        .context("Failed to read IPC message")?;

    if bytes_read > MAX_MESSAGE_SIZE {
        return Err(eyre::eyre!("Message too large: {} bytes", bytes_read));
    }

    if line.is_empty() {
        return Err(eyre::eyre!("Empty message received"));
    }

    let msg: DaemonMessage = serde_json::from_str(line.trim()).context("Failed to parse IPC message")?;
    debug!(?msg, "read_message: parsed message");

    Ok(msg)
}

/// Send a response on the stream
pub async fn send_response(stream: &mut UnixStream, response: DaemonResponse) -> Result<()> {
    let response_json = serde_json::to_string(&response).context("Failed to serialize response")?;
    stream
        .write_all(response_json.as_bytes())
        .await
        .context("Failed to write response")?;
    stream.write_all(b"\n").await.context("Failed to write newline")?;
    stream.flush().await.context("Failed to flush response")?;
    debug!(?response, "send_response: sent response");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_listener_creates_parent_dir() {
        let temp = TempDir::new().unwrap();
        let socket_path = temp.path().join("subdir").join("daemon.sock");

        let result = create_listener_at(&socket_path);
        assert!(result.is_ok());

        let (_, path) = result.unwrap();
        assert_eq!(path, socket_path);
        assert!(socket_path.exists());
    }

    #[tokio::test]
    async fn test_create_listener_removes_stale_socket() {
        let temp = TempDir::new().unwrap();
        let socket_path = temp.path().join("daemon.sock");

        // Create a stale file
        std::fs::write(&socket_path, "stale").unwrap();

        let result = create_listener_at(&socket_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_socket_removes_file() {
        let temp = TempDir::new().unwrap();
        let socket_path = temp.path().join("daemon.sock");

        // Create a file
        std::fs::write(&socket_path, "test").unwrap();
        assert!(socket_path.exists());

        cleanup_socket(&socket_path);
        assert!(!socket_path.exists());
    }

    #[test]
    fn test_cleanup_socket_handles_missing_file() {
        let temp = TempDir::new().unwrap();
        let socket_path = temp.path().join("nonexistent.sock");

        // Should not panic
        cleanup_socket(&socket_path);
    }

    #[tokio::test]
    async fn test_end_to_end_ping_pong() {
        use super::super::client::DaemonClient;
        use super::super::messages::{DaemonMessage, DaemonResponse};
        use std::time::Duration;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let temp = TempDir::new().unwrap();
        let socket_path = temp.path().join("test.sock");

        // Create listener
        let (listener, _) = create_listener_at(&socket_path).unwrap();

        // Spawn a mock daemon that responds to ping
        let mock_daemon = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();

            // Read message
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();

            let msg: DaemonMessage = serde_json::from_str(line.trim()).unwrap();
            assert!(matches!(msg, DaemonMessage::Ping));

            // Send response
            let response = DaemonResponse::Pong {
                version: "test-version".to_string(),
            };
            let response_json = serde_json::to_string(&response).unwrap();
            stream.write_all(response_json.as_bytes()).await.unwrap();
            stream.write_all(b"\n").await.unwrap();
        });

        // Give the listener time to start
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Client connects and pings
        let client = DaemonClient::with_socket_path(socket_path);
        let version = client.ping().await.unwrap();
        assert_eq!(version, "test-version");

        // Cleanup
        mock_daemon.await.unwrap();
    }

    #[tokio::test]
    async fn test_end_to_end_execution_pending() {
        use super::super::client::DaemonClient;
        use super::super::messages::{DaemonMessage, DaemonResponse};
        use std::time::Duration;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let temp = TempDir::new().unwrap();
        let socket_path = temp.path().join("test.sock");

        // Create listener
        let (listener, _) = create_listener_at(&socket_path).unwrap();

        // Spawn a mock daemon that handles ExecutionPending
        let mock_daemon = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();

            // Read message
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();

            let msg: DaemonMessage = serde_json::from_str(line.trim()).unwrap();
            match msg {
                DaemonMessage::ExecutionPending { id } => {
                    assert_eq!(id, "exec-123");
                }
                _ => panic!("Expected ExecutionPending"),
            }

            // Send OK response
            let response = DaemonResponse::Ok;
            let response_json = serde_json::to_string(&response).unwrap();
            stream.write_all(response_json.as_bytes()).await.unwrap();
            stream.write_all(b"\n").await.unwrap();
        });

        // Give the listener time to start
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Client connects and notifies
        let client = DaemonClient::with_socket_path(socket_path);
        client.notify_pending("exec-123").await.unwrap();

        // Cleanup
        mock_daemon.await.unwrap();
    }
}
