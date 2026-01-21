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
use tracing::debug;

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
        let exec_id_str = exec_id.into();
        let loop_type_str = loop_type.into();
        debug!(exec_id = %exec_id_str, loop_type = %loop_type_str, "LoopStats::new: called");
        Self {
            exec_id: exec_id_str,
            loop_type: loop_type_str,
            started_at: taskstore::now_ms(),
            ..Default::default()
        }
    }

    /// Record an iteration completion
    pub fn record_iteration(&mut self, duration: Duration) {
        debug!(exec_id = %self.exec_id, duration_ms = duration.as_millis() as u64, "LoopStats::record_iteration: called");
        self.iterations += 1;
        self.iteration_times_ms.push(duration.as_millis() as u64);
    }

    /// Record an API call
    pub fn record_api_call(&mut self, input_tokens: u64, output_tokens: u64) {
        debug!(exec_id = %self.exec_id, input_tokens, output_tokens, "LoopStats::record_api_call: called");
        self.api_calls += 1;
        self.tokens_input += input_tokens;
        self.tokens_output += output_tokens;
    }

    /// Record a tool call
    pub fn record_tool_call(&mut self, tool_name: &str) {
        debug!(exec_id = %self.exec_id, %tool_name, "LoopStats::record_tool_call: called");
        *self.tool_calls.entry(tool_name.to_string()).or_default() += 1;
    }

    /// Mark completion
    pub fn mark_complete(&mut self, status: &str) {
        debug!(exec_id = %self.exec_id, %status, "LoopStats::mark_complete: called");
        self.ended_at = taskstore::now_ms();
        self.final_status = status.to_string();
    }

    /// Calculate average iteration time
    pub fn avg_iteration_time_ms(&self) -> f64 {
        debug!(exec_id = %self.exec_id, iteration_count = self.iteration_times_ms.len(), "LoopStats::avg_iteration_time_ms: called");
        if self.iteration_times_ms.is_empty() {
            debug!(exec_id = %self.exec_id, "avg_iteration_time_ms: no iterations");
            0.0
        } else {
            let avg = self.iteration_times_ms.iter().sum::<u64>() as f64 / self.iteration_times_ms.len() as f64;
            debug!(exec_id = %self.exec_id, avg, "avg_iteration_time_ms: calculated");
            avg
        }
    }

    /// Calculate total duration
    pub fn total_duration_ms(&self) -> i64 {
        debug!(exec_id = %self.exec_id, ended_at = self.ended_at, started_at = self.started_at, "LoopStats::total_duration_ms: called");
        if self.ended_at > 0 {
            debug!(exec_id = %self.exec_id, "total_duration_ms: loop ended");
            self.ended_at - self.started_at
        } else {
            debug!(exec_id = %self.exec_id, "total_duration_ms: loop still running");
            taskstore::now_ms() - self.started_at
        }
    }
}

impl LoopMetrics {
    /// Create a new metrics tracker
    pub fn new() -> Self {
        debug!("LoopMetrics::new: called");
        Self::default()
    }

    /// Start tracking a new loop
    pub fn start_loop(&self, exec_id: &str, loop_type: &str) {
        debug!(%exec_id, %loop_type, "LoopMetrics::start_loop: called");
        let stats = LoopStats::new(exec_id, loop_type);

        // Insert loop stats
        if let Ok(mut loops) = self.loops.write() {
            debug!(%exec_id, "start_loop: inserting loop stats");
            loops.insert(exec_id.to_string(), stats);
        } else {
            debug!(%exec_id, "start_loop: failed to acquire loops write lock");
        }

        // Update type metrics
        if let Ok(mut type_metrics) = self.type_metrics.write() {
            debug!(%exec_id, %loop_type, "start_loop: updating type metrics");
            let metrics = type_metrics
                .entry(loop_type.to_string())
                .or_insert_with(|| TypeMetrics {
                    loop_type: loop_type.to_string(),
                    ..Default::default()
                });
            metrics.loops_started += 1;
        } else {
            debug!(%exec_id, "start_loop: failed to acquire type_metrics write lock");
        }
    }

