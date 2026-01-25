//! Batch request processor for high-throughput request handling.
//!
//! This module provides infrastructure for processing Modbus requests in batches,
//! enabling higher throughput through:
//!
//! - **Request batching**: Group multiple requests for parallel processing
//! - **Request coalescing**: Combine similar read requests to reduce I/O
//! - **Parallel execution**: Process batches across multiple threads
//! - **Backpressure**: Prevent overload with configurable limits
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │                        BatchProcessor                                 │
//! │  ┌────────────────────────────────────────────────────────────────┐  │
//! │  │                     Request Queue                              │  │
//! │  │  [Req1] [Req2] [Req3] ... [ReqN]  ──────► Batch Formation      │  │
//! │  └────────────────────────────────────────────────────────────────┘  │
//! │                              │                                        │
//! │                    ┌─────────┴─────────┐                             │
//! │                    ▼                   ▼                             │
//! │           ┌──────────────┐     ┌──────────────┐                     │
//! │           │  Coalescer   │     │   Batcher    │                     │
//! │           │ (Read Reqs)  │     │ (All Reqs)   │                     │
//! │           └──────┬───────┘     └──────┬───────┘                     │
//! │                  │                    │                              │
//! │                  └────────┬───────────┘                              │
//! │                           ▼                                          │
//! │  ┌────────────────────────────────────────────────────────────────┐  │
//! │  │                  Parallel Executor                             │  │
//! │  │  ┌─────────┐ ┌─────────┐ ┌─────────┐       ┌─────────┐        │  │
//! │  │  │Worker 1 │ │Worker 2 │ │Worker 3 │  ...  │Worker N │        │  │
//! │  │  └─────────┘ └─────────┘ └─────────┘       └─────────┘        │  │
//! │  └────────────────────────────────────────────────────────────────┘  │
//! │                              │                                        │
//! │                              ▼                                        │
//! │                    Response Distribution                             │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use tokio::sync::{oneshot, Semaphore};
use tracing::{debug, trace};

use super::config::BatchProcessorConfig;

/// Re-export config type
pub use super::config::BatchProcessorConfig as BatchConfig;

/// Unique identifier for a batch request.
pub type RequestId = u64;

/// Processing strategy for batch requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingStrategy {
    /// Process all requests in order (FIFO).
    Sequential,
    /// Process all requests in parallel.
    Parallel,
    /// Process in parallel with priority ordering.
    PriorityParallel,
    /// Adaptive strategy based on request type and load.
    Adaptive,
}

impl Default for ProcessingStrategy {
    fn default() -> Self {
        Self::Adaptive
    }
}

/// Priority level for batch requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RequestPriority {
    /// Low priority (background operations).
    Low = 0,
    /// Normal priority (default).
    Normal = 1,
    /// High priority (user-facing operations).
    High = 2,
    /// Critical priority (system operations).
    Critical = 3,
}

impl Default for RequestPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// A batch request to be processed.
#[derive(Debug)]
pub struct BatchRequest<T> {
    /// Unique request ID.
    pub id: RequestId,

    /// Request payload.
    pub payload: T,

    /// Request priority.
    pub priority: RequestPriority,

    /// Unit ID for the request.
    pub unit_id: u8,

    /// Function code.
    pub function_code: u8,

    /// Start address (for coalescing).
    pub start_address: u16,

    /// Quantity (for coalescing).
    pub quantity: u16,

    /// When the request was submitted.
    submitted_at: Instant,

    /// Response channel.
    response_tx: Option<oneshot::Sender<BatchResponse<T>>>,
}

impl<T> BatchRequest<T> {
    /// Create a new batch request.
    pub fn new(
        payload: T,
        unit_id: u8,
        function_code: u8,
        start_address: u16,
        quantity: u16,
    ) -> (Self, oneshot::Receiver<BatchResponse<T>>) {
        let (tx, rx) = oneshot::channel();
        let request = Self {
            id: 0, // Will be assigned by processor
            payload,
            priority: RequestPriority::Normal,
            unit_id,
            function_code,
            start_address,
            quantity,
            submitted_at: Instant::now(),
            response_tx: Some(tx),
        };
        (request, rx)
    }

