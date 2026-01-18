//! Loop analytics and metrics
//!
//! Tracks per-loop and aggregate metrics:
//! - Iteration counts and timing
//! - Success/failure rates
//! - Tool usage statistics
//! - LLM token consumption

use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Aggregate metrics for all loops
#[derive(Debug, Default)]
pub struct LoopMetrics {
    /// Per-loop metrics by exec_id
    loops: RwLock<HashMap<String, LoopStats>>,

    /// Per-type aggregate metrics
    type_metrics: RwLock<HashMap<String, TypeMetrics>>,

    /// Global counters
    global: GlobalMetrics,
}

/// Global metrics counters (thread-safe)
#[derive(Debug, Default)]
struct GlobalMetrics {
    total_iterations: AtomicU64,
    total_api_calls: AtomicU64,
    total_tokens_input: AtomicU64,
    total_tokens_output: AtomicU64,
    total_tool_calls: AtomicU64,
}

/// Per-loop type aggregate metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TypeMetrics {
    /// Loop type name
    pub loop_type: String,
    /// Number of loops started
    pub loops_started: u64,
    /// Number of loops completed successfully
    pub loops_completed: u64,
    /// Number of loops failed
    pub loops_failed: u64,
    /// Total iterations across all loops of this type
    pub total_iterations: u64,
    /// Average iterations per completed loop
    pub avg_iterations_per_loop: f64,
    /// Total API calls
    pub total_api_calls: u64,
    /// Total tokens consumed
    pub total_tokens: u64,
}

/// Per-loop execution statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoopStats {
    /// Execution ID
    pub exec_id: String,
    /// Loop type
    pub loop_type: String,
    /// Number of iterations completed
    pub iterations: u32,
    /// Total API calls made
    pub api_calls: u32,
    /// Total input tokens
    pub tokens_input: u64,
    /// Total output tokens
    pub tokens_output: u64,
    /// Tool call counts by tool name
    pub tool_calls: HashMap<String, u32>,
    /// Iteration timings (duration in ms)
    pub iteration_times_ms: Vec<u64>,
    /// Start time (Unix ms)
    pub started_at: i64,
    /// End time (Unix ms, 0 if still running)
    pub ended_at: i64,
    /// Final status
    pub final_status: String,
}

impl LoopStats {
    /// Create new stats for an execution
    pub fn new(exec_id: impl Into<String>, loop_type: impl Into<String>) -> Self {
        Self {
            exec_id: exec_id.into(),
            loop_type: loop_type.into(),
            started_at: taskstore::now_ms(),
            ..Default::default()
        }
    }

    /// Record an iteration completion
    pub fn record_iteration(&mut self, duration: Duration) {
        self.iterations += 1;
        self.iteration_times_ms.push(duration.as_millis() as u64);
    }

    /// Record an API call
    pub fn record_api_call(&mut self, input_tokens: u64, output_tokens: u64) {
        self.api_calls += 1;
        self.tokens_input += input_tokens;
        self.tokens_output += output_tokens;
    }

    /// Record a tool call
    pub fn record_tool_call(&mut self, tool_name: &str) {
        *self.tool_calls.entry(tool_name.to_string()).or_default() += 1;
    }

    /// Mark completion
    pub fn mark_complete(&mut self, status: &str) {
        self.ended_at = taskstore::now_ms();
        self.final_status = status.to_string();
    }

    /// Calculate average iteration time
    pub fn avg_iteration_time_ms(&self) -> f64 {
        if self.iteration_times_ms.is_empty() {
            0.0
        } else {
            self.iteration_times_ms.iter().sum::<u64>() as f64 / self.iteration_times_ms.len() as f64
        }
    }

    /// Calculate total duration
    pub fn total_duration_ms(&self) -> i64 {
        if self.ended_at > 0 {
            self.ended_at - self.started_at
        } else {
            taskstore::now_ms() - self.started_at
        }
    }
}

impl LoopMetrics {
    /// Create a new metrics tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Start tracking a new loop
    pub fn start_loop(&self, exec_id: &str, loop_type: &str) {
        let stats = LoopStats::new(exec_id, loop_type);

        // Insert loop stats
        if let Ok(mut loops) = self.loops.write() {
            loops.insert(exec_id.to_string(), stats);
        }

        // Update type metrics
        if let Ok(mut type_metrics) = self.type_metrics.write() {
            let metrics = type_metrics
                .entry(loop_type.to_string())
                .or_insert_with(|| TypeMetrics {
                    loop_type: loop_type.to_string(),
                    ..Default::default()
                });
            metrics.loops_started += 1;
        }
    }

