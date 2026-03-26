//! Transaction State Machine (TSM) — server side.
//!
//! Per ASHRAE 135 Clause 5.4, the server maintains one TSM entry per
//! in-flight confirmed transaction. The server TSM:
//!
//! - Detects duplicate requests (same invoke-id from the same source
//!   within the timeout window) and returns the cached response.
//! - Tracks active transactions for timeout/cleanup.
//! - Supports intentional response delay and drop probability for
//!   chaos-testing the client's retry logic.
//!
//! The TSM is keyed by `(SocketAddr, invoke_id)` so that transactions
//! from different clients with the same invoke_id are independent.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;

/// Key that uniquely identifies a server-side transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransactionKey {
    /// Source address of the requesting client.
    pub source: SocketAddr,
    /// Invoke ID from the confirmed request.
    pub invoke_id: u8,
}

impl TransactionKey {
    pub fn new(source: SocketAddr, invoke_id: u8) -> Self {
        Self { source, invoke_id }
    }
}

/// State of a server-side transaction.
#[derive(Debug, Clone)]
pub struct TransactionEntry {
    /// When this transaction was created.
    pub created_at: Instant,
    /// The service choice of the original request.
    pub service_choice: u8,
    /// Cached response APDU (set when the handler completes).
    /// If the same request arrives again before the entry expires,
    /// this cached response is returned without re-executing the handler.
    pub cached_response: Option<Vec<u8>>,
    /// Number of times a duplicate request was received.
    pub duplicate_count: u32,
    /// Current state of the transaction.
    pub state: TransactionState,
}

/// Transaction states per ASHRAE 135 Clause 5.4.5 (simplified).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionState {
    /// Request received, processing by handler.
    AwaitingResponse,
    /// Response generated, waiting to be sent (may be delayed for chaos).
    ResponseReady,
    /// Response has been sent; entry kept for duplicate detection.
    Completed,
}

/// Configuration for the server TSM.
#[derive(Debug, Clone)]
pub struct TsmConfig {
    /// How long completed entries are kept for duplicate detection.
    /// After this period, a retransmitted request is treated as new.
    pub duplicate_window: Duration,
    /// Maximum number of concurrent transactions.
    /// Exceeding this returns Abort(OutOfResources).
    pub max_transactions: usize,
    /// Intentional response delay for chaos testing (0 = no delay).
    pub chaos_delay: Duration,
    /// Probability (0.0–1.0) of intentionally dropping a response.
    /// 0.0 = never drop, 1.0 = always drop.
    pub chaos_drop_probability: f64,
}

impl Default for TsmConfig {
    fn default() -> Self {
        Self {
            duplicate_window: Duration::from_secs(10),
            max_transactions: 256,
            chaos_delay: Duration::ZERO,
            chaos_drop_probability: 0.0,
        }
    }
}

/// Server-side Transaction State Machine.
///
/// Thread-safe via `parking_lot::RwLock`. The TSM is designed for
/// high-throughput simulation workloads, using lock-free atomics for
/// statistics and RwLock for the transaction table.
pub struct ServerTsm {
    config: TsmConfig,
    /// Active and recently-completed transaction entries.
    transactions: RwLock<HashMap<TransactionKey, TransactionEntry>>,
    // --- Statistics (lock-free) ---
    total_transactions: AtomicU64,
    duplicate_count: AtomicU64,
    dropped_count: AtomicU64,
    timeout_count: AtomicU64,
}

impl ServerTsm {
    /// Create a new server TSM with default configuration.
    pub fn new() -> Self {
        Self::with_config(TsmConfig::default())
    }

    /// Create a new server TSM with custom configuration.
    pub fn with_config(config: TsmConfig) -> Self {
        Self {
            transactions: RwLock::new(HashMap::with_capacity(config.max_transactions)),
            config,
            total_transactions: AtomicU64::new(0),
            duplicate_count: AtomicU64::new(0),
            dropped_count: AtomicU64::new(0),
            timeout_count: AtomicU64::new(0),
        }
    }

