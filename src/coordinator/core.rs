//! Main Coordinator task implementation

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use eyre::Result;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

use super::config::CoordinatorConfig;
use super::handle::CoordinatorHandle;
use super::messages::{CoordMessage, CoordRequest, CoordinatorMetrics};
use super::persistence::{EventStore, PersistedEvent};

/// Pending query tracking
struct PendingQuery {
    reply_tx: oneshot::Sender<Result<String>>,
    #[allow(dead_code)]
    from_exec_id: String,
    #[allow(dead_code)]
    target_exec_id: String,
}

/// Rate limiter for per-loop message limiting
struct RateLimiter {
    counters: HashMap<String, VecDeque<Instant>>,
    limit: usize,
    window: Duration,
}

impl RateLimiter {
    fn new(limit: usize, window: Duration) -> Self {
        Self {
            counters: HashMap::new(),
            limit,
            window,
        }
    }

    fn check_and_record(&mut self, exec_id: &str) -> bool {
        let now = Instant::now();
        let counter = self.counters.entry(exec_id.to_string()).or_default();

        // Remove timestamps outside window
        while let Some(&timestamp) = counter.front() {
            if now.duration_since(timestamp) > self.window {
                counter.pop_front();
            } else {
                break;
            }
        }

        // Check if under limit
        if counter.len() < self.limit {
            counter.push_back(now);
            true
        } else {
            false
        }
    }

    fn clear(&mut self, exec_id: &str) {
        self.counters.remove(exec_id);
    }
}

/// The Coordinator mediates all inter-loop communication
pub struct Coordinator {
    config: CoordinatorConfig,
    tx: mpsc::Sender<CoordRequest>,
    rx: mpsc::Receiver<CoordRequest>,
    /// Optional event store for persistence
    event_store: Option<EventStore>,
}

impl Coordinator {
    /// Create a new Coordinator with the given configuration
    pub fn new(config: CoordinatorConfig) -> Self {
        let (tx, rx) = mpsc::channel(config.channel_buffer);
        Self {
            config,
            tx,
            rx,
            event_store: None,
        }
    }

    /// Create a new Coordinator with event persistence
    pub fn with_persistence(config: CoordinatorConfig, store_path: impl Into<std::path::PathBuf>) -> Self {
        let (tx, rx) = mpsc::channel(config.channel_buffer);
        Self {
            config,
            tx,
            rx,
            event_store: Some(EventStore::new(store_path)),
        }
    }

    /// Get a sender for creating handles
    pub fn sender(&self) -> mpsc::Sender<CoordRequest> {
        self.tx.clone()
    }

    /// Create a handle for a new execution
    ///
    /// This registers the execution with the Coordinator and returns a handle
    /// that can be used for coordination.
    pub async fn register(&self, exec_id: &str) -> Result<CoordinatorHandle> {
        let (msg_tx, msg_rx) = mpsc::channel(self.config.loop_channel_buffer);

        self.tx
            .send(CoordRequest::Register {
                exec_id: exec_id.to_string(),
                tx: msg_tx,
            })
            .await
            .map_err(|_| eyre::eyre!("Coordinator channel closed"))?;

        Ok(CoordinatorHandle::new(self.tx.clone(), msg_rx, exec_id.to_string()))
    }

    /// Unregister an execution
    pub async fn unregister(&self, exec_id: &str) -> Result<()> {
        self.tx
            .send(CoordRequest::Unregister {
                exec_id: exec_id.to_string(),
            })
            .await
            .map_err(|_| eyre::eyre!("Coordinator channel closed"))?;

        Ok(())
    }

    /// Request shutdown of the Coordinator
    pub async fn shutdown(&self) -> Result<()> {
        self.tx
            .send(CoordRequest::Shutdown)
            .await
            .map_err(|_| eyre::eyre!("Coordinator channel closed"))?;

        Ok(())
    }

