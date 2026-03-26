//! Server-side ACK waiter for reliable frame delivery to clients.
//!
//! When the server sends a TUNNELLING_REQUEST to the client (e.g., L_Data.ind,
//! L_Data.con, GroupValueResponse), it must wait for a TUNNELLING_ACK.
//! This module tracks pending ACKs, handles timeouts, and manages retries.
//!
//! Mirrors trap-knx's AckWaiter with knxd semantics:
//! - Default ACK timeout: 1 second
//! - Default max retries: 3
//! - Consecutive send errors > threshold → mark connection degraded

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Result of waiting for an ACK.
#[derive(Debug, Clone)]
pub enum AckResult {
    /// ACK received successfully.
    Success { latency: Duration, attempt: u8 },
    /// ACK received with error status.
    AckError { status: u8, attempt: u8 },
    /// Socket send failed.
    SendFailed { reason: String, attempt: u8 },
    /// All retries exhausted without ACK.
    MaxRetriesExceeded {
        attempts: u8,
        consecutive_errors: u32,
    },
    /// Consecutive errors exceeded threshold — connection degraded.
    TunnelRestart {
        consecutive_errors: u32,
        threshold: u32,
    },
    /// ACK receiver channel closed.
    ChannelClosed,
}

impl AckResult {
    /// Whether the delivery was successful.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }
}

/// A received ACK message.
#[derive(Debug, Clone)]
pub struct AckMessage {
    /// Channel ID from the ACK.
    pub channel_id: u8,
    /// Sequence number being ACKed.
    pub sequence: u8,
    /// Status code (0 = OK).
    pub status: u8,
}

/// Lock-free ACK waiter statistics.
#[derive(Debug)]
struct AckWaiterStats {
    total_waits: AtomicU64,
    first_try_success: AtomicU64,
    retry_success: AtomicU64,
    failures: AtomicU64,
    retransmissions: AtomicU64,
    avg_ack_latency_us: AtomicU64,
}

impl Default for AckWaiterStats {
    fn default() -> Self {
        Self {
            total_waits: AtomicU64::new(0),
            first_try_success: AtomicU64::new(0),
            retry_success: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            retransmissions: AtomicU64::new(0),
            avg_ack_latency_us: AtomicU64::new(0),
        }
    }
}

/// Snapshot of ACK waiter stats for reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AckWaiterStatsSnapshot {
    pub total_waits: u64,
    pub first_try_success: u64,
    pub retry_success: u64,
    pub failures: u64,
    pub retransmissions: u64,
    pub avg_ack_latency_us: u64,
}

/// Server-side ACK waiter with timeout, retry, and consecutive error tracking.
#[derive(Debug)]
pub struct AckWaiter {
    ack_timeout: Duration,
    max_retries: u8,
    consecutive_send_errors: AtomicU32,
    send_error_threshold: u32,
    stats: AckWaiterStats,
}

impl AckWaiter {
    /// Create a new AckWaiter with default settings (1s timeout, 3 retries, threshold 5).
    pub fn new() -> Self {
        Self {
            ack_timeout: Duration::from_secs(1),
            max_retries: 3,
            consecutive_send_errors: AtomicU32::new(0),
            send_error_threshold: 5,
            stats: AckWaiterStats::default(),
        }
    }

    /// Create with custom parameters.
    pub fn with_config(ack_timeout: Duration, max_retries: u8, send_error_threshold: u32) -> Self {
        Self {
            ack_timeout,
            max_retries,
            consecutive_send_errors: AtomicU32::new(0),
            send_error_threshold,
            stats: AckWaiterStats::default(),
        }
    }

    /// Wait for a matching ACK on the given receiver channel.
    ///
    /// Blocks until:
    /// - Matching ACK received → Success
    /// - Timeout → returns None for retry logic
    /// - Channel closed → ChannelClosed
    pub async fn wait_for_ack(
        &self,
        ack_rx: &mut mpsc::Receiver<AckMessage>,
        expected_sequence: u8,
        channel_id: u8,
    ) -> AckResult {
        self.stats.total_waits.fetch_add(1, Ordering::Relaxed);
        let start = Instant::now();

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                self.stats.retransmissions.fetch_add(1, Ordering::Relaxed);
            }