    /// Set request priority.
    pub fn with_priority(mut self, priority: RequestPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Get wait time since submission.
    pub fn wait_time(&self) -> Duration {
        self.submitted_at.elapsed()
    }

    /// Check if this request can be coalesced with another.
    pub fn can_coalesce(&self, other: &Self) -> bool {
        // Only read operations (FC 1-4) can be coalesced
        if self.function_code > 4 || other.function_code > 4 {
            return false;
        }

        // Must be same unit ID and function code
        if self.unit_id != other.unit_id || self.function_code != other.function_code {
            return false;
        }

        // Check if address ranges are adjacent or overlapping
        let self_end = self.start_address.saturating_add(self.quantity);
        let other_end = other.start_address.saturating_add(other.quantity);

        // Adjacent: self ends where other starts, or vice versa
        // Overlapping: ranges intersect
        (self_end >= other.start_address && self.start_address <= other_end) ||
        (other_end >= self.start_address && other.start_address <= self_end)
    }
}

/// Response from batch processing.
#[derive(Debug)]
pub struct BatchResponse<T> {
    /// Original request ID.
    pub request_id: RequestId,

    /// Response payload (if successful).
    pub result: Result<T, BatchError>,

    /// Processing latency.
    pub latency: Duration,

    /// Whether this was a coalesced response.
    pub coalesced: bool,
}

/// Errors from batch processing.
#[derive(Debug, Clone, thiserror::Error)]
pub enum BatchError {
    #[error("Request timed out after {0:?}")]
    Timeout(Duration),

    #[error("Processor is shutting down")]
    Shutdown,

    #[error("Queue is full ({current}/{max})")]
    QueueFull { current: usize, max: usize },

    #[error("Processing error: {0}")]
    Processing(String),

    #[error("Request cancelled")]
    Cancelled,
}

/// Statistics for the batch processor.
#[derive(Debug, Clone, Default)]
pub struct BatchStatistics {
    /// Total requests submitted.
    pub total_submitted: u64,
    /// Total requests processed.
    pub total_processed: u64,
    /// Total requests coalesced.
    pub total_coalesced: u64,
    /// Total batches executed.
    pub total_batches: u64,
    /// Average batch size.
    pub avg_batch_size: f64,
    /// Average processing latency (microseconds).
    pub avg_latency_us: f64,
    /// Current queue depth.
    pub queue_depth: usize,
    /// Peak queue depth.
    pub peak_queue_depth: usize,
}

/// Internal batch for processing.
struct ProcessingBatch<T> {
    requests: Vec<BatchRequest<T>>,
    created_at: Instant,
}

impl<T> ProcessingBatch<T> {
    fn new() -> Self {
        Self {
            requests: Vec::new(),
            created_at: Instant::now(),
        }
    }

    fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    fn len(&self) -> usize {
        self.requests.len()
    }

    fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    fn push(&mut self, request: BatchRequest<T>) {
        self.requests.push(request);
    }

    fn should_flush(&self, config: &BatchProcessorConfig) -> bool {
        self.len() >= config.batch_size || self.age() >= config.batch_timeout
    }
}

/// Coalescing key for grouping similar requests.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct CoalesceKey {
    unit_id: u8,
    function_code: u8,
}

/// Request handler trait for batch processing.
pub trait BatchHandler<T>: Send + Sync {
    /// Process a single request.
    fn process(&self, request: &BatchRequest<T>) -> Result<T, BatchError>;

    /// Process a batch of requests (can override for optimization).
    fn process_batch(&self, requests: &[BatchRequest<T>]) -> Vec<Result<T, BatchError>> {
        requests.iter().map(|r| self.process(r)).collect()
    }

