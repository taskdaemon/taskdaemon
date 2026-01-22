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
use super::types::{Event, EventLogEntry};

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
    pub fn write_event(&mut self, event: &Event) -> eyre::Result<()> {
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
                    let is_loop_completed = matches!(event, Event::LoopCompleted { .. });

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

/// Replay events for an execution from the default runs directory
///
/// Returns all events for the given execution ID, sorted by timestamp.
/// Returns an empty Vec if the execution has no logged events.
pub fn replay_execution_events(execution_id: &str) -> eyre::Result<Vec<Event>> {
    let home = dirs::home_dir().ok_or_else(|| eyre::eyre!("Could not determine home directory"))?;
    let runs_dir = home.join(".taskdaemon").join("runs");
    let entries = read_execution_events(&runs_dir, execution_id)?;
    Ok(entries.into_iter().map(|e| e.event).collect())
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

        let event = Event::LoopStarted {
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

        let event1 = Event::LoopStarted {
            execution_id: "test-123".to_string(),
            loop_type: "plan".to_string(),
            task_description: "Test task".to_string(),
        };
        let event2 = Event::IterationStarted {
            execution_id: "test-123".to_string(),
            iteration: 1,
        };
        let event3 = Event::IterationCompleted {
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

        let event1 = Event::LoopStarted {
            execution_id: "exec-1".to_string(),
            loop_type: "plan".to_string(),
            task_description: "Task 1".to_string(),
        };
        let event2 = Event::LoopStarted {
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
            .write_event(&Event::LoopStarted {
                execution_id: "test-read".to_string(),
                loop_type: "plan".to_string(),
                task_description: "Test".to_string(),
            })
            .unwrap();
        logger
            .write_event(&Event::IterationStarted {
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
            .write_event(&Event::LoopStarted {
                execution_id: "test-close".to_string(),
                loop_type: "plan".to_string(),
                task_description: "Test".to_string(),
            })
            .unwrap();

        assert!(logger.writers.contains_key("test-close"));
        logger.close_execution("test-close");
        assert!(!logger.writers.contains_key("test-close"));
    }

    #[test]
    fn test_replay_preserves_order() {
        let temp = tempdir().unwrap();
        let mut logger = EventLogger::new(temp.path());

        // Write events in order
        logger
            .write_event(&Event::LoopStarted {
                execution_id: "test-replay".to_string(),
                loop_type: "plan".to_string(),
                task_description: "Test".to_string(),
            })
            .unwrap();
        logger
            .write_event(&Event::IterationStarted {
                execution_id: "test-replay".to_string(),
                iteration: 1,
            })
            .unwrap();
        logger
            .write_event(&Event::ValidationStarted {
                execution_id: "test-replay".to_string(),
                iteration: 1,
                command: "echo test".to_string(),
            })
            .unwrap();
        logger
            .write_event(&Event::LoopCompleted {
                execution_id: "test-replay".to_string(),
                success: true,
                total_iterations: 1,
            })
            .unwrap();

        // Read back events
        let entries = read_execution_events(temp.path(), "test-replay").unwrap();
        assert_eq!(entries.len(), 4);

        // Verify order
        assert_eq!(entries[0].event.event_type(), "LoopStarted");
        assert_eq!(entries[1].event.event_type(), "IterationStarted");
        assert_eq!(entries[2].event.event_type(), "ValidationStarted");
        assert_eq!(entries[3].event.event_type(), "LoopCompleted");
    }

    #[test]
    fn test_close_execution_idempotent() {
        let temp = tempdir().unwrap();
        let mut logger = EventLogger::new(temp.path());

        logger
            .write_event(&Event::LoopStarted {
                execution_id: "idem-test".to_string(),
                loop_type: "plan".to_string(),
                task_description: "Test".to_string(),
            })
            .unwrap();

        // Close multiple times - should not panic
        logger.close_execution("idem-test");
        logger.close_execution("idem-test");
        logger.close_execution("idem-test");

        // Writer should be removed
        assert!(!logger.writers.contains_key("idem-test"));
    }

    #[test]
    fn test_close_nonexistent_execution() {
        let temp = tempdir().unwrap();
        let mut logger = EventLogger::new(temp.path());

        // Close an execution that was never opened - should not panic
        logger.close_execution("never-existed");
    }

    #[test]
    fn test_executions_are_isolated() {
        let temp = tempdir().unwrap();
        let mut logger = EventLogger::new(temp.path());

        // Write to two different executions
        logger
            .write_event(&Event::LoopStarted {
                execution_id: "iso-1".to_string(),
                loop_type: "plan".to_string(),
                task_description: "Task 1".to_string(),
            })
            .unwrap();
        logger
            .write_event(&Event::LoopStarted {
                execution_id: "iso-2".to_string(),
                loop_type: "spec".to_string(),
                task_description: "Task 2".to_string(),
            })
            .unwrap();

        // Add more events to iso-1
        logger
            .write_event(&Event::IterationStarted {
                execution_id: "iso-1".to_string(),
                iteration: 1,
            })
            .unwrap();

        // Read back - each execution should only have its own events
        let entries_1 = read_execution_events(temp.path(), "iso-1").unwrap();
        let entries_2 = read_execution_events(temp.path(), "iso-2").unwrap();

        assert_eq!(entries_1.len(), 2);
        assert_eq!(entries_2.len(), 1);

        // Verify correct content
        assert!(entries_1.iter().all(|e| e.event.execution_id() == "iso-1"));
        assert!(entries_2.iter().all(|e| e.event.execution_id() == "iso-2"));
    }

    #[test]
    fn test_events_persisted_immediately() {
        let temp = tempdir().unwrap();
        let mut logger = EventLogger::new(temp.path());

        // Write an event
        logger
            .write_event(&Event::LoopStarted {
                execution_id: "persist-test".to_string(),
                loop_type: "plan".to_string(),
                task_description: "Test".to_string(),
            })
            .unwrap();

        // Don't close - just read from disk immediately
        let log_path = temp.path().join("persist-test").join("events.jsonl");
        let content = std::fs::read_to_string(&log_path).unwrap();

        // Should be readable without closing
        assert!(content.contains("LoopStarted"));
        assert!(content.contains("persist-test"));
    }

    #[test]
    fn test_event_log_file_is_jsonl() {
        let temp = tempdir().unwrap();
        let mut logger = EventLogger::new(temp.path());

        // Write multiple events
        for i in 0..5 {
            logger
                .write_event(&Event::IterationStarted {
                    execution_id: "jsonl-test".to_string(),
                    iteration: i,
                })
                .unwrap();
        }

        // Read raw file - each line should be valid JSON
        let log_path = temp.path().join("jsonl-test").join("events.jsonl");
        let content = std::fs::read_to_string(&log_path).unwrap();

        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 5);

        for line in lines {
            // Each line should parse as JSON
            let parsed: serde_json::Value = serde_json::from_str(line).expect("Each line should be valid JSON");
            assert!(parsed.get("ts").is_some(), "Should have timestamp");
            assert!(parsed.get("event").is_some(), "Should have event");
        }
    }

    #[test]
    fn test_reopen_after_close() {
        let temp = tempdir().unwrap();
        let mut logger = EventLogger::new(temp.path());

        // Write, close, write again
        logger
            .write_event(&Event::LoopStarted {
                execution_id: "reopen-test".to_string(),
                loop_type: "plan".to_string(),
                task_description: "First".to_string(),
            })
            .unwrap();

        logger.close_execution("reopen-test");

        // Write to same execution again (should reopen/append)
        logger
            .write_event(&Event::LoopCompleted {
                execution_id: "reopen-test".to_string(),
                success: true,
                total_iterations: 1,
            })
            .unwrap();

        // Both events should be in the file
        let entries = read_execution_events(temp.path(), "reopen-test").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].event.event_type(), "LoopStarted");
        assert_eq!(entries[1].event.event_type(), "LoopCompleted");
    }
}
