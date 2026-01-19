# Spec: Rate Limiting System

**ID:** 013-rate-limiting  
**Status:** Draft  
**Dependencies:** [011-priority-scheduler]

## Summary

Implement comprehensive rate limiting to manage API usage, prevent resource exhaustion, and ensure fair access to external services. The system should support multiple rate limiting strategies and provide backpressure mechanisms.

## Acceptance Criteria

1. **Rate Limit Types**
   - Token bucket algorithm
   - Sliding window counters
   - Per-service limits
   - Global limits

2. **Limit Configuration**
   - Per-API endpoint limits
   - Per-loop-type limits
   - Time-based windows
   - Burst allowances

3. **Backpressure Handling**
   - Queue when rate limited
   - Priority-based access
   - Graceful degradation
   - Retry strategies

4. **Monitoring & Alerts**
   - Usage metrics
   - Limit approaching warnings
   - Breach notifications
   - Trend analysis

## Implementation Phases

### Phase 1: Core Algorithms
- Token bucket implementation
- Sliding window counter
- Rate limiter trait
- Basic enforcement

### Phase 2: Configuration System
- Limit definitions
- Service mapping
- Dynamic updates
- Validation logic

### Phase 3: Backpressure
- Request queuing
- Priority handling
- Retry logic
- Circuit breakers

### Phase 4: Observability
- Metrics collection
- Alert generation
- Dashboard data
- Usage reports

## Technical Details

### Module Structure
```
src/rate_limiting/
├── mod.rs
├── algorithms.rs  # Rate limiting algorithms
├── limiter.rs     # Main rate limiter
├── config.rs      # Configuration types
├── backpressure.rs # Backpressure handling
└── metrics.rs     # Usage metrics
```

### Core Types
```rust
pub trait RateLimiter: Send + Sync {
    async fn check_and_consume(&self, tokens: u32) -> Result<(), RateLimitError>;
    async fn try_consume(&self, tokens: u32) -> bool;
    fn available_tokens(&self) -> u32;
    fn reset_time(&self) -> Option<DateTime<Utc>>;
}

pub struct TokenBucket {
    capacity: u32,
    tokens: Arc<AtomicU32>,
    refill_rate: u32,
    refill_interval: Duration,
    last_refill: Arc<Mutex<Instant>>,
}

pub struct RateLimitConfig {
    pub llm_api: ServiceLimits,
    pub git_operations: ServiceLimits,
    pub file_operations: ServiceLimits,
    pub global_limits: GlobalLimits,
}

pub struct ServiceLimits {
    pub requests_per_minute: u32,
    pub requests_per_hour: u32,
    pub burst_capacity: u32,
    pub token_limits: Option<TokenLimits>,
}
```

### Multi-Level Rate Limiting
```rust
pub struct HierarchicalRateLimiter {
    global: Box<dyn RateLimiter>,
    per_service: HashMap<String, Box<dyn RateLimiter>>,
    per_loop: HashMap<LoopId, Box<dyn RateLimiter>>,
}

impl HierarchicalRateLimiter {
    pub async fn acquire(&self, context: &RequestContext) -> Result<RateToken, RateLimitError> {
        // Check global limits first
        self.global.check_and_consume(1).await?;
        
        // Then service-specific
        if let Some(limiter) = self.per_service.get(&context.service) {
            limiter.check_and_consume(context.weight).await?;
        }
        
        // Finally per-loop limits
        if let Some(limiter) = self.per_loop.get(&context.loop_id) {
            limiter.check_and_consume(context.weight).await?;
        }
        
        Ok(RateToken::new(context))
    }
}
```

### Backpressure Strategies
1. **Queue with timeout**: Hold requests until tokens available
2. **Priority queue**: High-priority requests get tokens first
3. **Exponential backoff**: Increasing delays between retries
4. **Circuit breaker**: Stop attempts after repeated failures

## Notes

- Rate limiters should be persistent across daemon restarts
- Consider implementing adaptive rate limiting based on error rates
- Provide clear feedback to loops about rate limit status
- Support for external rate limit headers (e.g., X-RateLimit-*)