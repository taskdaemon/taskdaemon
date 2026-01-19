//! SystemCapturedProgress - default progress strategy
//!
//! Records the raw stdout/stderr from each validation run, truncated to
//! prevent context window explosion. Keeps only the N most recent
//! iterations (older history is less relevant for debugging).

use std::collections::VecDeque;
use tracing::debug;

use super::{IterationContext, ProgressStrategy};

/// Default progress strategy: capture validation output verbatim
///
/// Records the raw stdout/stderr from each validation run, truncated
/// to prevent context window explosion. Keeps only the N most recent
/// iterations (older history is less relevant for debugging).
///
/// # Configuration
/// - `max_entries`: Maximum iterations to keep (default: 5)
/// - `max_output_chars`: Truncate output per iteration (default: 500)
///
/// # Example Output
/// ```markdown
/// ## Iteration 3
/// **Command:** `cargo test`
/// **Exit code:** 1
/// **Duration:** 2341ms
/// **Files changed:** src/lib.rs, src/main.rs
/// **Output:**
/// ```
/// running 8 tests
/// test auth::test_validate ... FAILED
///
/// failures:
///     auth::test_validate - assertion failed: expected Ok, got Err
/// ```
/// ```
#[derive(Debug, Clone)]
pub struct SystemCapturedProgress {
    entries: VecDeque<String>,
    max_entries: usize,
    max_output_chars: usize,
}

impl SystemCapturedProgress {
    /// Create with default settings (5 entries, 500 chars each)
    pub fn new() -> Self {
        debug!("SystemCapturedProgress::new: called");
        Self::default()
    }

    /// Create with custom limits
    pub fn with_limits(max_entries: usize, max_output_chars: usize) -> Self {
        debug!(
            %max_entries,
            %max_output_chars,
            "SystemCapturedProgress::with_limits: called"
        );
        Self {
            entries: VecDeque::with_capacity(max_entries),
            max_entries,
            max_output_chars,
        }
    }
}

impl Default for SystemCapturedProgress {
    fn default() -> Self {
        debug!("SystemCapturedProgress::default: called");
        Self {
            entries: VecDeque::with_capacity(5),
            max_entries: 5,
            max_output_chars: 500,
        }
    }
}

impl ProgressStrategy for SystemCapturedProgress {
    fn record(&mut self, ctx: &IterationContext) -> String {
        debug!(
            iteration = %ctx.iteration,
            exit_code = %ctx.exit_code,
            duration_ms = %ctx.duration_ms,
            "SystemCapturedProgress::record: called"
        );

        // Combine stdout and stderr, prefer stdout if available
        let output = if !ctx.stdout.is_empty() {
            debug!("SystemCapturedProgress::record: using stdout (not empty)");
            &ctx.stdout
        } else {
            debug!("SystemCapturedProgress::record: using stderr (stdout empty)");
            &ctx.stderr
        };

        // Truncate output, keeping the END (most relevant for errors)
        let truncated = if output.len() > self.max_output_chars {
            debug!(
                output_len = %output.len(),
                max_chars = %self.max_output_chars,
                "SystemCapturedProgress::record: truncating output"
            );
            let start = output.len() - self.max_output_chars;
            format!("...[truncated]...\n{}", &output[start..])
        } else {
            debug!("SystemCapturedProgress::record: output within limits, no truncation");
            output.clone()
        };

        // Format entry
        let entry = format!(
            "## Iteration {}\n\
             **Command:** `{}`\n\
             **Exit code:** {}\n\
             **Duration:** {}ms\n\
             **Files changed:** {}\n\
             **Output:**\n```\n{}\n```\n\n",
            ctx.iteration,
            ctx.validation_command,
            ctx.exit_code,
            ctx.duration_ms,
            if ctx.files_changed.is_empty() {
                debug!("SystemCapturedProgress::record: no files changed");
                "none".to_string()
            } else {
                debug!(
                    files_count = %ctx.files_changed.len(),
                    "SystemCapturedProgress::record: files changed"
                );
                ctx.files_changed.join(", ")
            },
            truncated.trim(),
        );

        // Add to queue, evict oldest if at capacity
        if self.entries.len() >= self.max_entries {
            debug!(
                entries_len = %self.entries.len(),
                max_entries = %self.max_entries,
                "SystemCapturedProgress::record: evicting oldest entry"
            );
            self.entries.pop_front();
        } else {
            debug!("SystemCapturedProgress::record: capacity available, no eviction");
        }
        self.entries.push_back(entry.clone());

        entry
    }

    fn get_progress(&self) -> String {
        debug!(
            entries_count = %self.entries.len(),
            "SystemCapturedProgress::get_progress: called"
        );
        self.entries.iter().cloned().collect::<Vec<_>>().join("")
    }

