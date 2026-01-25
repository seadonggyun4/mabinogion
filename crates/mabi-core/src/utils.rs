//! Common utilities and helper functions.
//!
//! This module provides shared utilities used across the simulator including:
//! - ID generation
//! - Time utilities
//! - String helpers
//! - Builder pattern macros

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use uuid::Uuid;

// =============================================================================
// ID Generation
// =============================================================================

/// Counter for sequential IDs.
static SEQUENTIAL_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a new UUID v4.
#[inline]
pub fn generate_uuid() -> String {
    Uuid::new_v4().to_string()
}

/// Generate a short UUID (first 8 characters).
#[inline]
pub fn generate_short_uuid() -> String {
    Uuid::new_v4().to_string()[..8].to_string()
}

/// Generate a sequential ID with prefix.
#[inline]
pub fn generate_sequential_id(prefix: &str) -> String {
    let seq = SEQUENTIAL_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("{}-{:08}", prefix, seq)
}

/// Generate a timestamp-based ID.
#[inline]
pub fn generate_timestamp_id(prefix: &str) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    format!("{}-{}", prefix, ts)
}

// =============================================================================
// Time Utilities
// =============================================================================

/// Get current timestamp in milliseconds since UNIX epoch.
#[inline]
pub fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Get current timestamp in microseconds since UNIX epoch.
#[inline]
pub fn current_timestamp_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

/// Get current timestamp in nanoseconds since UNIX epoch.
#[inline]
pub fn current_timestamp_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

/// Format duration as human-readable string.
pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();

    if secs >= 3600 {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        format!("{}h {}m", hours, mins)
    } else if secs >= 60 {
        let mins = secs / 60;
        let secs = secs % 60;
        format!("{}m {}s", mins, secs)
    } else if secs > 0 {
        format!("{}.{:03}s", secs, millis)
    } else if millis > 0 {
        format!("{}ms", millis)
    } else {
        format!("{}µs", duration.as_micros())
    }
}

/// Simple stopwatch for measuring elapsed time.
#[derive(Debug, Clone)]
pub struct Stopwatch {
    start: Instant,
    laps: Vec<(String, Duration)>,
}

impl Stopwatch {
    /// Start a new stopwatch.
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
            laps: Vec::new(),
        }
    }

    /// Get elapsed time since start.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Record a lap.
    pub fn lap(&mut self, name: impl Into<String>) {
        self.laps.push((name.into(), self.elapsed()));
    }

    /// Get all laps.
    pub fn laps(&self) -> &[(String, Duration)] {
        &self.laps
    }

    /// Reset the stopwatch.
    pub fn reset(&mut self) {
        self.start = Instant::now();
        self.laps.clear();
    }

    /// Get elapsed time in milliseconds.
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed().as_millis() as u64
    }

    /// Get elapsed time in microseconds.
    pub fn elapsed_us(&self) -> u64 {
        self.elapsed().as_micros() as u64
    }
}

impl Default for Stopwatch {
    fn default() -> Self {
        Self::start()
    }
}

// =============================================================================
// String Utilities
// =============================================================================

