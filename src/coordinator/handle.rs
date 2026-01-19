//! CoordinatorHandle - Client interface for loop communication

use std::time::Duration;

use eyre::{Result, eyre};
use tokio::sync::{mpsc, oneshot};
use tracing::debug;
use uuid::Uuid;

use super::messages::{CoordMessage, CoordRequest, CoordinatorMetrics};

/// Handle for loops to interact with the Coordinator
///
/// This handle is cloneable and can be passed to loops for coordination.
/// All operations are async and non-blocking.
#[derive(Clone)]
pub struct CoordinatorHandle {
    /// Sender to the Coordinator task
    tx: mpsc::Sender<CoordRequest>,

    /// Receiver for messages from Coordinator (not cloned, each handle has its own)
    /// This is None for cloned handles - only the original has a receiver
    rx: Option<std::sync::Arc<tokio::sync::Mutex<mpsc::Receiver<CoordMessage>>>>,

    /// This handle's execution ID
    exec_id: String,
}

impl CoordinatorHandle {
    /// Create a new handle for an execution
    pub(crate) fn new(tx: mpsc::Sender<CoordRequest>, rx: mpsc::Receiver<CoordMessage>, exec_id: String) -> Self {
        debug!(%exec_id, "CoordinatorHandle::new: called");
        Self {
            tx,
            rx: Some(std::sync::Arc::new(tokio::sync::Mutex::new(rx))),
            exec_id,
        }
    }

    /// Create a handle without a receiver (for sending only)
    pub(crate) fn sender_only(tx: mpsc::Sender<CoordRequest>, exec_id: String) -> Self {
        debug!(%exec_id, "CoordinatorHandle::sender_only: called");
        Self { tx, rx: None, exec_id }
    }

    /// Get this handle's execution ID
    pub fn exec_id(&self) -> &str {
        debug!(exec_id = %self.exec_id, "CoordinatorHandle::exec_id: called");
        &self.exec_id
    }

    /// Broadcast an event to all subscribers
    pub async fn alert(&self, event_type: &str, data: serde_json::Value) -> Result<()> {
        debug!(exec_id = %self.exec_id, %event_type, "CoordinatorHandle::alert: called");
        self.tx
            .send(CoordRequest::Alert {
                from_exec_id: self.exec_id.clone(),
                event_type: event_type.to_string(),
                data,
            })
            .await
            .map_err(|_| eyre!("Coordinator channel closed"))?;

        debug!("CoordinatorHandle::alert: sent");
        Ok(())
    }

    /// Send a query to a specific execution and wait for a reply
    pub async fn query(&self, target_exec_id: &str, question: &str, timeout: Duration) -> Result<String> {
        debug!(exec_id = %self.exec_id, %target_exec_id, %question, ?timeout, "CoordinatorHandle::query: called");
        let query_id = Uuid::now_v7().to_string();
        let (reply_tx, reply_rx) = oneshot::channel();

        self.tx
            .send(CoordRequest::Query {
                query_id: query_id.clone(),
                from_exec_id: self.exec_id.clone(),
                target_exec_id: target_exec_id.to_string(),
                question: question.to_string(),
                reply_tx,
                timeout,
            })
            .await
            .map_err(|_| eyre!("Coordinator channel closed"))?;

        debug!(%query_id, "CoordinatorHandle::query: waiting for reply");
        // Wait for reply (the coordinator handles the timeout)
        reply_rx
            .await
            .map_err(|_| eyre!("Query cancelled or coordinator shutdown"))?
    }

    /// Reply to a query (called by the receiver of a Query message)
    pub async fn reply_query(&self, query_id: &str, answer: &str) -> Result<()> {
        debug!(exec_id = %self.exec_id, %query_id, %answer, "CoordinatorHandle::reply_query: called");
        self.tx
            .send(CoordRequest::QueryReply {
                query_id: query_id.to_string(),
                answer: answer.to_string(),
            })
            .await
            .map_err(|_| eyre!("Coordinator channel closed"))?;

        debug!("CoordinatorHandle::reply_query: sent");
        Ok(())
    }

    /// Cancel a pending query
    pub async fn cancel_query(&self, query_id: &str) -> Result<()> {
        debug!(exec_id = %self.exec_id, %query_id, "CoordinatorHandle::cancel_query: called");
        self.tx
            .send(CoordRequest::QueryCancel {
                query_id: query_id.to_string(),
            })
            .await
            .map_err(|_| eyre!("Coordinator channel closed"))?;

        debug!("CoordinatorHandle::cancel_query: sent");
        Ok(())
    }

