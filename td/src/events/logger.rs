//! Event Logger - persists events to JSONL files
//!
//! The EventLogger subscribes to the EventBus and writes all events to
//! per-execution JSONL files for history, debugging, and replay.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::{debug, error, warn};

use super::bus::EventBus;
use super::types::{EventLogEntry, TdEvent};

/// Event logger that writes events to JSONL files
///
/// Events are written to `~/.taskdaemon/runs/{execution-id}/events.jsonl`
pub struct EventLogger {
    /// Base directory for run data (~/.taskdaemon/runs)
    runs_dir: PathBuf,
    /// Open file writers per execution
    writers: HashMap<String, BufWriter<File>>,
}

impl EventLogger {
    /// Create a new event logger
    pub fn new(runs_dir: impl AsRef<Path>) -> Self {
        let runs_dir = runs_dir.as_ref().to_path_buf();
        debug!(?runs_dir, "EventLogger::new: creating logger");
        Self {
            runs_dir,
            writers: HashMap::new(),
        }
    }

    /// Create a logger with the default runs directory (~/.taskdaemon/runs)
    pub fn with_default_path() -> eyre::Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| eyre::eyre!("Could not determine home directory"))?;
        let runs_dir = home.join(".taskdaemon").join("runs");
        fs::create_dir_all(&runs_dir)?;
        Ok(Self::new(runs_dir))
    }

    /// Write an event to its execution's log file
    pub fn write_event(&mut self, event: &TdEvent) -> eyre::Result<()> {
        let execution_id = event.execution_id();
        debug!(%execution_id, event_type = event.event_type(), "EventLogger::write_event");

        // Get or create writer for this execution
        let writer = if let Some(w) = self.writers.get_mut(execution_id) {
            w
        } else {
            // Create directory and file for new execution
            let exec_dir = self.runs_dir.join(execution_id);
            fs::create_dir_all(&exec_dir)?;

            let log_path = exec_dir.join("events.jsonl");
            debug!(?log_path, "EventLogger: creating new log file");

            let file = OpenOptions::new().create(true).append(true).open(&log_path)?;
            let writer = BufWriter::new(file);
            self.writers.insert(execution_id.to_string(), writer);
            self.writers.get_mut(execution_id).unwrap()
        };

        // Write event as JSON line
        let entry = EventLogEntry::new(event.clone());
        let json = serde_json::to_string(&entry)?;
        writeln!(writer, "{}", json)?;
        writer.flush()?;

        Ok(())
    }

    /// Close writer for an execution (e.g., when loop completes)
    pub fn close_execution(&mut self, execution_id: &str) {
        debug!(%execution_id, "EventLogger::close_execution");
        if let Some(mut writer) = self.writers.remove(execution_id) {
            let _ = writer.flush();
        }
    }

    /// Run the logger, consuming events from the bus until shutdown
    ///
    /// This is meant to be spawned as a background task.
    pub async fn run(mut self, event_bus: Arc<EventBus>) {
        debug!("EventLogger::run: starting event logger");
        let mut rx = event_bus.subscribe();

        loop {
            match rx.recv().await {
                Ok(event) => {
                    // Close writer if loop completed
                    let execution_id = event.execution_id().to_string();
                    let is_loop_completed = matches!(event, TdEvent::LoopCompleted { .. });

                    if let Err(e) = self.write_event(&event) {
                        error!(%execution_id, error = %e, "EventLogger: failed to write event");
                    }

                    if is_loop_completed {
                        self.close_execution(&execution_id);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(missed = n, "EventLogger: lagged behind, missed events");
                    // Continue processing - we'll catch up
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!("EventLogger: channel closed, shutting down");
                    break;
                }
            }
        }

        // Flush all remaining writers
        for (exec_id, mut writer) in self.writers.drain() {
            debug!(%exec_id, "EventLogger: flushing writer on shutdown");
            let _ = writer.flush();
        }
    }
}

/// Read events from an execution's log file
pub fn read_execution_events(runs_dir: impl AsRef<Path>, execution_id: &str) -> eyre::Result<Vec<EventLogEntry>> {
    let log_path = runs_dir.as_ref().join(execution_id).join("events.jsonl");
    debug!(?log_path, "read_execution_events: reading log file");

    if !log_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&log_path)?;
    let mut entries = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<EventLogEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                warn!(line, error = %e, "read_execution_events: failed to parse line");
            }
        }
    }

    debug!(count = entries.len(), "read_execution_events: loaded entries");
    Ok(entries)
}