/// Truncate a string to a maximum length.
pub fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Convert bytes to human-readable format.
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Sanitize a string for use as an identifier.
pub fn sanitize_identifier(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

// =============================================================================
// Builder Pattern Macros
// =============================================================================

/// Implement a builder pattern setter method.
///
/// # Example
///
/// ```rust,ignore
/// struct Config {
///     name: String,
///     port: u16,
/// }
///
/// impl Config {
///     mabi_core::builder_setter!(name, String);
///     mabi_core::builder_setter!(port, u16);
/// }
/// ```
#[macro_export]
macro_rules! builder_setter {
    ($field:ident, $type:ty) => {
        pub fn $field(mut self, value: $type) -> Self {
            self.$field = value;
            self
        }
    };
    ($field:ident, $type:ty, $doc:expr) => {
        #[doc = $doc]
        pub fn $field(mut self, value: $type) -> Self {
            self.$field = value;
            self
        }
    };
}

/// Implement a builder pattern setter method that takes impl Into<T>.
///
/// # Example
///
/// ```rust,ignore
/// impl Config {
///     mabi_core::builder_setter_into!(name, String);
/// }
///
/// let config = Config::default().name("test");
/// ```
#[macro_export]
macro_rules! builder_setter_into {
    ($field:ident, $type:ty) => {
        pub fn $field(mut self, value: impl Into<$type>) -> Self {
            self.$field = value.into();
            self
        }
    };
    ($field:ident, $type:ty, $doc:expr) => {
        #[doc = $doc]
        pub fn $field(mut self, value: impl Into<$type>) -> Self {
            self.$field = value.into();
            self
        }
    };
}

/// Implement a builder pattern setter method for Option<T>.
///
/// # Example
///
/// ```rust,ignore
/// impl Config {
///     mabi_core::builder_setter_option!(description, String);
/// }
/// ```
#[macro_export]
macro_rules! builder_setter_option {
    ($field:ident, $type:ty) => {
        pub fn $field(mut self, value: impl Into<$type>) -> Self {
            self.$field = Some(value.into());
            self
        }
    };
    ($field:ident, $type:ty, $doc:expr) => {
        #[doc = $doc]
        pub fn $field(mut self, value: impl Into<$type>) -> Self {
            self.$field = Some(value.into());
            self
        }
    };
}

/// Implement a builder pattern with_* prefix setter method.
///
/// # Example
///
/// ```rust,ignore
/// impl Config {
///     mabi_core::builder_with!(name, String);  // Creates with_name()
/// }
/// ```
#[macro_export]
macro_rules! builder_with {
    ($field:ident, $type:ty) => {
        paste::paste! {
            pub fn [<with_ $field>](mut self, value: impl Into<$type>) -> Self {
                self.$field = value.into();
                self
            }
        }
    };
    ($field:ident, $type:ty, $doc:expr) => {
        paste::paste! {
            #[doc = $doc]
            pub fn [<with_ $field>](mut self, value: impl Into<$type>) -> Self {
                self.$field = value.into();
                self
            }
        }
    };
}

// =============================================================================
// Retry Utilities
// =============================================================================

/// Retry configuration.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of attempts.
    pub max_attempts: u32,
    /// Initial delay between retries.
    pub initial_delay: Duration,
    /// Maximum delay between retries.
    pub max_delay: Duration,
    /// Exponential backoff multiplier.
    pub multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            multiplier: 2.0,
        }
    }
}

impl RetryConfig {
    /// Create a new retry config.
    pub fn new(max_attempts: u32) -> Self {
        Self {
            max_attempts,
            ..Default::default()
        }
    }

    /// Set initial delay.
    pub fn with_initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay;
        self
    }

    /// Set max delay.
    pub fn with_max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    /// Set multiplier.
    pub fn with_multiplier(mut self, multiplier: f64) -> Self {
        self.multiplier = multiplier;
        self
    }

    /// Calculate delay for attempt number.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }

        let delay_ms = self.initial_delay.as_millis() as f64
            * self.multiplier.powi(attempt.saturating_sub(1) as i32);
        let delay = Duration::from_millis(delay_ms.min(self.max_delay.as_millis() as f64) as u64);

        delay.min(self.max_delay)
    }
}

