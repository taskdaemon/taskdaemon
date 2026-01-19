# Spec: Analytics and Metrics System

**ID:** 023-analytics-metrics
**Status:** Draft
**Dependencies:** [009-execution-tracking]

## Summary

Build an analytics and metrics system that collects performance data, tracks usage patterns, and provides insights into system behavior. Support both real-time monitoring and historical analysis with efficient storage and querying.

## Acceptance Criteria

1. **Metrics Collection**
   - System performance metrics
   - Loop execution metrics
   - Resource usage tracking
   - Error rate monitoring

2. **Storage System**
   - Time-series data storage
   - Efficient compression
   - Retention policies
   - Aggregation support

3. **Query Interface**
   - Time-range queries
   - Metric aggregation
   - Percentile calculations
   - Custom queries

4. **Visualization**
   - Real-time dashboards
   - Historical trends
   - Alert integration
   - Export capabilities

## Implementation Phases

### Phase 1: Metrics Framework
- Define metric types
- Collection infrastructure
- Basic storage
- Simple queries

### Phase 2: Storage System
- Time-series database
- Data compression
- Retention management
- Index optimization

### Phase 3: Analytics Engine
- Aggregation pipeline
- Statistical functions
- Anomaly detection
- Performance analysis

### Phase 4: Visualization
- Dashboard framework
- Real-time updates
- Alert system
- Report generation

## Technical Details

### Module Structure
```
src/analytics/
├── mod.rs
├── metrics.rs     # Metric definitions
├── collector.rs   # Collection system
├── storage.rs     # Storage backend
├── query.rs       # Query engine
├── aggregator.rs  # Aggregation logic
├── dashboard.rs   # Dashboard data
└── alerts.rs      # Alert system
```

### Metric Types
```rust
pub enum MetricType {
    Counter(Counter),
    Gauge(Gauge),
    Histogram(Histogram),
    Summary(Summary),
}

pub struct Counter {
    pub name: String,
    pub labels: HashMap<String, String>,
    pub value: AtomicU64,
    pub created_at: DateTime<Utc>,
}

pub struct Gauge {
    pub name: String,
    pub labels: HashMap<String, String>,
    pub value: AtomicI64,
    pub updated_at: DateTime<Utc>,
}

pub struct Histogram {
    pub name: String,
    pub labels: HashMap<String, String>,
    pub buckets: Vec<HistogramBucket>,
    pub sum: AtomicU64,
    pub count: AtomicU64,
}

pub struct Summary {
    pub name: String,
    pub labels: HashMap<String, String>,
    pub quantiles: Vec<(f64, f64)>,
    pub sum: AtomicU64,
    pub count: AtomicU64,
}
```

### Metrics Collector
```rust
pub struct MetricsCollector {
    registry: Arc<RwLock<MetricRegistry>>,
    storage: Arc<dyn MetricStorage>,
    flush_interval: Duration,
    aggregation_rules: Vec<AggregationRule>,
}

impl MetricsCollector {
    pub async fn collect_loop_metrics(&self, execution: &LoopExecution) {
        // Execution time
        self.record_histogram(
            "loop_execution_duration",
            hashmap! {
                "loop_type" => execution.loop_type.to_string(),
                "status" => execution.status.to_string(),
            },
            execution.duration().as_secs_f64(),
        ).await;

        // Token usage
        self.record_counter(
            "llm_tokens_used",
            hashmap! {
                "loop_type" => execution.loop_type.to_string(),
                "token_type" => "prompt",
            },
            execution.metrics.total_tokens.prompt_tokens,
        ).await;

        // Tool usage
        for (tool, count) in &execution.metrics.tool_invocations {
            self.record_counter(
                "tool_invocations",
                hashmap! {
                    "tool" => tool.clone(),
                    "loop_type" => execution.loop_type.to_string(),
                },
                *count as u64,
            ).await;
        }

        // Error tracking
        if execution.error.is_some() {
            self.record_counter(
                "loop_errors",
                hashmap! {
                    "loop_type" => execution.loop_type.to_string(),
                    "error_type" => classify_error(&execution.error),
                },
                1,
            ).await;
        }
    }

    pub async fn collect_system_metrics(&self) {
        // CPU usage
        let cpu_percent = sys_info::cpu_usage();
        self.record_gauge(
            "system_cpu_usage",
            hashmap! {},
            (cpu_percent * 100.0) as i64,
        ).await;

        // Memory usage
        let memory = sys_info::mem_info().unwrap();
        self.record_gauge(
            "system_memory_used",
            hashmap! {},
            (memory.total - memory.free) as i64,
        ).await;

        // Active loops
        let active_loops = self.count_active_loops().await;
        self.record_gauge(
            "active_loops",
            hashmap! {},
            active_loops as i64,
        ).await;
    }
}
```