    fn clear(&mut self) {
        debug!(
            entries_count = %self.entries.len(),
            "SystemCapturedProgress::clear: called"
        );
        self.entries.clear();
    }

    fn len(&self) -> usize {
        debug!("SystemCapturedProgress::len: called");
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx(iteration: u32, exit_code: i32, output: &str) -> IterationContext {
        IterationContext {
            iteration,
            validation_command: "cargo test".to_string(),
            exit_code,
            stdout: output.to_string(),
            stderr: String::new(),
            duration_ms: 1000,
            files_changed: vec!["src/lib.rs".to_string()],
        }
    }

    #[test]
    fn test_system_captured_records_iteration() {
        let mut progress = SystemCapturedProgress::default();

        let entry = progress.record(&make_ctx(1, 1, "test failed"));

        assert!(entry.contains("Iteration 1"));
        assert!(entry.contains("**Exit code:** 1"));
        assert!(entry.contains("test failed"));
        assert_eq!(progress.len(), 1);
    }

    #[test]
    fn test_system_captured_truncates_entries() {
        let mut progress = SystemCapturedProgress::with_limits(3, 100);

        for i in 1..=5 {
            progress.record(&make_ctx(i, 1, &format!("error {}", i)));
        }

        // Should only keep last 3
        assert_eq!(progress.len(), 3);
        let text = progress.get_progress();
        assert!(!text.contains("Iteration 1"));
        assert!(!text.contains("Iteration 2"));
        assert!(text.contains("Iteration 3"));
        assert!(text.contains("Iteration 4"));
        assert!(text.contains("Iteration 5"));
    }

    #[test]
    fn test_system_captured_truncates_output() {
        let mut progress = SystemCapturedProgress::with_limits(5, 50);

        let long_output = "x".repeat(200);
        progress.record(&make_ctx(1, 1, &long_output));

        let text = progress.get_progress();
        assert!(text.contains("[truncated]"));
        // The full output would have 200 x's, but we truncated
        assert!(text.matches('x').count() <= 100); // Should be significantly less
    }

    #[test]
    fn test_system_captured_clear() {
        let mut progress = SystemCapturedProgress::default();

        progress.record(&make_ctx(1, 0, "ok"));
        progress.record(&make_ctx(2, 0, "ok"));
        assert_eq!(progress.len(), 2);

        progress.clear();
        assert_eq!(progress.len(), 0);
        assert!(progress.get_progress().is_empty());
    }

    #[test]
    fn test_empty_progress_returns_empty_string() {
        let progress = SystemCapturedProgress::default();
        assert!(progress.get_progress().is_empty());
        assert!(progress.is_empty());
    }

    #[test]
    fn test_files_changed_formatting() {
        let mut progress = SystemCapturedProgress::default();

        let ctx = IterationContext {
            iteration: 1,
            validation_command: "cargo test".to_string(),
            exit_code: 0,
            stdout: "ok".to_string(),
            stderr: String::new(),
            duration_ms: 100,
            files_changed: vec!["a.rs".to_string(), "b.rs".to_string()],
        };

        let entry = progress.record(&ctx);
        assert!(entry.contains("a.rs, b.rs"));
    }

    #[test]
    fn test_no_files_changed() {
        let mut progress = SystemCapturedProgress::default();

        let ctx = IterationContext {
            iteration: 1,
            validation_command: "cargo test".to_string(),
            exit_code: 0,
            stdout: "ok".to_string(),
            stderr: String::new(),
            duration_ms: 100,
            files_changed: vec![],
        };

        let entry = progress.record(&ctx);
        assert!(entry.contains("Files changed:** none"));
    }

    #[test]
    fn test_prefers_stdout_over_stderr() {
        let mut progress = SystemCapturedProgress::default();

        let ctx = IterationContext {
            iteration: 1,
            validation_command: "cargo test".to_string(),
            exit_code: 0,
            stdout: "stdout content".to_string(),
            stderr: "stderr content".to_string(),
            duration_ms: 100,
            files_changed: vec![],
        };

        let entry = progress.record(&ctx);
        assert!(entry.contains("stdout content"));
        assert!(!entry.contains("stderr content"));
    }

    #[test]
    fn test_uses_stderr_when_stdout_empty() {
        let mut progress = SystemCapturedProgress::default();

        let ctx = IterationContext {
            iteration: 1,
            validation_command: "cargo test".to_string(),
            exit_code: 1,
            stdout: String::new(),
            stderr: "error message".to_string(),
            duration_ms: 100,
            files_changed: vec![],
        };

        let entry = progress.record(&ctx);
        assert!(entry.contains("error message"));
    }
}