            let timeout = tokio::time::sleep(self.ack_timeout);
            tokio::pin!(timeout);

            loop {
                tokio::select! {
                    msg = ack_rx.recv() => {
                        match msg {
                            Some(ack) if ack.channel_id == channel_id && ack.sequence == expected_sequence => {
                                let latency = start.elapsed();

                                if ack.status != 0 {
                                    return AckResult::AckError {
                                        status: ack.status,
                                        attempt,
                                    };
                                }

                                // Update EMA latency: new = (prev * 4 + current) / 5
                                let current_us = latency.as_micros() as u64;
                                let prev = self.stats.avg_ack_latency_us.load(Ordering::Relaxed);
                                let new_avg = if prev == 0 {
                                    current_us
                                } else {
                                    (prev * 4 + current_us) / 5
                                };
                                self.stats.avg_ack_latency_us.store(new_avg, Ordering::Relaxed);

                                self.consecutive_send_errors.store(0, Ordering::SeqCst);

                                if attempt == 0 {
                                    self.stats.first_try_success.fetch_add(1, Ordering::Relaxed);
                                } else {
                                    self.stats.retry_success.fetch_add(1, Ordering::Relaxed);
                                }

                                return AckResult::Success { latency, attempt };
                            }
                            Some(_) => {
                                // Non-matching ACK — skip and keep waiting
                                continue;
                            }
                            None => {
                                return AckResult::ChannelClosed;
                            }
                        }
                    }
                    _ = &mut timeout => {
                        debug!(
                            channel_id,
                            sequence = expected_sequence,
                            attempt,
                            "ACK timeout"
                        );
                        break; // Break inner loop to retry
                    }
                }
            }
        }

        // All retries exhausted
        let errors = self.consecutive_send_errors.fetch_add(1, Ordering::SeqCst) + 1;
        self.stats.failures.fetch_add(1, Ordering::Relaxed);

        if errors >= self.send_error_threshold {
            warn!(
                consecutive_errors = errors,
                threshold = self.send_error_threshold,
                "Send error threshold exceeded"
            );
            AckResult::TunnelRestart {
                consecutive_errors: errors,
                threshold: self.send_error_threshold,
            }
        } else {
            AckResult::MaxRetriesExceeded {
                attempts: self.max_retries + 1,
                consecutive_errors: errors,
            }
        }
    }

    /// Record a successful send (resets consecutive error counter).
    pub fn record_success(&self) {
        self.consecutive_send_errors.store(0, Ordering::SeqCst);
    }

    /// Get current consecutive error count.
    pub fn consecutive_errors(&self) -> u32 {
        self.consecutive_send_errors.load(Ordering::SeqCst)
    }

    /// Reset consecutive error counter.
    pub fn reset_errors(&self) {
        self.consecutive_send_errors.store(0, Ordering::SeqCst);
    }

    /// Check if threshold is exceeded.
    pub fn is_threshold_exceeded(&self) -> bool {
        self.consecutive_errors() >= self.send_error_threshold
    }

    /// Get statistics snapshot.
    pub fn stats_snapshot(&self) -> AckWaiterStatsSnapshot {
        AckWaiterStatsSnapshot {
            total_waits: self.stats.total_waits.load(Ordering::Relaxed),
            first_try_success: self.stats.first_try_success.load(Ordering::Relaxed),
            retry_success: self.stats.retry_success.load(Ordering::Relaxed),
            failures: self.stats.failures.load(Ordering::Relaxed),
            retransmissions: self.stats.retransmissions.load(Ordering::Relaxed),
            avg_ack_latency_us: self.stats.avg_ack_latency_us.load(Ordering::Relaxed),
        }
    }
}

