//! ProgressStrategy trait definition

use tracing::debug;

/// Context passed to progress strategy after each iteration
#[derive(Debug, Clone)]
pub struct IterationContext {
    /// Current iteration number (1-indexed)
    pub iteration: u32,
    /// The validation command that was run
    pub validation_command: String,
    /// Exit code from validation (0 = success)
    pub exit_code: i32,
    /// Stdout from validation command
    pub stdout: String,
    /// Stderr from validation command
    pub stderr: String,
    /// How long validation took
    pub duration_ms: u64,
    /// Files modified during this iteration (from git status)
    pub files_changed: Vec<String>,
}

impl IterationContext {
    /// Create a new iteration context
    pub fn new(
        iteration: u32,
        validation_command: impl Into<String>,
        exit_code: i32,
        stdout: impl Into<String>,
        stderr: impl Into<String>,
        duration_ms: u64,
        files_changed: Vec<String>,
    ) -> Self {
        debug!(
            %iteration,
            %exit_code,
            %duration_ms,
            ?files_changed,
            "IterationContext::new: called"
        );
        Self {
            iteration,
            validation_command: validation_command.into(),
            exit_code,
            stdout: stdout.into(),
            stderr: stderr.into(),
            duration_ms,
            files_changed,
        }
    }

    /// Check if this iteration's validation passed
    pub fn passed(&self) -> bool {
        debug!(exit_code = %self.exit_code, "IterationContext::passed: called");
        let result = self.exit_code == 0;
        if result {
            debug!("IterationContext::passed: validation passed (exit_code == 0)");
        } else {
            debug!("IterationContext::passed: validation failed (exit_code != 0)");
        }
        result
    }
}

/// Strategy for accumulating progress across loop iterations
///
/// Implementors define how iteration outcomes are recorded and formatted
/// for injection into subsequent prompts.
pub trait ProgressStrategy: Send + Sync {
    /// Record the outcome of an iteration
    ///
    /// Called after each iteration completes (pass or fail).
    /// Returns the formatted entry for this iteration.
    fn record(&mut self, ctx: &IterationContext) -> String;

    /// Get accumulated progress text for prompt injection
    ///
    /// This text is injected into the `{{progress}}` template variable.
    /// Should return empty string if no progress recorded yet.
    fn get_progress(&self) -> String;

    /// Reset all accumulated progress
    ///
    /// Called when loop is restarted or manually cleared.
    fn clear(&mut self);

    /// Number of iterations currently recorded
    fn len(&self) -> usize;

    /// Whether any progress has been recorded
    fn is_empty(&self) -> bool {
        debug!("ProgressStrategy::is_empty: called");
        let result = self.len() == 0;
        if result {
            debug!("ProgressStrategy::is_empty: no progress recorded");
        } else {
            debug!("ProgressStrategy::is_empty: progress exists");
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iteration_context_new() {
        let ctx = IterationContext::new(
            1,
            "cargo test",
            0,
            "test passed",
            "",
            1500,
            vec!["src/lib.rs".to_string()],
        );

        assert_eq!(ctx.iteration, 1);
        assert_eq!(ctx.validation_command, "cargo test");
        assert_eq!(ctx.exit_code, 0);
        assert!(ctx.passed());
    }

    #[test]
    fn test_iteration_context_failed() {
        let ctx = IterationContext::new(1, "cargo test", 1, "", "test failed", 2000, vec![]);

        assert!(!ctx.passed());
    }
}