    /// Share data with a specific execution
    pub async fn share(&self, target_exec_id: &str, share_type: &str, data: serde_json::Value) -> Result<()> {
        debug!(exec_id = %self.exec_id, %target_exec_id, %share_type, "CoordinatorHandle::share: called");
        self.tx
            .send(CoordRequest::Share {
                from_exec_id: self.exec_id.clone(),
                target_exec_id: target_exec_id.to_string(),
                share_type: share_type.to_string(),
                data,
            })
            .await
            .map_err(|_| eyre!("Coordinator channel closed"))?;

        debug!("CoordinatorHandle::share: sent");
        Ok(())
    }

    /// Subscribe to an event type
    pub async fn subscribe(&self, event_type: &str) -> Result<()> {
        debug!(exec_id = %self.exec_id, %event_type, "CoordinatorHandle::subscribe: called");
        self.tx
            .send(CoordRequest::Subscribe {
                exec_id: self.exec_id.clone(),
                event_type: event_type.to_string(),
            })
            .await
            .map_err(|_| eyre!("Coordinator channel closed"))?;

        debug!("CoordinatorHandle::subscribe: sent");
        Ok(())
    }

    /// Unsubscribe from an event type
    pub async fn unsubscribe(&self, event_type: &str) -> Result<()> {
        debug!(exec_id = %self.exec_id, %event_type, "CoordinatorHandle::unsubscribe: called");
        self.tx
            .send(CoordRequest::Unsubscribe {
                exec_id: self.exec_id.clone(),
                event_type: event_type.to_string(),
            })
            .await
            .map_err(|_| eyre!("Coordinator channel closed"))?;

        debug!("CoordinatorHandle::unsubscribe: sent");
        Ok(())
    }

    /// Request another execution to stop gracefully
    pub async fn stop(&self, target_exec_id: &str, reason: &str) -> Result<()> {
        debug!(exec_id = %self.exec_id, %target_exec_id, %reason, "CoordinatorHandle::stop: called");
        self.tx
            .send(CoordRequest::Stop {
                from_exec_id: self.exec_id.clone(),
                target_exec_id: target_exec_id.to_string(),
                reason: reason.to_string(),
            })
            .await
            .map_err(|_| eyre!("Coordinator channel closed"))?;

        debug!("CoordinatorHandle::stop: sent");
        Ok(())
    }

    /// Receive messages from the Coordinator
    ///
    /// Returns None if the channel is closed or if this is a sender-only handle.
    pub async fn recv(&self) -> Option<CoordMessage> {
        debug!(exec_id = %self.exec_id, "CoordinatorHandle::recv: called");
        let rx = self.rx.as_ref()?;
        debug!("CoordinatorHandle::recv: has receiver");
        let mut rx_guard = rx.lock().await;
        let result = rx_guard.recv().await;
        if result.is_some() {
            debug!("CoordinatorHandle::recv: received message");
        } else {
            debug!("CoordinatorHandle::recv: channel closed");
        }
        result
    }

    /// Try to receive a message without blocking
    ///
    /// Returns None if no message is available or if this is a sender-only handle.
    pub fn try_recv(&self) -> Option<CoordMessage> {
        debug!(exec_id = %self.exec_id, "CoordinatorHandle::try_recv: called");
        let rx = self.rx.as_ref()?;
        debug!("CoordinatorHandle::try_recv: has receiver");
        // Use try_lock to avoid blocking
        let mut rx_guard = rx.try_lock().ok()?;
        debug!("CoordinatorHandle::try_recv: acquired lock");
        let result = rx_guard.try_recv().ok();
        if result.is_some() {
            debug!("CoordinatorHandle::try_recv: received message");
        } else {
            debug!("CoordinatorHandle::try_recv: no message available");
        }
        result
    }

    /// Get current coordinator metrics
    pub async fn metrics(&self) -> Result<CoordinatorMetrics> {
        debug!(exec_id = %self.exec_id, "CoordinatorHandle::metrics: called");
        let (reply_tx, reply_rx) = oneshot::channel();

        self.tx
            .send(CoordRequest::GetMetrics { reply_tx })
            .await
            .map_err(|_| eyre!("Coordinator channel closed"))?;

        debug!("CoordinatorHandle::metrics: waiting for reply");
        reply_rx.await.map_err(|_| eyre!("Coordinator shutdown before reply"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handle_exec_id() {
        let (tx, _rx) = mpsc::channel(10);
        let (msg_tx, msg_rx) = mpsc::channel(10);

        // Register first to get a proper setup
        let _ = tx
            .send(CoordRequest::Register {
                exec_id: "test-exec".to_string(),
                tx: msg_tx,
            })
            .await;

        let handle = CoordinatorHandle::new(tx, msg_rx, "test-exec".to_string());

        assert_eq!(handle.exec_id(), "test-exec");
    }

    #[tokio::test]
    async fn test_sender_only_handle() {
        let (tx, _rx) = mpsc::channel(10);

        let handle = CoordinatorHandle::sender_only(tx, "test-exec".to_string());

        // recv should return None for sender-only handles
        assert!(handle.recv().await.is_none());
        assert!(handle.try_recv().is_none());
    }
}