/// Spawn the event logger as a background task
pub fn spawn_event_logger(event_bus: Arc<EventBus>) -> eyre::Result<tokio::task::JoinHandle<()>> {
    let logger = EventLogger::with_default_path()?;
    Ok(tokio::spawn(async move {
        logger.run(event_bus).await;
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_event_logger_creation() {
        let temp = tempdir().unwrap();
        let logger = EventLogger::new(temp.path());
        assert!(logger.writers.is_empty());
    }

    #[test]
    fn test_write_event() {
        let temp = tempdir().unwrap();
        let mut logger = EventLogger::new(temp.path());

        let event = TdEvent::LoopStarted {
            execution_id: "test-123".to_string(),
            loop_type: "plan".to_string(),
            task_description: "Test task".to_string(),
        };

        logger.write_event(&event).unwrap();

        // Check file was created
        let log_path = temp.path().join("test-123").join("events.jsonl");
        assert!(log_path.exists());

        // Check content
        let content = fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("LoopStarted"));
        assert!(content.contains("test-123"));
    }

    #[test]
    fn test_multiple_events_same_execution() {
        let temp = tempdir().unwrap();
        let mut logger = EventLogger::new(temp.path());

        let event1 = TdEvent::LoopStarted {
            execution_id: "test-123".to_string(),
            loop_type: "plan".to_string(),
            task_description: "Test task".to_string(),
        };
        let event2 = TdEvent::IterationStarted {
            execution_id: "test-123".to_string(),
            iteration: 1,
        };
        let event3 = TdEvent::IterationCompleted {
            execution_id: "test-123".to_string(),
            iteration: 1,
            outcome: super::super::types::IterationOutcome::ValidationPassed,
        };

        logger.write_event(&event1).unwrap();
        logger.write_event(&event2).unwrap();
        logger.write_event(&event3).unwrap();

        // Check file has 3 lines
        let log_path = temp.path().join("test-123").join("events.jsonl");
        let content = fs::read_to_string(&log_path).unwrap();
        assert_eq!(content.lines().count(), 3);
    }

    #[test]
    fn test_multiple_executions() {
        let temp = tempdir().unwrap();
        let mut logger = EventLogger::new(temp.path());

        let event1 = TdEvent::LoopStarted {
            execution_id: "exec-1".to_string(),
            loop_type: "plan".to_string(),
            task_description: "Task 1".to_string(),
        };
        let event2 = TdEvent::LoopStarted {
            execution_id: "exec-2".to_string(),
            loop_type: "spec".to_string(),
            task_description: "Task 2".to_string(),
        };

        logger.write_event(&event1).unwrap();
        logger.write_event(&event2).unwrap();

        // Check both files exist
        assert!(temp.path().join("exec-1").join("events.jsonl").exists());
        assert!(temp.path().join("exec-2").join("events.jsonl").exists());
    }

    #[test]
    fn test_read_execution_events() {
        let temp = tempdir().unwrap();
        let mut logger = EventLogger::new(temp.path());

        // Write some events
        logger
            .write_event(&TdEvent::LoopStarted {
                execution_id: "test-read".to_string(),
                loop_type: "plan".to_string(),
                task_description: "Test".to_string(),
            })
            .unwrap();
        logger
            .write_event(&TdEvent::IterationStarted {
                execution_id: "test-read".to_string(),
                iteration: 1,
            })
            .unwrap();

        // Read them back
        let entries = read_execution_events(temp.path(), "test-read").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].event.event_type(), "LoopStarted");
        assert_eq!(entries[1].event.event_type(), "IterationStarted");
    }

    #[test]
    fn test_read_nonexistent_execution() {
        let temp = tempdir().unwrap();
        let entries = read_execution_events(temp.path(), "nonexistent").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_close_execution() {
        let temp = tempdir().unwrap();
        let mut logger = EventLogger::new(temp.path());

        logger
            .write_event(&TdEvent::LoopStarted {
                execution_id: "test-close".to_string(),
                loop_type: "plan".to_string(),
                task_description: "Test".to_string(),
            })
            .unwrap();

        assert!(logger.writers.contains_key("test-close"));
        logger.close_execution("test-close");
        assert!(!logger.writers.contains_key("test-close"));
    }
}
