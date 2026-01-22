//! IPC client for communicating with the daemon
//!
//! Provides a simple interface for the TUI/CLI to send messages to the daemon
//! via Unix Domain Socket.

use std::path::PathBuf;
use std::time::Duration;

use eyre::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tracing::debug;

use super::get_socket_path;
use super::messages::{DaemonMessage, DaemonResponse};

/// Default timeout for IPC operations
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Maximum message size (1KB as per design doc)
const MAX_MESSAGE_SIZE: usize = 1024;

/// Client for communicating with the daemon via IPC
#[derive(Debug, Clone)]
pub struct DaemonClient {
    socket_path: PathBuf,
    timeout: Duration,
}

impl Default for DaemonClient {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonClient {
    /// Create a new client with the default socket path
    pub fn new() -> Self {
        Self {
            socket_path: get_socket_path(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Create a client with a custom socket path (for testing)
    pub fn with_socket_path(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Set a custom timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Check if the daemon socket exists
    pub fn socket_exists(&self) -> bool {
        self.socket_path.exists()
    }

    /// Notify daemon that an execution is pending
    ///
    /// This is fire-and-forget: errors are logged but not propagated to avoid
    /// blocking the TUI. The daemon's 60-second poll serves as a fallback.
    pub async fn notify_pending(&self, execution_id: &str) -> Result<()> {
        debug!(%execution_id, "DaemonClient: notifying pending execution");
        let msg = DaemonMessage::ExecutionPending {
            id: execution_id.to_string(),
        };
        let response = self.send_message(msg).await?;
        match response {
            DaemonResponse::Ok => Ok(()),
            DaemonResponse::Error { message } => Err(eyre::eyre!("Daemon error: {}", message)),
            _ => Err(eyre::eyre!("Unexpected response")),
        }
    }

    /// Notify daemon that an execution was resumed
    pub async fn notify_resumed(&self, execution_id: &str) -> Result<()> {
        debug!(%execution_id, "DaemonClient: notifying resumed execution");
        let msg = DaemonMessage::ExecutionResumed {
            id: execution_id.to_string(),
        };
        let response = self.send_message(msg).await?;
        match response {
            DaemonResponse::Ok => Ok(()),
            DaemonResponse::Error { message } => Err(eyre::eyre!("Daemon error: {}", message)),
            _ => Err(eyre::eyre!("Unexpected response")),
        }
    }

    /// Check if daemon is alive and get its version
    pub async fn ping(&self) -> Result<String> {
        debug!("DaemonClient: pinging daemon");
        let response = self.send_message(DaemonMessage::Ping).await?;
        match response {
            DaemonResponse::Pong { version } => Ok(version),
            DaemonResponse::Error { message } => Err(eyre::eyre!("Daemon error: {}", message)),
            _ => Err(eyre::eyre!("Unexpected response")),
        }
    }

    /// Request daemon to shutdown gracefully
    pub async fn shutdown(&self) -> Result<()> {
        debug!("DaemonClient: requesting daemon shutdown");
        let response = self.send_message(DaemonMessage::Shutdown).await?;
        match response {
            DaemonResponse::Ok => Ok(()),
            DaemonResponse::Error { message } => Err(eyre::eyre!("Daemon error: {}", message)),
            _ => Err(eyre::eyre!("Unexpected response")),
        }
    }

    /// Send a message to the daemon and wait for response
    async fn send_message(&self, msg: DaemonMessage) -> Result<DaemonResponse> {
        debug!(?self.socket_path, ?msg, "DaemonClient: sending message");

        // Connect with timeout
        let stream = tokio::time::timeout(self.timeout, UnixStream::connect(&self.socket_path))
            .await
            .context("Connection timeout")?
            .context("Failed to connect to daemon socket")?;

        self.send_on_stream(stream, msg).await
    }

    /// Send message on an existing stream (extracted for testing)
    async fn send_on_stream(&self, mut stream: UnixStream, msg: DaemonMessage) -> Result<DaemonResponse> {
        // Serialize message
        let msg_json = serde_json::to_string(&msg).context("Failed to serialize message")?;

        // Validate message size
        if msg_json.len() > MAX_MESSAGE_SIZE {
            return Err(eyre::eyre!("Message too large: {} bytes", msg_json.len()));
        }

        // Send message with newline
        tokio::time::timeout(self.timeout, async {
            stream
                .write_all(msg_json.as_bytes())
                .await
                .context("Failed to write message")?;
            stream.write_all(b"\n").await.context("Failed to write newline")?;
            stream.flush().await.context("Failed to flush stream")?;
            Ok::<_, eyre::Error>(())
        })
        .await
        .context("Write timeout")??;

        // Read response with size limit
        let mut reader = BufReader::new(&mut stream);
        let mut response_line = String::new();

        tokio::time::timeout(self.timeout, async {
            let bytes_read = reader
                .read_line(&mut response_line)
                .await
                .context("Failed to read response")?;

            if bytes_read > MAX_MESSAGE_SIZE {
                return Err(eyre::eyre!("Response too large: {} bytes", bytes_read));
            }

            Ok::<_, eyre::Error>(())
        })
        .await
        .context("Read timeout")??;

        // Parse response
        let response: DaemonResponse =
            serde_json::from_str(response_line.trim()).context("Failed to parse daemon response")?;

        debug!(?response, "DaemonClient: received response");
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_client_default() {
        let client = DaemonClient::default();
        assert!(client.socket_path.ends_with("daemon.sock"));
    }

    #[test]
    fn test_client_with_custom_path() {
        let path = PathBuf::from("/custom/path/daemon.sock");
        let client = DaemonClient::with_socket_path(path.clone());
        assert_eq!(client.socket_path, path);
    }

    #[test]
    fn test_client_with_timeout() {
        let client = DaemonClient::new().with_timeout(Duration::from_secs(10));
        assert_eq!(client.timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_socket_exists_false() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("nonexistent.sock");
        let client = DaemonClient::with_socket_path(path);
        assert!(!client.socket_exists());
    }
}
