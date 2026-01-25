//! Resource limiter with backpressure support.
//!
//! This module provides comprehensive resource limiting for preventing overload:
//!
//! - **Memory limiting**: Prevent OOM conditions
//! - **Connection limiting**: Cap concurrent connections
//! - **Rate limiting**: Control requests per second
//! - **Backpressure**: Graceful degradation under load
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │                        ResourceLimiter                                │
//! │  ┌────────────────────────────────────────────────────────────────┐  │
//! │  │                    Resource Trackers                           │  │
//! │  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────────┐  │  │
//! │  │  │   Memory    │ │ Connections │ │    Rate (Token Bucket)  │  │  │
//! │  │  │  tracking   │ │  counting   │ │  tokens │ refill_rate   │  │  │
//! │  │  └─────────────┘ └─────────────┘ └─────────────────────────┘  │  │
//! │  └────────────────────────────────────────────────────────────────┘  │
//! │                              │                                        │
//! │  ┌────────────────────────────────────────────────────────────────┐  │
//! │  │                  Backpressure Controller                       │  │
//! │  │  ┌─────────────────────────────────────────────────────────┐  │  │
//! │  │  │ Utilization     0%     50%    75%    90%    100%       │  │  │
//! │  │  │ Response      Allow   Slow   Slower  Warn   Reject     │  │  │
//! │  │  └─────────────────────────────────────────────────────────┘  │  │
//! │  └────────────────────────────────────────────────────────────────┘  │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;

use super::config::{BackpressureStrategyConfig, ResourceLimiterConfig};

/// Resource types that can be limited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceType {
    /// Memory usage (bytes).
    Memory,
    /// Concurrent connections.
    Connections,
    /// Request rate (requests per second).
    RequestRate,
}

impl std::fmt::Display for ResourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Memory => write!(f, "memory"),
            Self::Connections => write!(f, "connections"),
            Self::RequestRate => write!(f, "request_rate"),
        }
    }
}

/// Result of a resource limit check.
#[derive(Debug, Clone)]
pub enum LimitResult {
    /// Request allowed.
    Allowed,
    /// Request allowed but approaching limit.
    AllowedWithWarning {
        resource: ResourceType,
        utilization: f64,
    },
    /// Request delayed (backpressure).
    Delayed {
        resource: ResourceType,
        delay: Duration,
    },
    /// Request rejected (at capacity).
    Rejected {
        resource: ResourceType,
        current: u64,
        limit: u64,
    },
}

impl LimitResult {
    /// Check if the request is allowed (immediately or after delay).
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed | Self::AllowedWithWarning { .. })
    }

    /// Check if the request needs to wait.
    pub fn needs_delay(&self) -> bool {
        matches!(self, Self::Delayed { .. })
    }

    /// Check if the request was rejected.
    pub fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected { .. })
    }

    /// Get the delay duration if any.
    pub fn delay(&self) -> Option<Duration> {
        match self {
            Self::Delayed { delay, .. } => Some(*delay),
            _ => None,
        }
    }
}

/// Backpressure strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackpressureStrategy {
    /// No backpressure - reject when limits exceeded.
    None,
    /// Gradual slowdown as resources approach limits.
    Gradual,
    /// Adaptive strategy that adjusts based on system state.
    Adaptive,
    /// Aggressive strategy for high-load scenarios.
    Aggressive,
}

impl From<BackpressureStrategyConfig> for BackpressureStrategy {
    fn from(config: BackpressureStrategyConfig) -> Self {
        match config {
            BackpressureStrategyConfig::None => Self::None,
            BackpressureStrategyConfig::Gradual => Self::Gradual,
            BackpressureStrategyConfig::Adaptive => Self::Adaptive,
            BackpressureStrategyConfig::Aggressive => Self::Aggressive,
        }
    }
}

/// Token bucket for rate limiting.
struct TokenBucket {
    /// Current tokens.
    tokens: AtomicU64,
    /// Maximum tokens (capacity).
    max_tokens: u64,
    /// Tokens added per second.
    refill_rate: u64,
    /// Last refill time.
    last_refill: RwLock<Instant>,
}

impl TokenBucket {
    fn new(max_tokens: u64, refill_rate: u64) -> Self {
        Self {
            tokens: AtomicU64::new(max_tokens),
            max_tokens,
            refill_rate,
            last_refill: RwLock::new(Instant::now()),
        }
    }

    /// Try to consume a token, return true if successful.
    fn try_consume(&self, count: u64) -> bool {
        self.refill();

        loop {
            let current = self.tokens.load(Ordering::Acquire);
            if current < count {
                return false;
            }

            if self.tokens.compare_exchange_weak(
                current,
                current - count,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ).is_ok() {
                return true;
            }
        }
    }

