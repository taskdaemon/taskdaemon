//! Coordinator event persistence for crash recovery
//!
//! Persists coordination events (alerts, queries, shares) to disk for recovery
//! after crashes or restarts.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use eyre::Result;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::debug;
use uuid::Uuid;

/// Type of persisted event
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PersistedEventType {
    /// Alert/broadcast event
    Alert,
    /// Query request
    Query,
    /// Data sharing event
    Share,
}

impl std::fmt::Display for PersistedEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        debug!(?self, "PersistedEventType::fmt: called");
        match self {
            Self::Alert => {
                debug!("PersistedEventType::fmt: Alert branch");
                write!(f, "Alert")
            }
            Self::Query => {
                debug!("PersistedEventType::fmt: Query branch");
                write!(f, "Query")
            }
            Self::Share => {
                debug!("PersistedEventType::fmt: Share branch");
                write!(f, "Share")
            }
        }
    }
}

/// Persisted coordinator event for crash recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedEvent {
    /// Unique event ID
    pub id: String,
    /// Type of event
    pub event_type: PersistedEventType,
    /// Source execution ID
    pub from_exec_id: String,
    /// Target execution ID (None for broadcasts)
    pub to_exec_id: Option<String>,
    /// Event payload (JSON)
    pub payload: String,
    /// Unix timestamp when created
    pub created_at: i64,
    /// Unix timestamp when resolved (None if pending)
    pub resolved_at: Option<i64>,
}