    /// Record an iteration for a loop
    pub fn record_iteration(&self, exec_id: &str, duration: Duration) {
        if let Ok(mut loops) = self.loops.write()
            && let Some(stats) = loops.get_mut(exec_id)
        {
            stats.record_iteration(duration);
        }
        self.global.total_iterations.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an API call for a loop
    pub fn record_api_call(&self, exec_id: &str, input_tokens: u64, output_tokens: u64) {
        if let Ok(mut loops) = self.loops.write()
            && let Some(stats) = loops.get_mut(exec_id)
        {
            stats.record_api_call(input_tokens, output_tokens);
        }
        self.global.total_api_calls.fetch_add(1, Ordering::Relaxed);
        self.global
            .total_tokens_input
            .fetch_add(input_tokens, Ordering::Relaxed);
        self.global
            .total_tokens_output
            .fetch_add(output_tokens, Ordering::Relaxed);
    }

    /// Record a tool call for a loop
    pub fn record_tool_call(&self, exec_id: &str, tool_name: &str) {
        if let Ok(mut loops) = self.loops.write()
            && let Some(stats) = loops.get_mut(exec_id)
        {
            stats.record_tool_call(tool_name);
        }
        self.global.total_tool_calls.fetch_add(1, Ordering::Relaxed);
    }

    /// Mark a loop as complete
    pub fn complete_loop(&self, exec_id: &str, status: &str) {
        let loop_type = if let Ok(mut loops) = self.loops.write()
            && let Some(stats) = loops.get_mut(exec_id)
        {
            stats.mark_complete(status);
            Some((
                stats.loop_type.clone(),
                stats.iterations,
                stats.api_calls,
                stats.tokens_input + stats.tokens_output,
            ))
        } else {
            None
        };

        // Update type aggregate metrics
        if let Some((loop_type, iterations, api_calls, tokens)) = loop_type
            && let Ok(mut type_metrics) = self.type_metrics.write()
            && let Some(metrics) = type_metrics.get_mut(&loop_type)
        {
            if status == "complete" {
                metrics.loops_completed += 1;
            } else {
                metrics.loops_failed += 1;
            }
            metrics.total_iterations += iterations as u64;
            metrics.total_api_calls += api_calls as u64;
            metrics.total_tokens += tokens;

            // Update average
            if metrics.loops_completed > 0 {
                metrics.avg_iterations_per_loop = metrics.total_iterations as f64 / metrics.loops_completed as f64;
            }
        }
    }

    /// Get stats for a specific loop
    pub fn get_loop_stats(&self, exec_id: &str) -> Option<LoopStats> {
        self.loops.read().ok()?.get(exec_id).cloned()
    }

    /// Get aggregate metrics for a loop type
    pub fn get_type_metrics(&self, loop_type: &str) -> Option<TypeMetrics> {
        self.type_metrics.read().ok()?.get(loop_type).cloned()
    }

    /// Get all type metrics
    pub fn all_type_metrics(&self) -> Vec<TypeMetrics> {
        self.type_metrics
            .read()
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get global summary
    pub fn global_summary(&self) -> GlobalSummary {
        GlobalSummary {
            total_iterations: self.global.total_iterations.load(Ordering::Relaxed),
            total_api_calls: self.global.total_api_calls.load(Ordering::Relaxed),
            total_tokens_input: self.global.total_tokens_input.load(Ordering::Relaxed),
            total_tokens_output: self.global.total_tokens_output.load(Ordering::Relaxed),
            total_tool_calls: self.global.total_tool_calls.load(Ordering::Relaxed),
            active_loops: self
                .loops
                .read()
                .map(|l| l.values().filter(|s| s.ended_at == 0).count())
                .unwrap_or(0),
        }
    }

    /// Export all metrics as JSON
    pub fn export_json(&self) -> serde_json::Value {
        let loops: Vec<_> = self
            .loops
            .read()
            .map(|l| l.values().cloned().collect())
            .unwrap_or_default();
        let types: Vec<_> = self.all_type_metrics();
        let global = self.global_summary();

        serde_json::json!({
            "global": global,
            "type_metrics": types,
            "loops": loops,
        })
    }
}

/// Global metrics summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSummary {
    pub total_iterations: u64,
    pub total_api_calls: u64,
    pub total_tokens_input: u64,
    pub total_tokens_output: u64,
    pub total_tool_calls: u64,
    pub active_loops: usize,
}

/// Timing helper for iterations
pub struct IterationTimer {
    start: Instant,
    exec_id: String,
}

impl IterationTimer {
    /// Start timing an iteration
    pub fn start(exec_id: impl Into<String>) -> Self {
        Self {
            start: Instant::now(),
            exec_id: exec_id.into(),
        }
    }