    /// Refill tokens based on elapsed time.
    fn refill(&self) {
        let mut last = self.last_refill.write();
        let elapsed = last.elapsed();
        let tokens_to_add = (elapsed.as_secs_f64() * self.refill_rate as f64) as u64;

        if tokens_to_add > 0 {
            *last = Instant::now();

            loop {
                let current = self.tokens.load(Ordering::Acquire);
                let new_tokens = (current + tokens_to_add).min(self.max_tokens);

                if self.tokens.compare_exchange_weak(
                    current,
                    new_tokens,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    break;
                }
            }
        }
    }

    /// Get current token count.
    fn current(&self) -> u64 {
        self.refill();
        self.tokens.load(Ordering::Relaxed)
    }

    /// Get utilization (0.0 = full, 1.0 = empty).
    fn utilization(&self) -> f64 {
        let current = self.current() as f64;
        1.0 - (current / self.max_tokens as f64)
    }

    /// Calculate delay needed to get a token.
    fn delay_for_token(&self) -> Duration {
        let current = self.current();
        if current > 0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(1.0 / self.refill_rate as f64)
        }
    }
}

/// Snapshot of current resource usage.
#[derive(Debug, Clone)]
pub struct ResourceSnapshot {
    /// Memory usage (bytes).
    pub memory_bytes: u64,
    /// Memory limit (bytes).
    pub memory_limit: u64,
    /// Current connections.
    pub connections: usize,
    /// Connection limit.
    pub connection_limit: usize,
    /// Current request rate.
    pub request_rate: f64,
    /// Request rate limit.
    pub request_rate_limit: u64,
    /// Timestamp.
    pub timestamp: Instant,
}

impl ResourceSnapshot {
    /// Get memory utilization (0.0-1.0).
    pub fn memory_utilization(&self) -> f64 {
        self.memory_bytes as f64 / self.memory_limit as f64
    }

    /// Get connection utilization (0.0-1.0).
    pub fn connection_utilization(&self) -> f64 {
        self.connections as f64 / self.connection_limit as f64
    }

    /// Get overall utilization (max of all resources).
    pub fn overall_utilization(&self) -> f64 {
        self.memory_utilization()
            .max(self.connection_utilization())
    }
}

/// Statistics for the resource limiter.
#[derive(Debug, Clone, Default)]
pub struct LimiterStatistics {
    /// Total requests checked.
    pub total_checks: u64,
    /// Requests allowed.
    pub allowed: u64,
    /// Requests allowed with warning.
    pub warned: u64,
    /// Requests delayed.
    pub delayed: u64,
    /// Requests rejected.
    pub rejected: u64,
    /// Total delay applied (microseconds).
    pub total_delay_us: u64,
    /// Rejections by resource type.
    pub rejections_by_type: [u64; 3], // Memory, Connections, Rate
}

/// Resource limiter with backpressure support.
pub struct ResourceLimiter {
    /// Configuration.
    config: ResourceLimiterConfig,

    /// Backpressure strategy.
    strategy: BackpressureStrategy,

    /// Current memory usage (tracked externally).
    memory_used: AtomicU64,

    /// Current connection count (tracked externally).
    connections: AtomicUsize,

    /// Token bucket for rate limiting.
    rate_bucket: TokenBucket,

    /// Statistics.
    stats: RwLock<LimiterStatistics>,

    /// Creation time.
    created_at: Instant,
}

impl ResourceLimiter {
    /// Create a new resource limiter.
    pub fn new(config: ResourceLimiterConfig) -> Self {
        let strategy = config.strategy.into();
        let bucket_size = config.max_requests_per_second;

        Self {
            strategy,
            memory_used: AtomicU64::new(0),
            connections: AtomicUsize::new(0),
            rate_bucket: TokenBucket::new(bucket_size, config.max_requests_per_second),
            config,
            stats: RwLock::new(LimiterStatistics::default()),
            created_at: Instant::now(),
        }
    }

    /// Create with default configuration.
    pub fn default_config() -> Self {
        Self::new(ResourceLimiterConfig::default())
    }