/// Get current Unix timestamp in seconds
fn now_timestamp() -> i64 {
    debug!("now_timestamp: called");
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

impl PersistedEvent {
    /// Create a new persisted event
    pub fn new(
        event_type: PersistedEventType,
        from_exec_id: impl Into<String>,
        to_exec_id: Option<String>,
        payload: impl Into<String>,
    ) -> Self {
        debug!(?event_type, "PersistedEvent::new: called");
        Self {
            id: Uuid::now_v7().to_string(),
            event_type,
            from_exec_id: from_exec_id.into(),
            to_exec_id,
            payload: payload.into(),
            created_at: now_timestamp(),
            resolved_at: None,
        }
    }

    /// Create an alert event
    pub fn alert(from_exec_id: impl Into<String>, event_type_name: &str, payload: impl Into<String>) -> Self {
        debug!(%event_type_name, "PersistedEvent::alert: called");
        Self::new(
            PersistedEventType::Alert,
            from_exec_id,
            None,
            serde_json::json!({
                "event_type": event_type_name,
                "data": payload.into()
            })
            .to_string(),
        )
    }

    /// Create a query event
    pub fn query(from_exec_id: impl Into<String>, to_exec_id: impl Into<String>, question: &str) -> Self {
        debug!(%question, "PersistedEvent::query: called");
        Self::new(
            PersistedEventType::Query,
            from_exec_id,
            Some(to_exec_id.into()),
            serde_json::json!({ "question": question }).to_string(),
        )
    }

    /// Create a share event
    pub fn share(
        from_exec_id: impl Into<String>,
        to_exec_id: impl Into<String>,
        share_type: &str,
        data: impl Into<String>,
    ) -> Self {
        debug!(%share_type, "PersistedEvent::share: called");
        Self::new(
            PersistedEventType::Share,
            from_exec_id,
            Some(to_exec_id.into()),
            serde_json::json!({
                "share_type": share_type,
                "data": data.into()
            })
            .to_string(),
        )
    }

    /// Check if event is resolved
    pub fn is_resolved(&self) -> bool {
        debug!(id = %self.id, "PersistedEvent::is_resolved: called");
        let resolved = self.resolved_at.is_some();
        if resolved {
            debug!("PersistedEvent::is_resolved: event is resolved");
        } else {
            debug!("PersistedEvent::is_resolved: event is not resolved");
        }
        resolved
    }

    /// Mark event as resolved
    pub fn resolve(&mut self) {
        debug!(id = %self.id, "PersistedEvent::resolve: called");
        self.resolved_at = Some(now_timestamp());
    }
}

/// Coordinator event store for persistence
pub struct EventStore {
    store_path: PathBuf,
}

impl EventStore {
    /// Create a new event store
    pub fn new(store_path: impl Into<PathBuf>) -> Self {
        let path = store_path.into();
        debug!(?path, "EventStore::new: called");
        Self { store_path: path }
    }

    /// Get the events file path
    fn events_file(&self) -> PathBuf {
        debug!("EventStore::events_file: called");
        self.store_path.join("coordinator_events.jsonl")
    }

    /// Ensure the store directory exists
    async fn ensure_dir(&self) -> Result<()> {
        debug!(path = ?self.store_path, "EventStore::ensure_dir: called");
        fs::create_dir_all(&self.store_path).await?;
        debug!("EventStore::ensure_dir: directory created");
        Ok(())
    }

    /// Persist an event
    pub async fn persist(&self, event: &PersistedEvent) -> Result<()> {
        debug!(event_id = %event.id, ?event.event_type, "EventStore::persist: called");
        self.ensure_dir().await?;

        let events_file = self.events_file();
        let line = serde_json::to_string(event)? + "\n";

        debug!(?events_file, "EventStore::persist: opening file");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_file)
            .await?;

        file.write_all(line.as_bytes()).await?;
        file.flush().await?;

        debug!("EventStore::persist: event written");
        Ok(())
    }

    /// Mark an event as resolved
    pub async fn resolve(&self, event_id: &str) -> Result<bool> {
        debug!(%event_id, "EventStore::resolve: called");
        let events_file = self.events_file();

        if !events_file.exists() {
            debug!("EventStore::resolve: events file does not exist");
            return Ok(false);
        }

        debug!("EventStore::resolve: events file exists, reading");
        let content = fs::read_to_string(&events_file).await?;

        let mut events: Vec<PersistedEvent> = content
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        let mut found = false;
        for event in &mut events {
            if event.id == event_id {
                debug!("EventStore::resolve: found event, marking resolved");
                event.resolve();
                found = true;
            }
        }

        if found {
            debug!("EventStore::resolve: writing updated events");
            let new_content: String = events
                .iter()
                .map(|e| serde_json::to_string(e).unwrap() + "\n")
                .collect();

            fs::write(&events_file, new_content).await?;
        } else {
            debug!("EventStore::resolve: event not found");
        }

        Ok(found)
    }

    /// Get all unresolved events for crash recovery
    pub async fn get_unresolved(&self) -> Result<Vec<PersistedEvent>> {
        debug!("EventStore::get_unresolved: called");
        let events_file = self.events_file();

        if !events_file.exists() {
            debug!("EventStore::get_unresolved: events file does not exist");
            return Ok(vec![]);
        }

        debug!("EventStore::get_unresolved: reading events file");
        let content = fs::read_to_string(&events_file).await?;
        let events: Vec<PersistedEvent> = content
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .filter(|e: &PersistedEvent| !e.is_resolved())
            .collect();

        debug!(count = events.len(), "EventStore::get_unresolved: returning events");
        Ok(events)
    }

    /// Get all events (resolved and unresolved)
    pub async fn get_all(&self) -> Result<Vec<PersistedEvent>> {
        debug!("EventStore::get_all: called");
        let events_file = self.events_file();

        if !events_file.exists() {
            debug!("EventStore::get_all: events file does not exist");
            return Ok(vec![]);
        }

        debug!("EventStore::get_all: reading events file");
        let content = fs::read_to_string(&events_file).await?;
        let events: Vec<PersistedEvent> = content
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        debug!(count = events.len(), "EventStore::get_all: returning events");
        Ok(events)
    }

    /// Get events for a specific execution
    pub async fn get_for_exec(&self, exec_id: &str) -> Result<Vec<PersistedEvent>> {
        debug!(%exec_id, "EventStore::get_for_exec: called");
        let all = self.get_all().await?;
        let events: Vec<PersistedEvent> = all
            .into_iter()
            .filter(|e| e.from_exec_id == exec_id || e.to_exec_id.as_deref() == Some(exec_id))
            .collect();
        debug!(count = events.len(), "EventStore::get_for_exec: returning events");
        Ok(events)
    }

    /// Clean up old resolved events (older than specified hours)
    pub async fn cleanup_old(&self, hours: i64) -> Result<usize> {
        debug!(%hours, "EventStore::cleanup_old: called");
        let events_file = self.events_file();

        if !events_file.exists() {
            debug!("EventStore::cleanup_old: events file does not exist");
            return Ok(0);
        }

        let cutoff = now_timestamp() - (hours * 3600);
        debug!(%cutoff, "EventStore::cleanup_old: reading events file");
        let content = fs::read_to_string(&events_file).await?;

        let events: Vec<PersistedEvent> = content
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        let original_count = events.len();
        debug!(%original_count, "EventStore::cleanup_old: found events");

        // Keep events that are either unresolved or resolved recently
        let kept: Vec<_> = events
            .into_iter()
            .filter(|e| !e.is_resolved() || e.resolved_at.unwrap_or(0) > cutoff)
            .collect();

        let removed_count = original_count - kept.len();

        if removed_count > 0 {
            debug!(%removed_count, "EventStore::cleanup_old: removing old events");
            let new_content: String = kept.iter().map(|e| serde_json::to_string(e).unwrap() + "\n").collect();

            fs::write(&events_file, new_content).await?;
        } else {
            debug!("EventStore::cleanup_old: no events to remove");
        }

        Ok(removed_count)
    }

    /// Clear all events (for testing)
    pub async fn clear(&self) -> Result<()> {
        debug!("EventStore::clear: called");
        let events_file = self.events_file();

        if events_file.exists() {
            debug!("EventStore::clear: removing events file");
            fs::remove_file(&events_file).await?;
        } else {
            debug!("EventStore::clear: events file does not exist");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_persist_and_get_unresolved() {
        let temp = tempdir().unwrap();
        let store = EventStore::new(temp.path());

        let event1 = PersistedEvent::alert("exec-001", "test_event", "data1");
        let event2 = PersistedEvent::query("exec-001", "exec-002", "What is the status?");

        store.persist(&event1).await.unwrap();
        store.persist(&event2).await.unwrap();

        let unresolved = store.get_unresolved().await.unwrap();
        assert_eq!(unresolved.len(), 2);
    }

    #[tokio::test]
    async fn test_resolve_event() {
        let temp = tempdir().unwrap();
        let store = EventStore::new(temp.path());

        let event = PersistedEvent::alert("exec-001", "test_event", "data");
        let event_id = event.id.clone();

        store.persist(&event).await.unwrap();

        // Should have 1 unresolved
        let unresolved = store.get_unresolved().await.unwrap();
        assert_eq!(unresolved.len(), 1);

        // Resolve it
        let found = store.resolve(&event_id).await.unwrap();
        assert!(found);

        // Should have 0 unresolved
        let unresolved = store.get_unresolved().await.unwrap();
        assert_eq!(unresolved.len(), 0);

        // All should still show 1
        let all = store.get_all().await.unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].is_resolved());
    }

    #[tokio::test]
    async fn test_resolve_nonexistent() {
        let temp = tempdir().unwrap();
        let store = EventStore::new(temp.path());

        let found = store.resolve("nonexistent").await.unwrap();
        assert!(!found);
    }

    #[tokio::test]
    async fn test_get_for_exec() {
        let temp = tempdir().unwrap();
        let store = EventStore::new(temp.path());

        // Create events involving different executions
        let event1 = PersistedEvent::alert("exec-001", "event1", "data1");
        let event2 = PersistedEvent::query("exec-001", "exec-002", "question");
        let event3 = PersistedEvent::share("exec-003", "exec-002", "type", "data");

        store.persist(&event1).await.unwrap();
        store.persist(&event2).await.unwrap();
        store.persist(&event3).await.unwrap();

        // exec-001 should see 2 events (sender of alert and query)
        let exec1_events = store.get_for_exec("exec-001").await.unwrap();
        assert_eq!(exec1_events.len(), 2);

        // exec-002 should see 2 events (target of query and share)
        let exec2_events = store.get_for_exec("exec-002").await.unwrap();
        assert_eq!(exec2_events.len(), 2);

        // exec-003 should see 1 event (sender of share)
        let exec3_events = store.get_for_exec("exec-003").await.unwrap();
        assert_eq!(exec3_events.len(), 1);
    }

    #[tokio::test]
    async fn test_cleanup_old() {
        let temp = tempdir().unwrap();
        let store = EventStore::new(temp.path());

        // Create and resolve an event
        let mut event = PersistedEvent::alert("exec-001", "old_event", "data");
        // Make it look old (resolved 48 hours ago)
        event.resolved_at = Some(now_timestamp() - 48 * 3600);

        store.persist(&event).await.unwrap();

        // Create a fresh unresolved event
        let event2 = PersistedEvent::alert("exec-002", "new_event", "data");
        store.persist(&event2).await.unwrap();

        // Cleanup events older than 24 hours
        let removed = store.cleanup_old(24).await.unwrap();
        assert_eq!(removed, 1);

        // Only unresolved event should remain
        let all = store.get_all().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].from_exec_id, "exec-002");
    }

    #[tokio::test]
    async fn test_clear() {
        let temp = tempdir().unwrap();
        let store = EventStore::new(temp.path());

        store
            .persist(&PersistedEvent::alert("exec-001", "event", "data"))
            .await
            .unwrap();

        let all = store.get_all().await.unwrap();
        assert_eq!(all.len(), 1);

        store.clear().await.unwrap();

        let all = store.get_all().await.unwrap();
        assert_eq!(all.len(), 0);
    }

    #[test]
    fn test_event_type_display() {
        assert_eq!(PersistedEventType::Alert.to_string(), "Alert");
        assert_eq!(PersistedEventType::Query.to_string(), "Query");
        assert_eq!(PersistedEventType::Share.to_string(), "Share");
    }

    #[test]
    fn test_persisted_event_constructors() {
        let alert = PersistedEvent::alert("exec-1", "test", "payload");
        assert_eq!(alert.event_type, PersistedEventType::Alert);
        assert!(alert.to_exec_id.is_none());

        let query = PersistedEvent::query("exec-1", "exec-2", "question?");
        assert_eq!(query.event_type, PersistedEventType::Query);
        assert_eq!(query.to_exec_id, Some("exec-2".to_string()));

        let share = PersistedEvent::share("exec-1", "exec-2", "type", "data");
        assert_eq!(share.event_type, PersistedEventType::Share);
        assert_eq!(share.to_exec_id, Some("exec-2".to_string()));
    }
}