    /// Begin a new transaction.
    ///
    /// Returns:
    /// - `Ok(None)` if this is a new transaction (proceed with handler)
    /// - `Ok(Some(cached_response))` if this is a duplicate with a cached response
    /// - `Err(TsmError::DuplicateInProgress)` if a duplicate is still being processed
    /// - `Err(TsmError::AtCapacity)` if the TSM is full
    pub fn begin_transaction(
        &self,
        key: TransactionKey,
        service_choice: u8,
    ) -> Result<Option<Vec<u8>>, TsmError> {
        let now = Instant::now();

        // Check for existing transaction
        {
            let mut txns = self.transactions.write();

            if let Some(entry) = txns.get_mut(&key) {
                // Check if the entry has expired
                if now.duration_since(entry.created_at) > self.config.duplicate_window {
                    // Expired — remove and treat as new
                    txns.remove(&key);
                } else {
                    // Duplicate detected
                    entry.duplicate_count += 1;
                    self.duplicate_count.fetch_add(1, Ordering::Relaxed);

                    return match entry.state {
                        TransactionState::AwaitingResponse => {
                            // Still processing — tell caller to wait
                            Err(TsmError::DuplicateInProgress)
                        }
                        TransactionState::ResponseReady | TransactionState::Completed => {
                            // Return cached response
                            Ok(entry.cached_response.clone())
                        }
                    };
                }
            }

            // Check capacity
            if txns.len() >= self.config.max_transactions {
                // Try to evict expired entries first
                let expired_keys: Vec<TransactionKey> = txns
                    .iter()
                    .filter(|(_, e)| {
                        now.duration_since(e.created_at) > self.config.duplicate_window
                    })
                    .map(|(k, _)| *k)
                    .collect();

                for k in &expired_keys {
                    txns.remove(k);
                }
                self.timeout_count
                    .fetch_add(expired_keys.len() as u64, Ordering::Relaxed);

                if txns.len() >= self.config.max_transactions {
                    return Err(TsmError::AtCapacity);
                }
            }

            // Insert new entry
            txns.insert(
                key,
                TransactionEntry {
                    created_at: now,
                    service_choice,
                    cached_response: None,
                    duplicate_count: 0,
                    state: TransactionState::AwaitingResponse,
                },
            );
        }

        self.total_transactions.fetch_add(1, Ordering::Relaxed);
        Ok(None)
    }

    /// Complete a transaction by caching the response.
    ///
    /// Returns `true` if the response should actually be sent (not dropped
    /// by chaos configuration). Returns `false` if the response should be
    /// intentionally dropped for testing.
    pub fn complete_transaction(&self, key: &TransactionKey, response: Vec<u8>) -> bool {
        let mut txns = self.transactions.write();

        if let Some(entry) = txns.get_mut(key) {
            entry.cached_response = Some(response);
            entry.state = TransactionState::Completed;
        }

        // Check chaos drop probability
        if self.config.chaos_drop_probability > 0.0 {
            // Simple deterministic "random" based on total transactions count.
            // For production chaos testing, a proper RNG would be used,
            // but for a simulator this provides reproducible behavior.
            let counter = self.total_transactions.load(Ordering::Relaxed);
            let threshold = (self.config.chaos_drop_probability * 1000.0) as u64;
            let pseudo_random = (counter * 7 + 13) % 1000;
            if pseudo_random < threshold {
                self.dropped_count.fetch_add(1, Ordering::Relaxed);
                return false;
            }
        }

        true
    }

    /// Get the chaos delay configured for this TSM.
    ///
    /// The server should `tokio::time::sleep()` this duration before
    /// sending the response, if non-zero.
    pub fn chaos_delay(&self) -> Duration {
        self.config.chaos_delay
    }

    /// Remove expired entries (periodic cleanup).
    ///
    /// Call this periodically (e.g., every second) to prevent unbounded
    /// growth of the transaction table.
    pub fn cleanup_expired(&self) -> usize {
        let now = Instant::now();
        let mut txns = self.transactions.write();
        let before = txns.len();

        txns.retain(|_, entry| {
            now.duration_since(entry.created_at) <= self.config.duplicate_window
        });

        let removed = before - txns.len();
        if removed > 0 {
            self.timeout_count
                .fetch_add(removed as u64, Ordering::Relaxed);
        }
        removed
    }

    /// Get the number of active transactions.
    pub fn active_count(&self) -> usize {
        self.transactions.read().len()
    }

    /// Get TSM statistics.
    pub fn statistics(&self) -> TsmStatistics {
        TsmStatistics {
            total_transactions: self.total_transactions.load(Ordering::Relaxed),
            active_transactions: self.active_count(),
            duplicate_count: self.duplicate_count.load(Ordering::Relaxed),
            dropped_count: self.dropped_count.load(Ordering::Relaxed),
            timeout_count: self.timeout_count.load(Ordering::Relaxed),
        }
    }