impl Default for AckWaiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ack_success_first_try() {
        let waiter = AckWaiter::new();
        let (tx, mut rx) = mpsc::channel(8);

        // Spawn sender immediately
        let send_handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            tx.send(AckMessage {
                channel_id: 1,
                sequence: 0,
                status: 0,
            })
            .await
            .unwrap();
        });

        let result = waiter.wait_for_ack(&mut rx, 0, 1).await;
        send_handle.await.unwrap();

        assert!(result.is_success());
        if let AckResult::Success { attempt, .. } = result {
            assert_eq!(attempt, 0);
        }
        assert_eq!(waiter.consecutive_errors(), 0);

        let stats = waiter.stats_snapshot();
        assert_eq!(stats.total_waits, 1);
        assert_eq!(stats.first_try_success, 1);
    }

    #[tokio::test]
    async fn test_ack_error_status() {
        let waiter = AckWaiter::new();
        let (tx, mut rx) = mpsc::channel(8);

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            tx.send(AckMessage {
                channel_id: 1,
                sequence: 0,
                status: 0x21,
            })
            .await
            .unwrap();
        });

        let result = waiter.wait_for_ack(&mut rx, 0, 1).await;
        assert!(matches!(
            result,
            AckResult::AckError {
                status: 0x21,
                attempt: 0
            }
        ));
    }

    #[tokio::test]
    async fn test_ack_timeout() {
        let waiter = AckWaiter::with_config(
            Duration::from_millis(50),
            0, // No retries
            5,
        );
        let (_tx, mut rx) = mpsc::channel::<AckMessage>(8);

        let result = waiter.wait_for_ack(&mut rx, 0, 1).await;
        assert!(matches!(
            result,
            AckResult::MaxRetriesExceeded { attempts: 1, .. }
        ));
        assert_eq!(waiter.consecutive_errors(), 1);
    }

    #[tokio::test]
    async fn test_channel_closed() {
        let waiter = AckWaiter::new();
        let (tx, mut rx) = mpsc::channel(8);
        drop(tx); // Close immediately

        let result = waiter.wait_for_ack(&mut rx, 0, 1).await;
        assert!(matches!(result, AckResult::ChannelClosed));
    }

    #[tokio::test]
    async fn test_skip_non_matching_ack() {
        let waiter = AckWaiter::new();
        let (tx, mut rx) = mpsc::channel(8);

        tokio::spawn(async move {
            // Send wrong sequence first
            tx.send(AckMessage {
                channel_id: 1,
                sequence: 99,
                status: 0,
            })
            .await
            .unwrap();
            tokio::time::sleep(Duration::from_millis(5)).await;
            // Then send correct one
            tx.send(AckMessage {
                channel_id: 1,
                sequence: 5,
                status: 0,
            })
            .await
            .unwrap();
        });

        let result = waiter.wait_for_ack(&mut rx, 5, 1).await;
        assert!(result.is_success());
    }

    #[tokio::test]
    async fn test_threshold_exceeded() {
        let waiter = AckWaiter::with_config(
            Duration::from_millis(20),
            0, // No retries
            3, // Low threshold
        );
        let (_tx, mut rx) = mpsc::channel::<AckMessage>(8);

        // Exhaust threshold
        for _ in 0..2 {
            let _ = waiter.wait_for_ack(&mut rx, 0, 1).await;
        }

        let result = waiter.wait_for_ack(&mut rx, 0, 1).await;
        assert!(matches!(
            result,
            AckResult::TunnelRestart {
                consecutive_errors: 3,
                threshold: 3
            }
        ));
    }

    #[test]
    fn test_record_success_resets_errors() {
        let waiter = AckWaiter::new();
        waiter.consecutive_send_errors.store(3, Ordering::SeqCst);
        assert_eq!(waiter.consecutive_errors(), 3);

        waiter.record_success();
        assert_eq!(waiter.consecutive_errors(), 0);
    }
}