    /// Record an iteration for a loop
    pub fn record_iteration(&self, exec_id: &str, duration: Duration) {
        debug!(%exec_id, duration_ms = duration.as_millis() as u64, "LoopMetrics::record_iteration: called");
        if let Ok(mut loops) = self.loops.write()
            && let Some(stats) = loops.get_mut(exec_id)
        {
            debug!(%exec_id, "record_iteration: recording to stats");
            stats.record_iteration(duration);
        } else {
            debug!(%exec_id, "record_iteration: loop not found or lock failed");
        }
        self.global.total_iterations.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an API call for a loop
    pub fn record_api_call(&self, exec_id: &str, input_tokens: u64, output_tokens: u64) {
        debug!(%exec_id, input_tokens, output_tokens, "LoopMetrics::record_api_call: called");
        if let Ok(mut loops) = self.loops.write()
            && let Some(stats) = loops.get_mut(exec_id)
        {
            debug!(%exec_id, "record_api_call: recording to stats");
            stats.record_api_call(input_tokens, output_tokens);
        } else {
            debug!(%exec_id, "record_api_call: loop not found or lock failed");
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
        debug!(%exec_id, %tool_name, "LoopMetrics::record_tool_call: called");
        if let Ok(mut loops) = self.loops.write()
            && let Some(stats) = loops.get_mut(exec_id)
        {
            debug!(%exec_id, "record_tool_call: recording to stats");
            stats.record_tool_call(tool_name);
        } else {
            debug!(%exec_id, "record_tool_call: loop not found or lock failed");
        }
        self.global.total_tool_calls.fetch_add(1, Ordering::Relaxed);
    }

    /// Mark a loop as complete
    pub fn complete_loop(&self, exec_id: &str, status: &str) {
        debug!(%exec_id, %status, "LoopMetrics::complete_loop: called");
        let loop_type = if let Ok(mut loops) = self.loops.write()
            && let Some(stats) = loops.get_mut(exec_id)
        {
            debug!(%exec_id, "complete_loop: marking stats complete");
            stats.mark_complete(status);
            Some((
                stats.loop_type.clone(),
                stats.iterations,
                stats.api_calls,
                stats.tokens_input + stats.tokens_output,
            ))
        } else {
            debug!(%exec_id, "complete_loop: loop not found or lock failed");
            None
        };

        // Update type aggregate metrics
        if let Some((loop_type, iterations, api_calls, tokens)) = loop_type
            && let Ok(mut type_metrics) = self.type_metrics.write()
            && let Some(metrics) = type_metrics.get_mut(&loop_type)
        {
            if status == "complete" {
                debug!(%exec_id, %loop_type, "complete_loop: incrementing loops_completed");
                metrics.loops_completed += 1;
            } else {
                debug!(%exec_id, %loop_type, "complete_loop: incrementing loops_failed");
                metrics.loops_failed += 1;
            }
            metrics.total_iterations += iterations as u64;
            metrics.total_api_calls += api_calls as u64;
            metrics.total_tokens += tokens;

            // Update average
            if metrics.loops_completed > 0 {
                metrics.avg_iterations_per_loop = metrics.total_iterations as f64 / metrics.loops_completed as f64;
                debug!(%exec_id, %loop_type, avg = metrics.avg_iterations_per_loop, "complete_loop: updated average");
            }
        } else {
            debug!(%exec_id, "complete_loop: could not update type metrics");
        }
    }

    /// Get stats for a specific loop
    pub fn get_loop_stats(&self, exec_id: &str) -> Option<LoopStats> {
        debug!(%exec_id, "LoopMetrics::get_loop_stats: called");
        let result = self.loops.read().ok()?.get(exec_id).cloned();
        debug!(%exec_id, found = result.is_some(), "get_loop_stats: returning");
        result
    }

    /// Get aggregate metrics for a loop type
    pub fn get_type_metrics(&self, loop_type: &str) -> Option<TypeMetrics> {
        debug!(%loop_type, "LoopMetrics::get_type_metrics: called");
        let result = self.type_metrics.read().ok()?.get(loop_type).cloned();
        debug!(%loop_type, found = result.is_some(), "get_type_metrics: returning");
        result
    }

    /// Get all type metrics
    pub fn all_type_metrics(&self) -> Vec<TypeMetrics> {
        debug!("LoopMetrics::all_type_metrics: called");
        let result: Vec<_> = self
            .type_metrics
            .read()
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default();
        debug!(count = result.len(), "all_type_metrics: returning");
        result
    }

    /// Get global summary
    pub fn global_summary(&self) -> GlobalSummary {
        debug!("LoopMetrics::global_summary: called");
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
        debug!("LoopMetrics::export_json: called");
        let loops: Vec<_> = self
            .loops
            .read()
            .map(|l| l.values().cloned().collect())
            .unwrap_or_default();
        let types: Vec<_> = self.all_type_metrics();
        let global = self.global_summary();
        debug!(
            loop_count = loops.len(),
            type_count = types.len(),
            "export_json: returning"
        );

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
        let exec_id_str = exec_id.into();
        debug!(exec_id = %exec_id_str, "IterationTimer::start: called");
        Self {
            start: Instant::now(),
            exec_id: exec_id_str,
        }
    }

    /// Stop timing and record to metrics
    pub fn stop(self, metrics: &LoopMetrics) {
        let duration = self.start.elapsed();
        debug!(exec_id = %self.exec_id, duration_ms = duration.as_millis() as u64, "IterationTimer::stop: called");
        metrics.record_iteration(&self.exec_id, duration);
    }

    /// Get elapsed duration without stopping
    pub fn elapsed(&self) -> Duration {
        debug!(exec_id = %self.exec_id, "IterationTimer::elapsed: called");
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