    /// Update chaos testing parameters at runtime.
    pub fn set_chaos_delay(&mut self, delay: Duration) {
        self.config.chaos_delay = delay;
    }

    /// Update chaos drop probability at runtime.
    pub fn set_chaos_drop_probability(&mut self, probability: f64) {
        self.config.chaos_drop_probability = probability.clamp(0.0, 1.0);
    }
}

impl Default for ServerTsm {
    fn default() -> Self {
        Self::new()
    }
}

/// TSM statistics.
#[derive(Debug, Clone)]
pub struct TsmStatistics {
    /// Total number of transactions started.
    pub total_transactions: u64,
    /// Current number of active (not yet expired) transactions.
    pub active_transactions: usize,
    /// Number of duplicate requests detected.
    pub duplicate_count: u64,
    /// Number of responses intentionally dropped (chaos testing).
    pub dropped_count: u64,
    /// Number of transactions that expired without being completed.
    pub timeout_count: u64,
}

/// TSM errors.
#[derive(Debug, thiserror::Error)]
pub enum TsmError {
    /// A duplicate request is still being processed.
    #[error("Duplicate request still in progress")]
    DuplicateInProgress,

    /// The TSM has reached maximum capacity.
    #[error("TSM at maximum capacity")]
    AtCapacity,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn localhost(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    #[test]
    fn test_new_transaction() {
        let tsm = ServerTsm::new();
        let key = TransactionKey::new(localhost(47808), 1);

        let result = tsm.begin_transaction(key, 12);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // New transaction, no cache
        assert_eq!(tsm.active_count(), 1);
    }

    #[test]
    fn test_duplicate_detection_in_progress() {
        let tsm = ServerTsm::new();
        let key = TransactionKey::new(localhost(47808), 1);

        // First request
        tsm.begin_transaction(key, 12).unwrap();

        // Duplicate while still processing
        let result = tsm.begin_transaction(key, 12);
        assert!(matches!(result, Err(TsmError::DuplicateInProgress)));
    }

    #[test]
    fn test_duplicate_returns_cached_response() {
        let tsm = ServerTsm::new();
        let key = TransactionKey::new(localhost(47808), 1);

        // Start and complete transaction
        tsm.begin_transaction(key, 12).unwrap();
        let response = vec![0x30, 1, 12, 0xAA, 0xBB];
        tsm.complete_transaction(&key, response.clone());

        // Duplicate should return cached response
        let result = tsm.begin_transaction(key, 12).unwrap();
        assert_eq!(result, Some(response));
    }

    #[test]
    fn test_different_invoke_ids_are_independent() {
        let tsm = ServerTsm::new();
        let addr = localhost(47808);
        let key1 = TransactionKey::new(addr, 1);
        let key2 = TransactionKey::new(addr, 2);

        tsm.begin_transaction(key1, 12).unwrap();
        let result = tsm.begin_transaction(key2, 15);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
        assert_eq!(tsm.active_count(), 2);
    }

    #[test]
    fn test_different_sources_are_independent() {
        let tsm = ServerTsm::new();
        let key1 = TransactionKey::new(localhost(47808), 1);
        let key2 = TransactionKey::new(localhost(47809), 1); // Same invoke_id, different source

        tsm.begin_transaction(key1, 12).unwrap();
        let result = tsm.begin_transaction(key2, 12);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
        assert_eq!(tsm.active_count(), 2);
    }

    #[test]
    fn test_capacity_limit() {
        let config = TsmConfig {
            max_transactions: 2,
            ..Default::default()
        };
        let tsm = ServerTsm::with_config(config);

        let key1 = TransactionKey::new(localhost(47808), 1);
        let key2 = TransactionKey::new(localhost(47808), 2);
        let key3 = TransactionKey::new(localhost(47808), 3);

        tsm.begin_transaction(key1, 12).unwrap();
        tsm.begin_transaction(key2, 12).unwrap();

        // Third should fail
        let result = tsm.begin_transaction(key3, 12);
        assert!(matches!(result, Err(TsmError::AtCapacity)));
    }

    #[test]
    fn test_cleanup_expired() {
        let config = TsmConfig {
            duplicate_window: Duration::from_millis(10),
            ..Default::default()
        };
        let tsm = ServerTsm::with_config(config);

        let key = TransactionKey::new(localhost(47808), 1);
        tsm.begin_transaction(key, 12).unwrap();
        tsm.complete_transaction(&key, vec![0x20, 1, 12]);

        assert_eq!(tsm.active_count(), 1);

        // Wait for expiry
        std::thread::sleep(Duration::from_millis(15));

        let removed = tsm.cleanup_expired();
        assert_eq!(removed, 1);
        assert_eq!(tsm.active_count(), 0);
    }

    #[test]
    fn test_expired_entry_treated_as_new() {
        let config = TsmConfig {
            duplicate_window: Duration::from_millis(10),
            ..Default::default()
        };
        let tsm = ServerTsm::with_config(config);

        let key = TransactionKey::new(localhost(47808), 1);
        tsm.begin_transaction(key, 12).unwrap();
        tsm.complete_transaction(&key, vec![0x20, 1, 12]);

        // Wait for expiry
        std::thread::sleep(Duration::from_millis(15));

        // Same key should be treated as new
        let result = tsm.begin_transaction(key, 12);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_statistics() {
        let tsm = ServerTsm::new();
        let key = TransactionKey::new(localhost(47808), 1);

        tsm.begin_transaction(key, 12).unwrap();
        tsm.complete_transaction(&key, vec![0x20, 1, 12]);

        // Duplicate
        let _ = tsm.begin_transaction(key, 12);

        let stats = tsm.statistics();
        assert_eq!(stats.total_transactions, 1);
        assert_eq!(stats.active_transactions, 1);
        assert_eq!(stats.duplicate_count, 1);
    }

    #[test]
    fn test_chaos_drop() {
        let config = TsmConfig {
            chaos_drop_probability: 1.0, // Always drop
            ..Default::default()
        };
        let tsm = ServerTsm::with_config(config);
        let key = TransactionKey::new(localhost(47808), 1);

        tsm.begin_transaction(key, 12).unwrap();
        let should_send = tsm.complete_transaction(&key, vec![0x20, 1, 12]);
        assert!(!should_send); // Should be dropped

        let stats = tsm.statistics();
        assert_eq!(stats.dropped_count, 1);
    }

    #[test]
    fn test_no_chaos_drop() {
        let config = TsmConfig {
            chaos_drop_probability: 0.0, // Never drop
            ..Default::default()
        };
        let tsm = ServerTsm::with_config(config);
        let key = TransactionKey::new(localhost(47808), 1);

        tsm.begin_transaction(key, 12).unwrap();
        let should_send = tsm.complete_transaction(&key, vec![0x20, 1, 12]);
        assert!(should_send);
    }

    #[test]
    fn test_chaos_delay() {
        let config = TsmConfig {
            chaos_delay: Duration::from_millis(500),
            ..Default::default()
        };
        let tsm = ServerTsm::with_config(config);
        assert_eq!(tsm.chaos_delay(), Duration::from_millis(500));
    }

    #[test]
    fn test_capacity_evicts_expired() {
        let config = TsmConfig {
            max_transactions: 2,
            duplicate_window: Duration::from_millis(10),
            ..Default::default()
        };
        let tsm = ServerTsm::with_config(config);

        let key1 = TransactionKey::new(localhost(47808), 1);
        let key2 = TransactionKey::new(localhost(47808), 2);

        tsm.begin_transaction(key1, 12).unwrap();
        tsm.begin_transaction(key2, 12).unwrap();

        // Wait for expiry
        std::thread::sleep(Duration::from_millis(15));

        // Now a new transaction should succeed because expired entries are evicted
        let key3 = TransactionKey::new(localhost(47808), 3);
        let result = tsm.begin_transaction(key3, 12);
        assert!(result.is_ok());
    }

    #[test]
    fn test_transaction_states() {
        let tsm = ServerTsm::new();
        let key = TransactionKey::new(localhost(47808), 1);

        tsm.begin_transaction(key, 12).unwrap();

        // Check state is AwaitingResponse
        {
            let txns = tsm.transactions.read();
            let entry = txns.get(&key).unwrap();
            assert_eq!(entry.state, TransactionState::AwaitingResponse);
        }

        // Complete
        tsm.complete_transaction(&key, vec![0x20, 1, 12]);

        // Check state is Completed
        {
            let txns = tsm.transactions.read();
            let entry = txns.get(&key).unwrap();
            assert_eq!(entry.state, TransactionState::Completed);
            assert!(entry.cached_response.is_some());
        }
    }
}