    /// Check if two requests can be coalesced (override for custom logic).
    fn can_coalesce(&self, a: &BatchRequest<T>, b: &BatchRequest<T>) -> bool {
        a.can_coalesce(b)
    }
}

/// Batch processor for high-throughput request handling.
pub struct BatchProcessor<T>
where
    T: Send + 'static,
{
    /// Configuration.
    config: BatchProcessorConfig,

    /// Processing strategy.
    strategy: ProcessingStrategy,

    /// Next request ID.
    next_id: AtomicU64,

    /// Current pending batch.
    pending_batch: Mutex<ProcessingBatch<T>>,

    /// Pending request count.
    pending_count: AtomicUsize,

    /// Peak queue depth.
    peak_queue_depth: AtomicUsize,

    /// Concurrency limiter.
    concurrency_semaphore: Arc<Semaphore>,

    /// Statistics.
    stats: RwLock<BatchStatisticsInternal>,

    /// Shutdown flag.
    shutdown: std::sync::atomic::AtomicBool,
}

/// Internal mutable statistics.
#[derive(Default)]
struct BatchStatisticsInternal {
    total_submitted: u64,
    total_processed: u64,
    total_coalesced: u64,
    total_batches: u64,
    total_latency_us: u64,
}

impl<T> BatchProcessor<T>
where
    T: Send + Clone + 'static,
{
    /// Create a new batch processor.
    pub fn new(config: BatchProcessorConfig) -> Self {
        let max_concurrent = config.batch_size * 2; // Allow 2x batch size in flight

        Self {
            config,
            strategy: ProcessingStrategy::Adaptive,
            next_id: AtomicU64::new(1),
            pending_batch: Mutex::new(ProcessingBatch::new()),
            pending_count: AtomicUsize::new(0),
            peak_queue_depth: AtomicUsize::new(0),
            concurrency_semaphore: Arc::new(Semaphore::new(max_concurrent)),
            stats: RwLock::new(BatchStatisticsInternal::default()),
            shutdown: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Create with specific processing strategy.
    pub fn with_strategy(mut self, strategy: ProcessingStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Submit a request for batch processing.
    ///
    /// The request must already have a response channel set (from `BatchRequest::new`).
    /// Returns an error if the queue is full or the processor is shutting down.
    pub fn submit(
        &self,
        mut request: BatchRequest<T>,
    ) -> Result<(), BatchError> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(BatchError::Shutdown);
        }

        let current = self.pending_count.load(Ordering::Relaxed);
        if current >= self.config.max_pending {
            return Err(BatchError::QueueFull {
                current,
                max: self.config.max_pending,
            });
        }

        // Assign request ID
        request.id = self.next_id.fetch_add(1, Ordering::Relaxed);

        // Capture request ID before moving
        let request_id = request.id;

        // Add to pending batch
        {
            let mut batch = self.pending_batch.lock();
            batch.push(request);
            let count = batch.len();
            self.pending_count.store(count, Ordering::Relaxed);

            // Update peak
            let peak = self.peak_queue_depth.load(Ordering::Relaxed);
            if count > peak {
                self.peak_queue_depth.store(count, Ordering::Relaxed);
            }
        }

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.total_submitted += 1;
        }

        trace!(
            request_id,
            pending = self.pending_count.load(Ordering::Relaxed),
            "Request submitted"
        );

        Ok(())
    }

    /// Process pending batch immediately.
    pub async fn flush<H: BatchHandler<T>>(&self, handler: &H) -> usize {
        let batch = {
            let mut pending = self.pending_batch.lock();
            std::mem::replace(&mut *pending, ProcessingBatch::new())
        };

        if batch.is_empty() {
            return 0;
        }

        let batch_size = batch.len();
        self.pending_count.store(0, Ordering::Relaxed);

        debug!(
            batch_size,
            age_ms = batch.age().as_millis(),
            "Flushing batch"
        );

        // Process based on strategy
        let processed = match self.strategy {
            ProcessingStrategy::Sequential => {
                self.process_sequential(batch, handler).await
            }
            ProcessingStrategy::Parallel | ProcessingStrategy::PriorityParallel => {
                self.process_parallel(batch, handler).await
            }
            ProcessingStrategy::Adaptive => {
                if batch_size > 10 {
                    self.process_parallel(batch, handler).await
                } else {
                    self.process_sequential(batch, handler).await
                }
            }
        };

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.total_batches += 1;
        }

        processed
    }

    /// Process batch sequentially.
    async fn process_sequential<H: BatchHandler<T>>(
        &self,
        batch: ProcessingBatch<T>,
        handler: &H,
    ) -> usize {
        let mut processed = 0;

        for request in batch.requests {
            let start = Instant::now();
            let result = handler.process(&request);
            let latency = start.elapsed();

            self.send_response(request, result, latency, false);
            processed += 1;

            // Update stats
            {
                let mut stats = self.stats.write();
                stats.total_processed += 1;
                stats.total_latency_us += latency.as_micros() as u64;
            }
        }

        processed
    }

    /// Process batch in parallel.
    async fn process_parallel<H: BatchHandler<T>>(
        &self,
        batch: ProcessingBatch<T>,
        handler: &H,
    ) -> usize {
        // Group by coalesce key if coalescing is enabled
        let requests = if self.config.coalescing.enabled {
            self.coalesce_requests(batch.requests, handler)
        } else {
            batch.requests
        };

        let results = handler.process_batch(&requests);
        let mut processed = 0;

        for (request, result) in requests.into_iter().zip(results.into_iter()) {
            let latency = request.wait_time();
            self.send_response(request, result, latency, false);
            processed += 1;

            // Update stats
            {
                let mut stats = self.stats.write();
                stats.total_processed += 1;
                stats.total_latency_us += latency.as_micros() as u64;
            }
        }

        processed
    }

    /// Coalesce similar requests.
    fn coalesce_requests<H: BatchHandler<T>>(
        &self,
        mut requests: Vec<BatchRequest<T>>,
        handler: &H,
    ) -> Vec<BatchRequest<T>> {
        if requests.len() <= 1 {
            return requests;
        }

        // Sort by coalesce key and address
        requests.sort_by(|a, b| {
            (a.unit_id, a.function_code, a.start_address)
                .cmp(&(b.unit_id, b.function_code, b.start_address))
        });

        let mut coalesced = Vec::with_capacity(requests.len());
        let mut i = 0;

        while i < requests.len() {
            let mut current = requests.swap_remove(i);
            let mut merged_count = 0;

            // Try to coalesce with subsequent requests
            let mut j = i;
            while j < requests.len() && merged_count < self.config.coalescing.max_coalesce {
                if handler.can_coalesce(&current, &requests[j]) {
                    // Merge address range
                    let other = requests.swap_remove(j);
                    let new_start = current.start_address.min(other.start_address);
                    let current_end = current.start_address + current.quantity;
                    let other_end = other.start_address + other.quantity;
                    let new_end = current_end.max(other_end);

                    current.start_address = new_start;
                    current.quantity = new_end - new_start;
                    merged_count += 1;

                    // Mark as coalesced in stats
                    {
                        let mut stats = self.stats.write();
                        stats.total_coalesced += 1;
                    }
                } else {
                    j += 1;
                }
            }

            coalesced.push(current);
            if i < requests.len() {
                i += 1;
            } else {
                break;
            }
        }

        coalesced
    }

    /// Send response to waiting caller.
    fn send_response(
        &self,
        mut request: BatchRequest<T>,
        result: Result<T, BatchError>,
        latency: Duration,
        coalesced: bool,
    ) {
        if let Some(tx) = request.response_tx.take() {
            let response = BatchResponse {
                request_id: request.id,
                result,
                latency,
                coalesced,
            };
            let _ = tx.send(response);
        }
    }

    /// Check if batch should be flushed based on config.
    pub fn should_flush(&self) -> bool {
        let batch = self.pending_batch.lock();
        batch.should_flush(&self.config)
    }

    /// Get current queue depth.
    pub fn queue_depth(&self) -> usize {
        self.pending_count.load(Ordering::Relaxed)
    }

    /// Get batch statistics.
    pub fn statistics(&self) -> BatchStatistics {
        let stats = self.stats.read();
        let queue_depth = self.pending_count.load(Ordering::Relaxed);
        let peak_queue_depth = self.peak_queue_depth.load(Ordering::Relaxed);

        let avg_batch_size = if stats.total_batches > 0 {
            stats.total_processed as f64 / stats.total_batches as f64
        } else {
            0.0
        };

        let avg_latency_us = if stats.total_processed > 0 {
            stats.total_latency_us as f64 / stats.total_processed as f64
        } else {
            0.0
        };

        BatchStatistics {
            total_submitted: stats.total_submitted,
            total_processed: stats.total_processed,
            total_coalesced: stats.total_coalesced,
            total_batches: stats.total_batches,
            avg_batch_size,
            avg_latency_us,
            queue_depth,
            peak_queue_depth,
        }
    }

    /// Check if processor is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Shutdown the processor.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);

        // Drain pending requests with error
        let batch = {
            let mut pending = self.pending_batch.lock();
            std::mem::replace(&mut *pending, ProcessingBatch::new())
        };

        for mut request in batch.requests {
            if let Some(tx) = request.response_tx.take() {
                let response = BatchResponse {
                    request_id: request.id,
                    result: Err(BatchError::Shutdown),
                    latency: request.wait_time(),
                    coalesced: false,
                };
                let _ = tx.send(response);
            }
        }
    }

    /// Get configuration.
    pub fn config(&self) -> &BatchProcessorConfig {
        &self.config
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use super::super::config::CoalescingConfig;

    /// Simple test handler that echoes the payload.
    struct EchoHandler;

    impl BatchHandler<Vec<u8>> for EchoHandler {
        fn process(&self, request: &BatchRequest<Vec<u8>>) -> Result<Vec<u8>, BatchError> {
            Ok(request.payload.clone())
        }
    }

    fn make_config(batch_size: usize, max_pending: usize) -> BatchProcessorConfig {
        BatchProcessorConfig {
            enabled: true,
            batch_size,
            batch_timeout: Duration::from_millis(10),
            max_pending,
            coalescing: CoalescingConfig::disabled(),
        }
    }

    #[test]
    fn test_request_creation() {
        let payload = vec![0x03u8, 0x00, 0x00, 0x00, 0x0A];
        let (request, _rx) = BatchRequest::<Vec<u8>>::new(payload.clone(), 1, 0x03, 0, 10);

        assert_eq!(request.unit_id, 1);
        assert_eq!(request.function_code, 0x03);
        assert_eq!(request.start_address, 0);
        assert_eq!(request.quantity, 10);
        assert_eq!(request.payload, payload);
    }

    #[test]
    fn test_can_coalesce() {
        let (req1, _) = BatchRequest::<Vec<u8>>::new(vec![], 1, 0x03, 0, 10);
        let (req2, _) = BatchRequest::<Vec<u8>>::new(vec![], 1, 0x03, 10, 10);
        let (req3, _) = BatchRequest::<Vec<u8>>::new(vec![], 1, 0x03, 100, 10);
        let (req4, _) = BatchRequest::<Vec<u8>>::new(vec![], 2, 0x03, 0, 10);
        let (req5, _) = BatchRequest::<Vec<u8>>::new(vec![], 1, 0x06, 0, 10);

        // Adjacent ranges - can coalesce
        assert!(req1.can_coalesce(&req2));

        // Non-adjacent ranges - cannot coalesce
        assert!(!req1.can_coalesce(&req3));

        // Different unit IDs - cannot coalesce
        assert!(!req1.can_coalesce(&req4));

        // Different function codes - cannot coalesce
        assert!(!req1.can_coalesce(&req5));

        // Write operations cannot be coalesced
        assert!(!req5.can_coalesce(&req1));
    }

    #[tokio::test]
    async fn test_submit_and_flush() {
        let config = make_config(10, 100);
        let processor = BatchProcessor::<Vec<u8>>::new(config);
        let handler = EchoHandler;

        // Submit a request
        let payload = vec![0x01, 0x02, 0x03];
        let (request, rx) = BatchRequest::new(payload.clone(), 1, 0x03, 0, 10);
        processor.submit(request).unwrap();

        assert_eq!(processor.queue_depth(), 1);

        // Flush
        let processed = processor.flush(&handler).await;
        assert_eq!(processed, 1);
        assert_eq!(processor.queue_depth(), 0);

        // Check response
        let response = rx.await.unwrap();
        assert!(response.result.is_ok());
        assert_eq!(response.result.unwrap(), payload);
    }

    #[tokio::test]
    async fn test_queue_full_rejection() {
        let config = make_config(10, 2);
        let processor = BatchProcessor::<Vec<u8>>::new(config);

        // Submit up to limit
        let (req1, _) = BatchRequest::new(vec![], 1, 0x03, 0, 10);
        let (req2, _) = BatchRequest::new(vec![], 1, 0x03, 0, 10);
        assert!(processor.submit(req1).is_ok());
        assert!(processor.submit(req2).is_ok());

        // Should be rejected
        let (req3, _) = BatchRequest::new(vec![], 1, 0x03, 0, 10);
        let result = processor.submit(req3);
        assert!(matches!(result, Err(BatchError::QueueFull { .. })));
    }

    #[tokio::test]
    async fn test_statistics() {
        let config = make_config(10, 100);
        let processor = BatchProcessor::<Vec<u8>>::new(config);
        let handler = EchoHandler;

        // Submit and process requests
        for i in 0..5 {
            let (request, _) = BatchRequest::new(vec![i], 1, 0x03, i as u16, 1);
            processor.submit(request).unwrap();
        }

        processor.flush(&handler).await;

        let stats = processor.statistics();
        assert_eq!(stats.total_submitted, 5);
        assert_eq!(stats.total_processed, 5);
        assert_eq!(stats.total_batches, 1);
        assert_eq!(stats.avg_batch_size, 5.0);
    }

    #[tokio::test]
    async fn test_shutdown() {
        let config = make_config(10, 100);
        let processor = BatchProcessor::<Vec<u8>>::new(config);

        // Submit a request
        let (request, rx) = BatchRequest::new(vec![], 1, 0x03, 0, 10);
        processor.submit(request).unwrap();

        // Shutdown
        processor.shutdown();

        // Check pending request got error
        let response = rx.await.unwrap();
        assert!(matches!(response.result, Err(BatchError::Shutdown)));

        // New submissions should fail
        let (request, _) = BatchRequest::new(vec![], 1, 0x03, 0, 10);
        assert!(matches!(processor.submit(request), Err(BatchError::Shutdown)));
    }

    #[test]
    fn test_priority() {
        let (request, _) = BatchRequest::<Vec<u8>>::new(vec![], 1, 0x03, 0, 10);
        assert_eq!(request.priority, RequestPriority::Normal);

        let request: BatchRequest<Vec<u8>> = request.with_priority(RequestPriority::High);
        assert_eq!(request.priority, RequestPriority::High);

        // Priority ordering
        assert!(RequestPriority::Critical > RequestPriority::High);
        assert!(RequestPriority::High > RequestPriority::Normal);
        assert!(RequestPriority::Normal > RequestPriority::Low);
    }
}