### Time-Series Storage
```rust
pub struct TimeSeriesStorage {
    data_dir: PathBuf,
    retention_policy: RetentionPolicy,
    compressor: Box<dyn Compressor>,
    indices: Arc<RwLock<MetricIndices>>,
}

impl TimeSeriesStorage {
    pub async fn write_metric(&self, metric: &MetricPoint) -> Result<(), StorageError> {
        // Determine shard based on timestamp
        let shard = self.get_shard(metric.timestamp);

        // Compress metric
        let compressed = self.compressor.compress(metric)?;

        // Write to shard file
        shard.append(compressed).await?;

        // Update indices
        self.indices.write().await.index_metric(metric);

        Ok(())
    }

    pub async fn query(
        &self,
        query: &MetricQuery,
    ) -> Result<Vec<MetricPoint>, StorageError> {
        // Use indices to find relevant shards
        let shards = self.indices.read().await.find_shards(query);

        // Read from shards in parallel
        let mut results = Vec::new();
        let futures: Vec<_> = shards.into_iter()
            .map(|shard| self.read_shard(shard, query))
            .collect();

        for result in join_all(futures).await {
            results.extend(result?);
        }

        // Apply query filters and aggregations
        self.apply_query_operations(results, query)
    }
}
```

### Analytics Queries
```rust
pub struct AnalyticsEngine {
    storage: Arc<TimeSeriesStorage>,
    cache: Arc<QueryCache>,
}

impl AnalyticsEngine {
    pub async fn loop_performance_stats(
        &self,
        loop_type: &str,
        time_range: TimeRange,
    ) -> Result<PerformanceStats, Error> {
        let query = MetricQuery {
            metric_names: vec!["loop_execution_duration".to_string()],
            filters: vec![
                Filter::Label("loop_type".to_string(), loop_type.to_string()),
            ],
            time_range,
            aggregation: Some(Aggregation::Percentiles(vec![0.5, 0.95, 0.99])),
        };

        let results = self.storage.query(&query).await?;

        Ok(PerformanceStats {
            median_duration: results.percentile(0.5),
            p95_duration: results.percentile(0.95),
            p99_duration: results.percentile(0.99),
            total_executions: results.count(),
            success_rate: self.calculate_success_rate(&results),
        })
    }

    pub async fn token_usage_report(
        &self,
        time_range: TimeRange,
    ) -> Result<TokenUsageReport, Error> {
        let query = MetricQuery {
            metric_names: vec!["llm_tokens_used".to_string()],
            filters: vec![],
            time_range,
            aggregation: Some(Aggregation::Sum),
            group_by: vec!["loop_type".to_string(), "token_type".to_string()],
        };

        let results = self.storage.query(&query).await?;

        Ok(TokenUsageReport {
            total_tokens: results.sum(),
            by_loop_type: results.group_sum("loop_type"),
            by_token_type: results.group_sum("token_type"),
            estimated_cost: self.calculate_token_cost(results.sum()),
        })
    }
}
```

### Real-time Dashboard
```rust
pub struct DashboardData {
    pub system_health: SystemHealth,
    pub active_loops: Vec<ActiveLoopInfo>,
    pub recent_errors: Vec<ErrorInfo>,
    pub performance_metrics: PerformanceMetrics,
    pub resource_usage: ResourceUsage,
}

pub struct MetricsDashboard {
    analytics: Arc<AnalyticsEngine>,
    update_interval: Duration,
    subscribers: Arc<RwLock<Vec<DashboardSubscriber>>>,
}

impl MetricsDashboard {
    pub async fn start(&self) {
        let mut interval = tokio::time::interval(self.update_interval);

        loop {
            interval.tick().await;

            let dashboard_data = self.collect_dashboard_data().await;

            // Notify subscribers
            let subscribers = self.subscribers.read().await;
            for subscriber in subscribers.iter() {
                subscriber.update(dashboard_data.clone()).await;
            }
        }
    }

    async fn collect_dashboard_data(&self) -> DashboardData {
        DashboardData {
            system_health: self.calculate_system_health().await,
            active_loops: self.get_active_loops().await,
            recent_errors: self.get_recent_errors().await,
            performance_metrics: self.get_performance_metrics().await,
            resource_usage: self.get_resource_usage().await,
        }
    }
}
```

### Alert Rules
```rust
pub struct AlertRule {
    pub name: String,
    pub condition: AlertCondition,
    pub severity: AlertSeverity,
    pub cooldown: Duration,
    pub actions: Vec<AlertAction>,
}

pub enum AlertCondition {
    MetricThreshold {
        metric: String,
        operator: ComparisonOp,
        threshold: f64,
        duration: Duration,
    },
    ErrorRate {
        threshold: f64,
        window: Duration,
    },
    Custom(Box<dyn Fn(&MetricPoint) -> bool>),
}

pub struct AlertManager {
    rules: Vec<AlertRule>,
    analytics: Arc<AnalyticsEngine>,
    notification_channels: Vec<Box<dyn NotificationChannel>>,
}

impl AlertManager {
    pub async fn evaluate_rules(&self) {
        for rule in &self.rules {
            if let Some(alert) = self.check_rule(rule).await {
                self.fire_alert(alert).await;
            }
        }
    }
}
```

## Notes

- Consider using Prometheus format for metrics compatibility
- Implement efficient compression for long-term storage
- Provide pre-built dashboards for common use cases
- Support custom metrics from user loops