    /// Run the Coordinator task
    ///
    /// This consumes the Coordinator and runs until shutdown is requested.
    pub async fn run(mut self) {
        let coord_tx = self.tx.clone();
        let event_store = self.event_store.take();

        // Internal state
        let mut registry: HashMap<String, mpsc::Sender<CoordMessage>> = HashMap::new();
        let mut subscriptions: HashMap<String, HashSet<String>> = HashMap::new();
        let mut pending_queries: HashMap<String, PendingQuery> = HashMap::new();
        let mut pending_event_ids: HashMap<String, String> = HashMap::new(); // query_id -> event_id
        let mut rate_limiter = RateLimiter::new(self.config.rate_limit_per_sec, Duration::from_secs(1));

        // Metrics
        let mut metrics = CoordinatorMetrics::default();

        info!("Coordinator started");

        while let Some(req) = self.rx.recv().await {
            metrics.messages_received += 1;

            match req {
                CoordRequest::Register { exec_id, tx } => {
                    debug!(exec_id = %exec_id, "Registering execution");
                    registry.insert(exec_id, tx);
                    metrics.registered_executions = registry.len();
                }

                CoordRequest::Unregister { exec_id } => {
                    debug!(exec_id = %exec_id, "Unregistering execution");
                    registry.remove(&exec_id);
                    rate_limiter.clear(&exec_id);

                    // Remove from all subscriptions
                    for subscribers in subscriptions.values_mut() {
                        subscribers.remove(&exec_id);
                    }

                    metrics.registered_executions = registry.len();
                }

                CoordRequest::Alert {
                    from_exec_id,
                    event_type,
                    data,
                } => {
                    // Rate limit check
                    if !rate_limiter.check_and_record(&from_exec_id) {
                        warn!(from_exec_id = %from_exec_id, "Rate limit exceeded for alert");
                        metrics.rate_limit_violations += 1;
                        continue;
                    }

                    debug!(
                        from_exec_id = %from_exec_id,
                        event_type = %event_type,
                        "Broadcasting alert"
                    );

                    // Persist the alert event for crash recovery
                    if let Some(ref store) = event_store {
                        let event = PersistedEvent::alert(&from_exec_id, &event_type, data.to_string());
                        if let Err(e) = store.persist(&event).await {
                            warn!("Failed to persist alert event: {}", e);
                        }
                    }

                    // Broadcast to subscribers
                    if let Some(subscribers) = subscriptions.get(&event_type) {
                        let msg = CoordMessage::Notification {
                            from_exec_id: from_exec_id.clone(),
                            event_type: event_type.clone(),
                            data: data.clone(),
                        };

                        for exec_id in subscribers {
                            if let Some(tx) = registry.get(exec_id)
                                && tx.send(msg.clone()).await.is_ok()
                            {
                                metrics.messages_sent += 1;
                            }
                        }
                    }
                }

                CoordRequest::Query {
                    query_id,
                    from_exec_id,
                    target_exec_id,
                    question,
                    reply_tx,
                    timeout,
                } => {
                    // Rate limit check
                    if !rate_limiter.check_and_record(&from_exec_id) {
                        warn!(from_exec_id = %from_exec_id, "Rate limit exceeded for query");
                        metrics.rate_limit_violations += 1;
                        let _ = reply_tx.send(Err(eyre::eyre!("Rate limit exceeded")));
                        continue;
                    }

                    debug!(
                        query_id = %query_id,
                        from_exec_id = %from_exec_id,
                        target_exec_id = %target_exec_id,
                        "Sending query"
                    );

                    // Persist the query event for crash recovery
                    if let Some(ref store) = event_store {
                        let event = PersistedEvent::query(&from_exec_id, &target_exec_id, &question);
                        let event_id = event.id.clone();
                        if let Err(e) = store.persist(&event).await {
                            warn!("Failed to persist query event: {}", e);
                        } else {
                            // Track event_id to resolve later
                            pending_event_ids.insert(query_id.clone(), event_id);
                        }
                    }

                    // Send query to target
                    if let Some(tx) = registry.get(&target_exec_id) {
                        let msg = CoordMessage::Query {
                            query_id: query_id.clone(),
                            from_exec_id: from_exec_id.clone(),
                            question,
                        };

                        if tx.send(msg).await.is_ok() {
                            metrics.messages_sent += 1;

                            // Track pending query
                            pending_queries.insert(
                                query_id.clone(),
                                PendingQuery {
                                    reply_tx,
                                    from_exec_id,
                                    target_exec_id,
                                },
                            );
                            metrics.pending_queries = pending_queries.len();

                            // Spawn timeout handler
                            let query_id_clone = query_id.clone();
                            let timeout_tx = coord_tx.clone();
                            tokio::spawn(async move {
                                tokio::time::sleep(timeout).await;
                                let _ = timeout_tx
                                    .send(CoordRequest::QueryTimeout {
                                        query_id: query_id_clone,
                                    })
                                    .await;
                            });
                        } else {
                            let _ = reply_tx.send(Err(eyre::eyre!("Target execution channel closed")));
                        }
                    } else {
                        let _ = reply_tx.send(Err(eyre::eyre!("Target execution not found")));
                    }
                }

                CoordRequest::QueryReply { query_id, answer } => {
                    debug!(query_id = %query_id, "Received query reply");

                    if let Some(pending) = pending_queries.remove(&query_id) {
                        let _ = pending.reply_tx.send(Ok(answer));
                        metrics.pending_queries = pending_queries.len();

                        // Resolve the persisted event
                        if let Some(event_id) = pending_event_ids.remove(&query_id)
                            && let Some(ref store) = event_store
                            && let Err(e) = store.resolve(&event_id).await
                        {
                            warn!("Failed to resolve query event: {}", e);
                        }
                    }
                }

                CoordRequest::QueryCancel { query_id } => {
                    debug!(query_id = %query_id, "Cancelling query");

                    if let Some(pending) = pending_queries.remove(&query_id) {
                        let _ = pending.reply_tx.send(Err(eyre::eyre!("Query cancelled")));
                        metrics.pending_queries = pending_queries.len();

                        // Resolve the persisted event (even though cancelled)
                        if let Some(event_id) = pending_event_ids.remove(&query_id)
                            && let Some(ref store) = event_store
                            && let Err(e) = store.resolve(&event_id).await
                        {
                            warn!("Failed to resolve cancelled query event: {}", e);
                        }
                    }
                }

                CoordRequest::QueryTimeout { query_id } => {
                    if let Some(pending) = pending_queries.remove(&query_id) {
                        warn!(query_id = %query_id, "Query timed out");
                        let _ = pending.reply_tx.send(Err(eyre::eyre!("Query timeout")));
                        metrics.pending_queries = pending_queries.len();
                        metrics.query_timeouts += 1;

                        // Resolve the persisted event (even though timed out)
                        if let Some(event_id) = pending_event_ids.remove(&query_id)
                            && let Some(ref store) = event_store
                            && let Err(e) = store.resolve(&event_id).await
                        {
                            warn!("Failed to resolve timed out query event: {}", e);
                        }
                    }
                }

                CoordRequest::Share {
                    from_exec_id,
                    target_exec_id,
                    share_type,
                    data,
                } => {
                    // Rate limit check
                    if !rate_limiter.check_and_record(&from_exec_id) {
                        warn!(from_exec_id = %from_exec_id, "Rate limit exceeded for share");
                        metrics.rate_limit_violations += 1;
                        continue;
                    }

                    debug!(
                        from_exec_id = %from_exec_id,
                        target_exec_id = %target_exec_id,
                        share_type = %share_type,
                        "Sharing data"
                    );

                    // Persist the share event for crash recovery
                    if let Some(ref store) = event_store {
                        let event =
                            PersistedEvent::share(&from_exec_id, &target_exec_id, &share_type, data.to_string());
                        if let Err(e) = store.persist(&event).await {
                            warn!("Failed to persist share event: {}", e);
                        }
                    }

                    if let Some(tx) = registry.get(&target_exec_id) {
                        let msg = CoordMessage::Share {
                            from_exec_id,
                            share_type,
                            data,
                        };
                        if tx.send(msg).await.is_ok() {
                            metrics.messages_sent += 1;
                        }
                    }
                }

                CoordRequest::Subscribe { exec_id, event_type } => {
                    debug!(exec_id = %exec_id, event_type = %event_type, "Subscribing");

                    subscriptions.entry(event_type).or_default().insert(exec_id);

                    metrics.total_subscriptions = subscriptions.values().map(|s| s.len()).sum();
                }

                CoordRequest::Unsubscribe { exec_id, event_type } => {
                    debug!(exec_id = %exec_id, event_type = %event_type, "Unsubscribing");

                    if let Some(subscribers) = subscriptions.get_mut(&event_type) {
                        subscribers.remove(&exec_id);
                    }

                    metrics.total_subscriptions = subscriptions.values().map(|s| s.len()).sum();
                }

                CoordRequest::Stop {
                    from_exec_id,
                    target_exec_id,
                    reason,
                } => {
                    debug!(
                        from_exec_id = %from_exec_id,
                        target_exec_id = %target_exec_id,
                        reason = %reason,
                        "Sending stop request"
                    );

                    if let Some(tx) = registry.get(&target_exec_id) {
                        let msg = CoordMessage::Stop { from_exec_id, reason };
                        if tx.send(msg).await.is_ok() {
                            metrics.messages_sent += 1;
                        }
                    }
                }

                CoordRequest::GetMetrics { reply_tx } => {
                    let _ = reply_tx.send(metrics.clone());
                }

                CoordRequest::Shutdown => {
                    info!("Coordinator shutting down");
                    break;
                }
            }
        }

        info!("Coordinator stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_coordinator_register_unregister() {
        let coord = Coordinator::new(CoordinatorConfig::default());
        let coord_sender = coord.sender();

        // Spawn coordinator task
        let coord_task = tokio::spawn(coord.run());

        // Register an execution
        let (msg_tx, _msg_rx) = mpsc::channel(10);
        coord_sender
            .send(CoordRequest::Register {
                exec_id: "exec-001".to_string(),
                tx: msg_tx,
            })
            .await
            .unwrap();

        // Get metrics
        let (reply_tx, reply_rx) = oneshot::channel();
        coord_sender.send(CoordRequest::GetMetrics { reply_tx }).await.unwrap();

        let metrics = reply_rx.await.unwrap();
        assert_eq!(metrics.registered_executions, 1);

        // Unregister
        coord_sender
            .send(CoordRequest::Unregister {
                exec_id: "exec-001".to_string(),
            })
            .await
            .unwrap();

        // Shutdown
        coord_sender.send(CoordRequest::Shutdown).await.unwrap();

        coord_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_coordinator_alert_subscription() {
        let coord = Coordinator::new(CoordinatorConfig::default());
        let coord_sender = coord.sender();

        let coord_task = tokio::spawn(coord.run());

        // Register two executions
        let (msg_tx1, mut msg_rx1) = mpsc::channel(10);
        let (msg_tx2, mut msg_rx2) = mpsc::channel(10);

        coord_sender
            .send(CoordRequest::Register {
                exec_id: "exec-001".to_string(),
                tx: msg_tx1,
            })
            .await
            .unwrap();

        coord_sender
            .send(CoordRequest::Register {
                exec_id: "exec-002".to_string(),
                tx: msg_tx2,
            })
            .await
            .unwrap();

        // Subscribe exec-002 to "phase_complete"
        coord_sender
            .send(CoordRequest::Subscribe {
                exec_id: "exec-002".to_string(),
                event_type: "phase_complete".to_string(),
            })
            .await
            .unwrap();

        // exec-001 sends an alert
        coord_sender
            .send(CoordRequest::Alert {
                from_exec_id: "exec-001".to_string(),
                event_type: "phase_complete".to_string(),
                data: json!({"phase": "Phase 1"}),
            })
            .await
            .unwrap();

        // Give coordinator time to process
        tokio::time::sleep(Duration::from_millis(50)).await;

        // exec-002 should receive the notification
        let msg = msg_rx2.try_recv();
        assert!(msg.is_ok());
        match msg.unwrap() {
            CoordMessage::Notification { event_type, .. } => {
                assert_eq!(event_type, "phase_complete");
            }
            _ => panic!("Wrong message type"),
        }

        // exec-001 should not receive (not subscribed)
        assert!(msg_rx1.try_recv().is_err());

        // Shutdown
        coord_sender.send(CoordRequest::Shutdown).await.unwrap();
        coord_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_coordinator_query_reply() {
        let coord = Coordinator::new(CoordinatorConfig::default());
        let coord_sender = coord.sender();

        let coord_task = tokio::spawn(coord.run());

        // Register two executions
        let (msg_tx1, _msg_rx1) = mpsc::channel(10);
        let (msg_tx2, mut msg_rx2) = mpsc::channel(10);

        coord_sender
            .send(CoordRequest::Register {
                exec_id: "exec-001".to_string(),
                tx: msg_tx1,
            })
            .await
            .unwrap();

        coord_sender
            .send(CoordRequest::Register {
                exec_id: "exec-002".to_string(),
                tx: msg_tx2,
            })
            .await
            .unwrap();

        // exec-001 queries exec-002
        let (reply_tx, reply_rx) = oneshot::channel();
        coord_sender
            .send(CoordRequest::Query {
                query_id: "query-001".to_string(),
                from_exec_id: "exec-001".to_string(),
                target_exec_id: "exec-002".to_string(),
                question: "What is the API URL?".to_string(),
                reply_tx,
                timeout: Duration::from_secs(5),
            })
            .await
            .unwrap();

        // Give coordinator time to process
        tokio::time::sleep(Duration::from_millis(50)).await;

        // exec-002 should receive the query
        let msg = msg_rx2.try_recv();
        assert!(msg.is_ok());
        match msg.unwrap() {
            CoordMessage::Query { query_id, question, .. } => {
                assert_eq!(query_id, "query-001");
                assert_eq!(question, "What is the API URL?");
            }
            _ => panic!("Wrong message type"),
        }

        // exec-002 replies
        coord_sender
            .send(CoordRequest::QueryReply {
                query_id: "query-001".to_string(),
                answer: "http://localhost:8080".to_string(),
            })
            .await
            .unwrap();

        // exec-001 should receive the reply
        let reply = reply_rx.await.unwrap();
        assert!(reply.is_ok());
        assert_eq!(reply.unwrap(), "http://localhost:8080");

        // Shutdown
        coord_sender.send(CoordRequest::Shutdown).await.unwrap();
        coord_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_coordinator_query_timeout() {
        let coord = Coordinator::new(CoordinatorConfig::default());
        let coord_sender = coord.sender();

        let coord_task = tokio::spawn(coord.run());

        // Register two executions
        let (msg_tx1, _msg_rx1) = mpsc::channel(10);
        let (msg_tx2, _msg_rx2) = mpsc::channel(10);

        coord_sender
            .send(CoordRequest::Register {
                exec_id: "exec-001".to_string(),
                tx: msg_tx1,
            })
            .await
            .unwrap();

        coord_sender
            .send(CoordRequest::Register {
                exec_id: "exec-002".to_string(),
                tx: msg_tx2,
            })
            .await
            .unwrap();

        // exec-001 queries exec-002 with short timeout
        let (reply_tx, reply_rx) = oneshot::channel();
        coord_sender
            .send(CoordRequest::Query {
                query_id: "query-001".to_string(),
                from_exec_id: "exec-001".to_string(),
                target_exec_id: "exec-002".to_string(),
                question: "What is the API URL?".to_string(),
                reply_tx,
                timeout: Duration::from_millis(100),
            })
            .await
            .unwrap();

        // Don't reply - wait for timeout
        let reply = reply_rx.await.unwrap();
        assert!(reply.is_err());
        assert!(reply.unwrap_err().to_string().contains("timeout"));

        // Shutdown
        coord_sender.send(CoordRequest::Shutdown).await.unwrap();
        coord_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_coordinator_share() {
        let coord = Coordinator::new(CoordinatorConfig::default());
        let coord_sender = coord.sender();

        let coord_task = tokio::spawn(coord.run());

        // Register two executions
        let (msg_tx1, _msg_rx1) = mpsc::channel(10);
        let (msg_tx2, mut msg_rx2) = mpsc::channel(10);

        coord_sender
            .send(CoordRequest::Register {
                exec_id: "exec-001".to_string(),
                tx: msg_tx1,
            })
            .await
            .unwrap();

        coord_sender
            .send(CoordRequest::Register {
                exec_id: "exec-002".to_string(),
                tx: msg_tx2,
            })
            .await
            .unwrap();

        // exec-001 shares data with exec-002
        coord_sender
            .send(CoordRequest::Share {
                from_exec_id: "exec-001".to_string(),
                target_exec_id: "exec-002".to_string(),
                share_type: "test_results".to_string(),
                data: json!({"passed": 42, "failed": 3}),
            })
            .await
            .unwrap();

        // Give coordinator time to process
        tokio::time::sleep(Duration::from_millis(50)).await;

        // exec-002 should receive the share
        let msg = msg_rx2.try_recv();
        assert!(msg.is_ok());
        match msg.unwrap() {
            CoordMessage::Share { share_type, data, .. } => {
                assert_eq!(share_type, "test_results");
                assert_eq!(data["passed"], 42);
            }
            _ => panic!("Wrong message type"),
        }

        // Shutdown
        coord_sender.send(CoordRequest::Shutdown).await.unwrap();
        coord_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_coordinator_stop() {
        let coord = Coordinator::new(CoordinatorConfig::default());
        let coord_sender = coord.sender();

        let coord_task = tokio::spawn(coord.run());

        // Register two executions
        let (msg_tx1, _msg_rx1) = mpsc::channel(10);
        let (msg_tx2, mut msg_rx2) = mpsc::channel(10);

        coord_sender
            .send(CoordRequest::Register {
                exec_id: "exec-001".to_string(),
                tx: msg_tx1,
            })
            .await
            .unwrap();

        coord_sender
            .send(CoordRequest::Register {
                exec_id: "exec-002".to_string(),
                tx: msg_tx2,
            })
            .await
            .unwrap();

        // exec-001 requests exec-002 to stop
        coord_sender
            .send(CoordRequest::Stop {
                from_exec_id: "exec-001".to_string(),
                target_exec_id: "exec-002".to_string(),
                reason: "Rebase needed".to_string(),
            })
            .await
            .unwrap();

        // Give coordinator time to process
        tokio::time::sleep(Duration::from_millis(50)).await;

        // exec-002 should receive the stop request
        let msg = msg_rx2.try_recv();
        assert!(msg.is_ok());
        match msg.unwrap() {
            CoordMessage::Stop { reason, .. } => {
                assert_eq!(reason, "Rebase needed");
            }
            _ => panic!("Wrong message type"),
        }

        // Shutdown
        coord_sender.send(CoordRequest::Shutdown).await.unwrap();
        coord_task.await.unwrap();
    }

    #[tokio::test]
    async fn test_coordinator_rate_limiting() {
        let config = CoordinatorConfig {
            rate_limit_per_sec: 2,
            ..Default::default()
        };
        let coord = Coordinator::new(config);
        let coord_sender = coord.sender();

        let coord_task = tokio::spawn(coord.run());

        // Register execution
        let (msg_tx, _msg_rx) = mpsc::channel(10);
        coord_sender
            .send(CoordRequest::Register {
                exec_id: "exec-001".to_string(),
                tx: msg_tx,
            })
            .await
            .unwrap();

        // Subscribe to receive alerts
        coord_sender
            .send(CoordRequest::Subscribe {
                exec_id: "exec-001".to_string(),
                event_type: "test".to_string(),
            })
            .await
            .unwrap();

        // Send more alerts than rate limit allows
        for i in 0..5 {
            coord_sender
                .send(CoordRequest::Alert {
                    from_exec_id: "exec-001".to_string(),
                    event_type: "test".to_string(),
                    data: json!({"i": i}),
                })
                .await
                .unwrap();
        }

        // Give coordinator time to process
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Check metrics - should have rate limit violations
        let (reply_tx, reply_rx) = oneshot::channel();
        coord_sender.send(CoordRequest::GetMetrics { reply_tx }).await.unwrap();

        let metrics = reply_rx.await.unwrap();
        assert!(metrics.rate_limit_violations > 0);

        // Shutdown
        coord_sender.send(CoordRequest::Shutdown).await.unwrap();
        coord_task.await.unwrap();
    }
}
