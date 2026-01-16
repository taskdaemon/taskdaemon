//! Validation execution

use std::time::Duration;

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
        self.exit_code == success_exit_code
    }
}

/// Run a validation command in the worktree
pub async fn run_validation(
    command: &str,
    worktree: &std::path::Path,
    timeout: Duration,
) -> eyre::Result<ValidationResult> {
    let start = std::time::Instant::now();

    let output = tokio::time::timeout(
        timeout,
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(worktree)
            .output(),
    )
    .await??;

    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(ValidationResult {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        duration_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