    /// Check if a request should be allowed.
    pub fn check(&self) -> LimitResult {
        let mut stats = self.stats.write();
        stats.total_checks += 1;

        // Check memory
        let memory_result = self.check_memory();
        if memory_result.is_rejected() {
            stats.rejected += 1;
            stats.rejections_by_type[0] += 1;
            return memory_result;
        }

        // Check connections
        let conn_result = self.check_connections();
        if conn_result.is_rejected() {
            stats.rejected += 1;
            stats.rejections_by_type[1] += 1;
            return conn_result;
        }

        // Check rate
        let rate_result = self.check_rate();
        if rate_result.is_rejected() {
            stats.rejected += 1;
            stats.rejections_by_type[2] += 1;
            return rate_result;
        }

        // Combine results
        let combined = self.combine_results(memory_result, conn_result, rate_result);

        match &combined {
            LimitResult::Allowed => stats.allowed += 1,
            LimitResult::AllowedWithWarning { .. } => stats.warned += 1,
            LimitResult::Delayed { delay, .. } => {
                stats.delayed += 1;
                stats.total_delay_us += delay.as_micros() as u64;
            }
            LimitResult::Rejected { .. } => stats.rejected += 1,
        }

        combined
    }

    /// Check if a request should be allowed, consuming rate token if so.
    pub fn try_acquire(&self) -> LimitResult {
        let result = self.check();

        if result.is_allowed() || result.needs_delay() {
            // Actually consume the rate token
            self.rate_bucket.try_consume(1);
        }

        result
    }

    /// Check memory limits.
    fn check_memory(&self) -> LimitResult {
        let used = self.memory_used.load(Ordering::Relaxed);
        let limit = self.config.max_memory_bytes;
        let utilization = used as f64 / limit as f64;

        if utilization >= 1.0 {
            return LimitResult::Rejected {
                resource: ResourceType::Memory,
                current: used,
                limit,
            };
        }

        self.apply_backpressure(ResourceType::Memory, utilization)
    }

    /// Check connection limits.
    fn check_connections(&self) -> LimitResult {
        let current = self.connections.load(Ordering::Relaxed);
        let limit = self.config.max_connections;
        let utilization = current as f64 / limit as f64;

        if current >= limit {
            return LimitResult::Rejected {
                resource: ResourceType::Connections,
                current: current as u64,
                limit: limit as u64,
            };
        }

        self.apply_backpressure(ResourceType::Connections, utilization)
    }

    /// Check rate limits.
    fn check_rate(&self) -> LimitResult {
        let utilization = self.rate_bucket.utilization();

        if utilization >= 1.0 {
            let delay = self.rate_bucket.delay_for_token();
            if self.strategy == BackpressureStrategy::None {
                return LimitResult::Rejected {
                    resource: ResourceType::RequestRate,
                    current: 0,
                    limit: self.config.max_requests_per_second,
                };
            } else {
                return LimitResult::Delayed {
                    resource: ResourceType::RequestRate,
                    delay,
                };
            }
        }

        self.apply_backpressure(ResourceType::RequestRate, utilization)
    }

    /// Apply backpressure based on strategy and utilization.
    fn apply_backpressure(&self, resource: ResourceType, utilization: f64) -> LimitResult {
        let threshold = self.config.backpressure_threshold;

        if utilization < threshold {
            return LimitResult::Allowed;
        }

        match self.strategy {
            BackpressureStrategy::None => {
                if utilization >= 1.0 {
                    LimitResult::Rejected {
                        resource,
                        current: 0,
                        limit: 0,
                    }
                } else {
                    LimitResult::Allowed
                }
            }
            BackpressureStrategy::Gradual => {
                // Linear delay increase above threshold
                let excess = (utilization - threshold) / (1.0 - threshold);
                if excess > 0.0 {
                    let delay_ms = (excess * 100.0) as u64; // Max 100ms delay
                    LimitResult::AllowedWithWarning {
                        resource,
                        utilization,
                    }
                } else {
                    LimitResult::Allowed
                }
            }
            BackpressureStrategy::Adaptive => {
                // Exponential response based on utilization
                if utilization > 0.9 {
                    let delay_ms = ((utilization - 0.9) * 1000.0) as u64;
                    LimitResult::Delayed {
                        resource,
                        delay: Duration::from_millis(delay_ms.min(100)),
                    }
                } else if utilization > threshold {
                    LimitResult::AllowedWithWarning {
                        resource,
                        utilization,
                    }
                } else {
                    LimitResult::Allowed
                }
            }
            BackpressureStrategy::Aggressive => {
                // Start backpressure earlier and more aggressively
                if utilization > 0.8 {
                    let excess = (utilization - 0.8) / 0.2;
                    let delay_ms = (excess * 200.0) as u64; // Max 200ms delay
                    LimitResult::Delayed {
                        resource,
                        delay: Duration::from_millis(delay_ms.min(200)),
                    }
                } else if utilization > 0.6 {
                    LimitResult::AllowedWithWarning {
                        resource,
                        utilization,
                    }
                } else {
                    LimitResult::Allowed
                }
            }
        }
    }