    /// Stop timing and record to metrics
    pub fn stop(self, metrics: &LoopMetrics) {
        let duration = self.start.elapsed();
        metrics.record_iteration(&self.exec_id, duration);
    }

    /// Get elapsed duration without stopping
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_stats_new() {
        let stats = LoopStats::new("exec-1", "phase");
        assert_eq!(stats.exec_id, "exec-1");
        assert_eq!(stats.loop_type, "phase");
        assert_eq!(stats.iterations, 0);
        assert!(stats.started_at > 0);
    }

    #[test]
    fn test_loop_stats_record_iteration() {
        let mut stats = LoopStats::new("exec-1", "phase");
        stats.record_iteration(Duration::from_millis(100));
        stats.record_iteration(Duration::from_millis(200));

        assert_eq!(stats.iterations, 2);
        assert_eq!(stats.iteration_times_ms, vec![100, 200]);
        assert_eq!(stats.avg_iteration_time_ms(), 150.0);
    }

    #[test]
    fn test_loop_stats_record_api_call() {
        let mut stats = LoopStats::new("exec-1", "phase");
        stats.record_api_call(100, 50);
        stats.record_api_call(200, 100);

        assert_eq!(stats.api_calls, 2);
        assert_eq!(stats.tokens_input, 300);
        assert_eq!(stats.tokens_output, 150);
    }

    #[test]
    fn test_loop_stats_record_tool_call() {
        let mut stats = LoopStats::new("exec-1", "phase");
        stats.record_tool_call("read");
        stats.record_tool_call("write");
        stats.record_tool_call("read");

        assert_eq!(stats.tool_calls.get("read"), Some(&2));
        assert_eq!(stats.tool_calls.get("write"), Some(&1));
    }

    #[test]
    fn test_metrics_start_and_complete_loop() {
        let metrics = LoopMetrics::new();

        metrics.start_loop("exec-1", "phase");
        metrics.record_iteration("exec-1", Duration::from_millis(100));
        metrics.record_api_call("exec-1", 500, 200);
        metrics.complete_loop("exec-1", "complete");

        let stats = metrics.get_loop_stats("exec-1").unwrap();
        assert_eq!(stats.iterations, 1);
        assert_eq!(stats.api_calls, 1);
        assert_eq!(stats.final_status, "complete");

        let type_metrics = metrics.get_type_metrics("phase").unwrap();
        assert_eq!(type_metrics.loops_started, 1);
        assert_eq!(type_metrics.loops_completed, 1);
    }

    #[test]
    fn test_metrics_global_summary() {
        let metrics = LoopMetrics::new();

        metrics.start_loop("exec-1", "phase");
        metrics.start_loop("exec-2", "spec");
        metrics.record_iteration("exec-1", Duration::from_millis(100));
        metrics.record_iteration("exec-2", Duration::from_millis(200));
        metrics.record_api_call("exec-1", 100, 50);

        let summary = metrics.global_summary();
        assert_eq!(summary.total_iterations, 2);
        assert_eq!(summary.total_api_calls, 1);
        assert_eq!(summary.active_loops, 2);
    }

    #[test]
    fn test_iteration_timer() {
        let metrics = LoopMetrics::new();
        metrics.start_loop("exec-1", "phase");

        let timer = IterationTimer::start("exec-1");
        std::thread::sleep(Duration::from_millis(10));
        assert!(timer.elapsed() >= Duration::from_millis(10));
        timer.stop(&metrics);

        let stats = metrics.get_loop_stats("exec-1").unwrap();
        assert_eq!(stats.iterations, 1);
        assert!(stats.iteration_times_ms[0] >= 10);
    }
}
