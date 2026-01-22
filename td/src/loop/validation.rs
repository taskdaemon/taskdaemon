//! Validation execution

use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::debug;

use crate::events::EventEmitter;

/// Result of running validation command
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Exit code from the validation command
    pub exit_code: i32,

    /// Standard output
    pub stdout: String,

    /// Standard error
    pub stderr: String,

    /// How long validation took
    pub duration_ms: u64,
}

impl ValidationResult {
    /// Check if validation passed
    pub fn passed(&self, success_exit_code: i32) -> bool {
        let passed = self.exit_code == success_exit_code;
        debug!(
            exit_code = self.exit_code,
            success_exit_code, passed, "ValidationResult::passed: called"
        );
        passed
    }
}

/// Run a validation command in the worktree
pub async fn run_validation(
    command: &str,
    worktree: &std::path::Path,
    timeout: Duration,
) -> eyre::Result<ValidationResult> {
    debug!(%command, ?worktree, timeout_ms = timeout.as_millis() as u64, "run_validation: called");
    let start = std::time::Instant::now();

    debug!(%command, "run_validation: executing command");
    let output = tokio::time::timeout(
        timeout,
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(worktree)
            .output(),
    )
    .await;

    match output {
        Ok(Ok(output)) => {
            let duration_ms = start.elapsed().as_millis() as u64;
            let exit_code = output.status.code().unwrap_or(-1);
            debug!(exit_code, duration_ms, "run_validation: command completed");
            Ok(ValidationResult {
                exit_code,
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                duration_ms,
            })
        }
        Ok(Err(e)) => {
            debug!(error = %e, "run_validation: command execution failed");
            Err(e.into())
        }
        Err(_) => {
            debug!("run_validation: command timed out");
            Err(eyre::eyre!("Validation command timed out"))
        }
    }
}

/// Run a validation command with streaming output
///
/// This version spawns the process and streams stdout/stderr line-by-line
/// to the event emitter. Use this when real-time output visibility is needed.
pub async fn run_validation_streaming(
    command: &str,
    worktree: &std::path::Path,
    timeout: Duration,
    emitter: &EventEmitter,
    iteration: u32,
) -> eyre::Result<ValidationResult> {
    debug!(%command, ?worktree, timeout_ms = timeout.as_millis() as u64, "run_validation_streaming: called");
    let start = std::time::Instant::now();

    // Emit validation started event
    emitter.validation_started(iteration, command);

    debug!(%command, "run_validation_streaming: spawning command");
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(worktree)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| eyre::eyre!("Failed to capture stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| eyre::eyre!("Failed to capture stderr"))?;

    // Create readers for stdout and stderr
    let stdout_reader = BufReader::new(stdout);
    let stderr_reader = BufReader::new(stderr);

    // Clone emitter for stdout task
    let stdout_emitter = emitter.clone();
    let stdout_iteration = iteration;

    // Spawn task to read stdout lines
    let stdout_task = tokio::spawn(async move {
        let mut lines = stdout_reader.lines();
        let mut output = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            stdout_emitter.validation_output(stdout_iteration, &line, false);
            output.push_str(&line);
            output.push('\n');
        }
        output
    });

    // Clone emitter for stderr task
    let stderr_emitter = emitter.clone();
    let stderr_iteration = iteration;

    // Spawn task to read stderr lines
    let stderr_task = tokio::spawn(async move {
        let mut lines = stderr_reader.lines();
        let mut output = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            stderr_emitter.validation_output(stderr_iteration, &line, true);
            output.push_str(&line);
            output.push('\n');
        }
        output
    });

    // Wait for process with timeout
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => {
            debug!(error = %e, "run_validation_streaming: wait failed");
            return Err(e.into());
        }
        Err(_) => {
            debug!("run_validation_streaming: command timed out");
            // Try to kill the process
            let _ = child.kill().await;
            return Err(eyre::eyre!("Validation command timed out"));
        }
    };

    // Wait for output tasks to complete
    let stdout_output = stdout_task.await.unwrap_or_default();
    let stderr_output = stderr_task.await.unwrap_or_default();

    let duration_ms = start.elapsed().as_millis() as u64;
    let exit_code = status.code().unwrap_or(-1);

    // Emit validation completed event
    emitter.validation_completed(iteration, exit_code, duration_ms);

    debug!(exit_code, duration_ms, "run_validation_streaming: command completed");
    Ok(ValidationResult {
        exit_code,
        stdout: stdout_output,
        stderr: stderr_output,
        duration_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventBus;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_validation_success() {
        let temp = tempdir().unwrap();
        let result = run_validation("echo ok", temp.path(), Duration::from_secs(30))
            .await
            .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.passed(0));
        assert!(result.stdout.contains("ok"));
    }

    #[tokio::test]
    async fn test_validation_failure() {
        let temp = tempdir().unwrap();
        let result = run_validation("exit 1", temp.path(), Duration::from_secs(30))
            .await
            .unwrap();

        assert_eq!(result.exit_code, 1);
        assert!(!result.passed(0));
    }

    #[tokio::test]
    async fn test_validation_timeout() {
        let temp = tempdir().unwrap();
        let result = run_validation("sleep 10", temp.path(), Duration::from_millis(100)).await;

        // Should timeout
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_validation_streaming_success() {
        let temp = tempdir().unwrap();
        let bus = EventBus::with_default_capacity();
        let emitter = bus.emitter_for("test-exec");
        let mut rx = bus.subscribe();

        let result = run_validation_streaming(
            "echo hello; echo world",
            temp.path(),
            Duration::from_secs(30),
            &emitter,
            1,
        )
        .await
        .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
        assert!(result.stdout.contains("world"));

        // Check we received events
        let mut event_types = Vec::new();
        while let Ok(event) = rx.try_recv() {
            event_types.push(event.event_type().to_string());
        }

        assert!(event_types.contains(&"ValidationStarted".to_string()));
        assert!(event_types.contains(&"ValidationOutput".to_string()));
        assert!(event_types.contains(&"ValidationCompleted".to_string()));
    }

    #[tokio::test]
    async fn test_validation_streaming_stderr() {
        let temp = tempdir().unwrap();
        let bus = EventBus::with_default_capacity();
        let emitter = bus.emitter_for("test-exec");
        let mut rx = bus.subscribe();

        let result = run_validation_streaming("echo error >&2", temp.path(), Duration::from_secs(30), &emitter, 1)
            .await
            .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stderr.contains("error"));

        // Check we received stderr output event
        let mut has_stderr_event = false;
        while let Ok(event) = rx.try_recv() {
            if let crate::events::Event::ValidationOutput { is_stderr: true, .. } = event {
                has_stderr_event = true;
            }
        }
        assert!(has_stderr_event);
    }
}