    /// Combine multiple limit results.
    fn combine_results(
        &self,
        memory: LimitResult,
        connections: LimitResult,
        rate: LimitResult,
    ) -> LimitResult {
        // Priority: Rejected > Delayed > Warning > Allowed
        for result in [&memory, &connections, &rate] {
            if result.is_rejected() {
                return result.clone();
            }
        }

        // Find longest delay
        let mut max_delay = Duration::ZERO;
        let mut delayed_resource = None;

        for result in [&memory, &connections, &rate] {
            if let Some(delay) = result.delay() {
                if delay > max_delay {
                    max_delay = delay;
                    delayed_resource = match result {
                        LimitResult::Delayed { resource, .. } => Some(*resource),
                        _ => None,
                    };
                }
            }
        }

        if let Some(resource) = delayed_resource {
            return LimitResult::Delayed {
                resource,
                delay: max_delay,
            };
        }

        // Check for warnings
        for result in [&memory, &connections, &rate] {
            if let LimitResult::AllowedWithWarning { resource, utilization } = result {
                return LimitResult::AllowedWithWarning {
                    resource: *resource,
                    utilization: *utilization,
                };
            }
        }

        LimitResult::Allowed
    }

    /// Update memory usage.
    pub fn set_memory_used(&self, bytes: u64) {
        self.memory_used.store(bytes, Ordering::Relaxed);
    }