/// Retry an async operation with exponential backoff.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_core::utils::{retry_async, RetryConfig};
///
/// let result = retry_async(
///     RetryConfig::new(3),
///     || async {
///         // Your fallible async operation
///         Ok::<_, std::io::Error>(42)
///     },
/// ).await;
/// ```
pub async fn retry_async<F, Fut, T, E>(config: RetryConfig, mut f: F) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let mut last_error = None;

    for attempt in 0..config.max_attempts {
        match f().await {
            Ok(value) => return Ok(value),
            Err(e) => {
                last_error = Some(e);

                if attempt + 1 < config.max_attempts {
                    let delay = config.delay_for_attempt(attempt + 1);
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    Err(last_error.expect("Should have at least one error"))
}

// =============================================================================
// Rate Limiting
// =============================================================================

/// Simple rate limiter using token bucket algorithm.
#[derive(Debug)]
pub struct RateLimiter {
    /// Maximum tokens in the bucket.
    capacity: u64,
    /// Current tokens in the bucket.
    tokens: AtomicU64,
    /// Tokens added per second.
    refill_rate: f64,
    /// Last refill time.
    last_refill: parking_lot::Mutex<Instant>,
}

impl RateLimiter {
    /// Create a new rate limiter.
    pub fn new(capacity: u64, refill_rate: f64) -> Self {
        Self {
            capacity,
            tokens: AtomicU64::new(capacity),
            refill_rate,
            last_refill: parking_lot::Mutex::new(Instant::now()),
        }
    }

    /// Try to acquire a token. Returns true if successful.
    pub fn try_acquire(&self) -> bool {
        self.refill();

        loop {
            let current = self.tokens.load(Ordering::Acquire);
            if current == 0 {
                return false;
            }

            if self
                .tokens
                .compare_exchange_weak(current, current - 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return true;
            }
        }
    }

    /// Try to acquire multiple tokens. Returns true if successful.
    pub fn try_acquire_n(&self, n: u64) -> bool {
        self.refill();

        loop {
            let current = self.tokens.load(Ordering::Acquire);
            if current < n {
                return false;
            }

            if self
                .tokens
                .compare_exchange_weak(current, current - n, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return true;
            }
        }
    }

    /// Get current token count.
    pub fn available(&self) -> u64 {
        self.refill();
        self.tokens.load(Ordering::Acquire)
    }

    fn refill(&self) {
        let mut last_refill = self.last_refill.lock();
        let now = Instant::now();
        let elapsed = now.duration_since(*last_refill).as_secs_f64();

        if elapsed > 0.0 {
            let new_tokens = (elapsed * self.refill_rate) as u64;
            if new_tokens > 0 {
                let current = self.tokens.load(Ordering::Acquire);
                let new_value = (current + new_tokens).min(self.capacity);
                self.tokens.store(new_value, Ordering::Release);
                *last_refill = now;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_uuid() {
        let uuid1 = generate_uuid();
        let uuid2 = generate_uuid();
        assert_ne!(uuid1, uuid2);
        assert_eq!(uuid1.len(), 36);
    }

    #[test]
    fn test_generate_sequential_id() {
        let id1 = generate_sequential_id("device");
        let id2 = generate_sequential_id("device");
        assert!(id1.starts_with("device-"));
        assert!(id2.starts_with("device-"));
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(3661)), "1h 1m");
        assert_eq!(format_duration(Duration::from_secs(61)), "1m 1s");
        assert_eq!(format_duration(Duration::from_millis(1500)), "1.500s");
        assert_eq!(format_duration(Duration::from_millis(500)), "500ms");
        assert_eq!(format_duration(Duration::from_micros(500)), "500µs");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048576), "1.00 MB");
        assert_eq!(format_bytes(1073741824), "1.00 GB");
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("hello", 10), "hello");
        assert_eq!(truncate_string("hello world", 8), "hello...");
    }

    #[test]
    fn test_sanitize_identifier() {
        assert_eq!(sanitize_identifier("hello world!"), "hello_world_");
        assert_eq!(sanitize_identifier("device-001"), "device-001");
    }

    #[test]
    fn test_stopwatch() {
        let mut sw = Stopwatch::start();
        std::thread::sleep(Duration::from_millis(10));
        sw.lap("step1");
        assert!(sw.elapsed().as_millis() >= 10);
        assert_eq!(sw.laps().len(), 1);
    }

    #[test]
    fn test_retry_config() {
        let config = RetryConfig::new(3);
        assert_eq!(config.delay_for_attempt(0), Duration::ZERO);
        assert_eq!(config.delay_for_attempt(1), Duration::from_millis(100));
        assert_eq!(config.delay_for_attempt(2), Duration::from_millis(200));
    }

    #[test]
    fn test_rate_limiter() {
        let limiter = RateLimiter::new(10, 100.0);
        assert!(limiter.try_acquire());
        assert_eq!(limiter.available(), 9);

        // Exhaust tokens
        for _ in 0..9 {
            assert!(limiter.try_acquire());
        }
        assert!(!limiter.try_acquire());
    }
}