    /// Add to memory usage.
    pub fn add_memory(&self, bytes: u64) {
        self.memory_used.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Subtract from memory usage.
    pub fn release_memory(&self, bytes: u64) {
        self.memory_used.fetch_sub(bytes, Ordering::Relaxed);
    }

    /// Update connection count.
    pub fn set_connections(&self, count: usize) {
        self.connections.store(count, Ordering::Relaxed);
    }

    /// Increment connection count.
    pub fn add_connection(&self) -> bool {
        let current = self.connections.fetch_add(1, Ordering::Relaxed);
        if current >= self.config.max_connections {
            self.connections.fetch_sub(1, Ordering::Relaxed);
            false
        } else {
            true
        }
    }

    /// Decrement connection count.
    pub fn remove_connection(&self) {
        self.connections.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get current resource snapshot.
    pub fn snapshot(&self) -> ResourceSnapshot {
        self.rate_bucket.refill();

        ResourceSnapshot {
            memory_bytes: self.memory_used.load(Ordering::Relaxed),
            memory_limit: self.config.max_memory_bytes,
            connections: self.connections.load(Ordering::Relaxed),
            connection_limit: self.config.max_connections,
            request_rate: self.rate_bucket.current() as f64,
            request_rate_limit: self.config.max_requests_per_second,
            timestamp: Instant::now(),
        }
    }

    /// Get limiter statistics.
    pub fn statistics(&self) -> LimiterStatistics {
        self.stats.read().clone()
    }

    /// Reset statistics.
    pub fn reset_statistics(&self) {
        *self.stats.write() = LimiterStatistics::default();
    }

    /// Get overall utilization (0.0-1.0).
    pub fn overall_utilization(&self) -> f64 {
        self.snapshot().overall_utilization()
    }

    /// Get configuration.
    pub fn config(&self) -> &ResourceLimiterConfig {
        &self.config
    }

    /// Get uptime.
    pub fn uptime(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Check if system is under pressure.
    pub fn is_under_pressure(&self) -> bool {
        self.overall_utilization() >= self.config.backpressure_threshold
    }

    /// Get current backpressure level (0.0-1.0).
    pub fn backpressure_level(&self) -> f64 {
        let utilization = self.overall_utilization();
        let threshold = self.config.backpressure_threshold;

        if utilization < threshold {
            0.0
        } else {
            (utilization - threshold) / (1.0 - threshold)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> ResourceLimiterConfig {
        ResourceLimiterConfig {
            max_memory_bytes: 1024 * 1024, // 1MB
            max_connections: 100,
            max_requests_per_second: 1000,
            backpressure_threshold: 0.8,
            strategy: BackpressureStrategyConfig::Adaptive,
        }
    }

    #[test]
    fn test_allowed_under_limit() {
        let limiter = ResourceLimiter::new(make_config());

        let result = limiter.check();
        assert!(result.is_allowed());
        assert!(!result.is_rejected());
    }

    #[test]
    fn test_memory_limit() {
        let limiter = ResourceLimiter::new(make_config());

        // Set memory near limit
        limiter.set_memory_used(1024 * 1024 + 1);

        let result = limiter.check();
        assert!(result.is_rejected());
        assert!(matches!(result, LimitResult::Rejected { resource: ResourceType::Memory, .. }));
    }

    #[test]
    fn test_connection_limit() {
        let limiter = ResourceLimiter::new(make_config());

        // Set connections at limit
        limiter.set_connections(100);

        let result = limiter.check();
        assert!(result.is_rejected());
        assert!(matches!(result, LimitResult::Rejected { resource: ResourceType::Connections, .. }));
    }

    #[test]
    fn test_add_remove_connection() {
        let limiter = ResourceLimiter::new(make_config());

        // Add connections up to limit
        for i in 0..100 {
            assert!(limiter.add_connection(), "Failed at connection {}", i);
        }

        // Should fail at limit
        assert!(!limiter.add_connection());

        // Remove one
        limiter.remove_connection();

        // Should succeed now
        assert!(limiter.add_connection());
    }

    #[test]
    fn test_memory_tracking() {
        let limiter = ResourceLimiter::new(make_config());

        limiter.add_memory(100);
        assert_eq!(limiter.snapshot().memory_bytes, 100);

        limiter.add_memory(200);
        assert_eq!(limiter.snapshot().memory_bytes, 300);

        limiter.release_memory(100);
        assert_eq!(limiter.snapshot().memory_bytes, 200);
    }

    #[test]
    fn test_backpressure_threshold() {
        let limiter = ResourceLimiter::new(make_config());

        // Below threshold - no pressure
        limiter.set_memory_used(512 * 1024); // 50%
        assert!(!limiter.is_under_pressure());
        assert_eq!(limiter.backpressure_level(), 0.0);

        // Above threshold - under pressure
        limiter.set_memory_used(900 * 1024); // 90%
        assert!(limiter.is_under_pressure());
        assert!(limiter.backpressure_level() > 0.0);
    }

    #[test]
    fn test_warning_near_limit() {
        let limiter = ResourceLimiter::new(make_config());

        // Set utilization above threshold but below limit
        limiter.set_memory_used(850 * 1024); // ~83%

        let result = limiter.check();
        // Should be allowed with warning or delayed
        assert!(!result.is_rejected());
    }

    #[test]
    fn test_statistics() {
        let limiter = ResourceLimiter::new(make_config());

        // Make some checks
        limiter.check();
        limiter.check();
        limiter.set_memory_used(1024 * 1024 + 1);
        limiter.check(); // Should be rejected

        let stats = limiter.statistics();
        assert_eq!(stats.total_checks, 3);
        assert!(stats.allowed >= 2);
        assert!(stats.rejected >= 1);
    }

    #[test]
    fn test_no_backpressure_strategy() {
        let mut config = make_config();
        config.strategy = BackpressureStrategyConfig::None;

        let limiter = ResourceLimiter::new(config);

        // Set high utilization
        limiter.set_memory_used(950 * 1024); // 95%

        let result = limiter.check();
        // With None strategy, should still be allowed until 100%
        assert!(!result.is_rejected());
    }

    #[test]
    fn test_snapshot() {
        let limiter = ResourceLimiter::new(make_config());

        limiter.set_memory_used(512 * 1024);
        limiter.set_connections(50);

        let snapshot = limiter.snapshot();
        assert_eq!(snapshot.memory_bytes, 512 * 1024);
        assert_eq!(snapshot.memory_limit, 1024 * 1024);
        assert_eq!(snapshot.connections, 50);
        assert_eq!(snapshot.connection_limit, 100);
        assert_eq!(snapshot.memory_utilization(), 0.5);
        assert_eq!(snapshot.connection_utilization(), 0.5);
    }

    #[test]
    fn test_limit_result_methods() {
        let allowed = LimitResult::Allowed;
        assert!(allowed.is_allowed());
        assert!(!allowed.is_rejected());
        assert!(!allowed.needs_delay());
        assert!(allowed.delay().is_none());

        let rejected = LimitResult::Rejected {
            resource: ResourceType::Memory,
            current: 100,
            limit: 100,
        };
        assert!(!rejected.is_allowed());
        assert!(rejected.is_rejected());

        let delayed = LimitResult::Delayed {
            resource: ResourceType::RequestRate,
            delay: Duration::from_millis(50),
        };
        assert!(!delayed.is_allowed());
        assert!(delayed.needs_delay());
        assert_eq!(delayed.delay(), Some(Duration::from_millis(50)));
    }
}
