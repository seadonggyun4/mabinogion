//! Server-side sequence number tracking (knxd-compatible).
//!
//! Implements rno/sno dual sequence tracking with CAS-based wrapping,
//! matching knxd's `eibnettunnel.cpp` validation logic:
//!
//! - `(seqno+1) & 0xff == rno` → Duplicate (ACK only, don't process)
//! - `distance >= FATAL_DESYNC_THRESHOLD` → FatalDesync (tunnel restart)
//! - else → OutOfOrder (log warning)

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

/// Distance threshold for fatal desync (knxd: `seqno >= rno + 5`).
const FATAL_DESYNC_THRESHOLD: u8 = 5;

/// Modular distance boundary — distances >= 128 are considered backward.
const FORWARD_BOUNDARY: u8 = 128;

/// Result of validating a received sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceivedValidation {
    /// Sequence matches expected — advance rno and process frame.
    Valid {
        sequence: u8,
    },
    /// Duplicate of the last processed frame — send ACK but don't process.
    /// knxd: `(seqno+1) & 0xff == rno`
    Duplicate {
        sequence: u8,
        expected: u8,
    },
    /// Mild out-of-order (1-4 frames ahead) — log warning, send ACK.
    OutOfOrder {
        sequence: u8,
        expected: u8,
        distance: u8,
    },
    /// Fatal desync (5+ frames) — requires tunnel restart.
    /// knxd: `seqno >= rno + 5`
    FatalDesync {
        sequence: u8,
        expected: u8,
        distance: u8,
    },
}

impl ReceivedValidation {
    /// Whether the frame should be processed (forwarded to application).
    pub fn should_process(&self) -> bool {
        matches!(self, Self::Valid { .. } | Self::OutOfOrder { .. })
    }

    /// Whether an ACK should be sent for this frame.
    pub fn should_ack(&self) -> bool {
        !matches!(self, Self::FatalDesync { .. })
    }

    /// Whether the tunnel should be restarted.
    pub fn requires_restart(&self) -> bool {
        matches!(self, Self::FatalDesync { .. })
    }
}

/// Result of validating a sent sequence's ACK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckValidation {
    /// ACK matches the last sent sequence.
    Valid,
    /// ACK does not match expected sequence.
    Mismatch { expected: u8, actual: u8 },
}

/// Lock-free sequence statistics.
#[derive(Debug)]
pub struct SequenceStats {
    frames_sent: AtomicU64,
    frames_received: AtomicU64,
    duplicates_detected: AtomicU64,
    out_of_order_detected: AtomicU64,
    fatal_desyncs: AtomicU64,
    resets: AtomicU64,
}

impl Default for SequenceStats {
    fn default() -> Self {
        Self {
            frames_sent: AtomicU64::new(0),
            frames_received: AtomicU64::new(0),
            duplicates_detected: AtomicU64::new(0),
            out_of_order_detected: AtomicU64::new(0),
            fatal_desyncs: AtomicU64::new(0),
            resets: AtomicU64::new(0),
        }
    }
}

/// Snapshot of sequence statistics for reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceStatsSnapshot {
    pub frames_sent: u64,
    pub frames_received: u64,
    pub duplicates_detected: u64,
    pub out_of_order_detected: u64,
    pub fatal_desyncs: u64,
    pub resets: u64,
}

/// Server-side sequence tracker with knxd-compatible validation.
///
/// Tracks two independent sequence counters:
/// - **sno** (send number): incremented for each frame sent to the client
/// - **rno** (receive number): expected next sequence from the client
///
/// Both counters wrap at 255→0 using atomic CAS to prevent race conditions.
#[derive(Debug)]
pub struct SequenceTracker {
    /// Send sequence number — next value to use when sending a frame to the client.
    sno: AtomicU8,
    /// Receive sequence number — next expected value from the client.
    rno: AtomicU8,
    /// Fatal desync threshold (configurable, default: 5).
    fatal_desync_threshold: u8,
    /// Statistics counters.
    stats: SequenceStats,
}

impl SequenceTracker {
    /// Create a new sequence tracker with default thresholds.
    pub fn new() -> Self {
        Self {
            sno: AtomicU8::new(0),
            rno: AtomicU8::new(0),
            fatal_desync_threshold: FATAL_DESYNC_THRESHOLD,
            stats: SequenceStats::default(),
        }
    }

    /// Create with a custom fatal desync threshold.
    pub fn with_desync_threshold(threshold: u8) -> Self {
        Self {
            sno: AtomicU8::new(0),
            rno: AtomicU8::new(0),
            fatal_desync_threshold: threshold.max(2),
            stats: SequenceStats::default(),
        }
    }

    /// Get the next send sequence number, incrementing atomically with CAS.
    ///
    /// Returns the current sno value and advances to the next.
    /// Uses compare_exchange loop to safely handle 255→0 wrapping.
    pub fn next_sno(&self) -> u8 {
        loop {
            let current = self.sno.load(Ordering::SeqCst);
            let next = current.wrapping_add(1);
            if self.sno.compare_exchange(current, next, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                self.stats.frames_sent.fetch_add(1, Ordering::Relaxed);
                return current;
            }
        }
    }

    /// Peek at the current sno without incrementing.
    pub fn current_sno(&self) -> u8 {
        self.sno.load(Ordering::SeqCst)
    }

    /// Peek at the current rno (expected receive sequence).
    pub fn current_rno(&self) -> u8 {
        self.rno.load(Ordering::SeqCst)
    }

    /// Validate a received sequence number against expected rno.
    ///
    /// Implements knxd's 4-branch validation:
    /// 1. `seqno == rno` → Valid, advance rno
    /// 2. `(seqno+1) & 0xff == rno` → Duplicate of last processed
    /// 3. forward distance < FATAL_DESYNC_THRESHOLD → OutOfOrder
    /// 4. forward distance >= FATAL_DESYNC_THRESHOLD → FatalDesync
    pub fn validate_received(&self, sequence: u8) -> ReceivedValidation {
        let expected = self.rno.load(Ordering::SeqCst);

        // Case 1: exact match — valid frame
        if sequence == expected {
            // CAS loop to safely advance rno
            loop {
                let current = self.rno.load(Ordering::SeqCst);
                if current != expected {
                    // Another thread already advanced — still Valid but don't double-advance
                    break;
                }
                let next = expected.wrapping_add(1);
                if self.rno.compare_exchange(current, next, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                    break;
                }
            }
            self.stats.frames_received.fetch_add(1, Ordering::Relaxed);
            return ReceivedValidation::Valid { sequence };
        }

        // Case 2: duplicate — (seqno+1) & 0xff == rno
        // This means seqno == rno - 1 (the frame we just processed)
        if sequence.wrapping_add(1) == expected {
            self.stats.duplicates_detected.fetch_add(1, Ordering::Relaxed);
            return ReceivedValidation::Duplicate { sequence, expected };
        }

        // Calculate forward distance (modular, 0-255)
        let distance = sequence.wrapping_sub(expected);

        // Case 3/4: check if distance is "forward" (< 128) or "backward" (>= 128)
        if distance < FORWARD_BOUNDARY {
            if distance >= self.fatal_desync_threshold {
                // Case 4: Fatal desync
                self.stats.fatal_desyncs.fetch_add(1, Ordering::Relaxed);
                ReceivedValidation::FatalDesync {
                    sequence,
                    expected,
                    distance,
                }
            } else {
                // Case 3: Mild out-of-order (1-4 frames ahead)
                self.stats.out_of_order_detected.fetch_add(1, Ordering::Relaxed);
                ReceivedValidation::OutOfOrder {
                    sequence,
                    expected,
                    distance,
                }
            }
        } else {
            // Backward distance — treat as duplicate of an older frame
            self.stats.duplicates_detected.fetch_add(1, Ordering::Relaxed);
            ReceivedValidation::Duplicate { sequence, expected }
        }
    }

    /// Validate that an ACK matches the last sent sequence.
    pub fn validate_ack(&self, acked_sequence: u8) -> AckValidation {
        let last_sent = self.sno.load(Ordering::SeqCst).wrapping_sub(1);
        if acked_sequence == last_sent {
            AckValidation::Valid
        } else {
            AckValidation::Mismatch {
                expected: last_sent,
                actual: acked_sequence,
            }
        }
    }

    /// Reset both sequence counters (used on tunnel reconnect).
    pub fn reset(&self) {
        self.sno.store(0, Ordering::SeqCst);
        self.rno.store(0, Ordering::SeqCst);
        self.stats.resets.fetch_add(1, Ordering::Relaxed);
    }

    /// Get a snapshot of current statistics.
    pub fn stats_snapshot(&self) -> SequenceStatsSnapshot {
        SequenceStatsSnapshot {
            frames_sent: self.stats.frames_sent.load(Ordering::Relaxed),
            frames_received: self.stats.frames_received.load(Ordering::Relaxed),
            duplicates_detected: self.stats.duplicates_detected.load(Ordering::Relaxed),
            out_of_order_detected: self.stats.out_of_order_detected.load(Ordering::Relaxed),
            fatal_desyncs: self.stats.fatal_desyncs.load(Ordering::Relaxed),
            resets: self.stats.resets.load(Ordering::Relaxed),
        }
    }

    /// Build a TUNNELLING_ACK frame body for the given channel and sequence.
    pub fn build_ack_frame(channel_id: u8, sequence: u8, status: u8) -> Vec<u8> {
        vec![4, channel_id, sequence, status]
    }
}

impl Default for SequenceTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_sequence() {
        let tracker = SequenceTracker::new();
        assert_eq!(tracker.current_rno(), 0);

        let result = tracker.validate_received(0);
        assert!(matches!(result, ReceivedValidation::Valid { sequence: 0 }));
        assert_eq!(tracker.current_rno(), 1);

        let result = tracker.validate_received(1);
        assert!(matches!(result, ReceivedValidation::Valid { sequence: 1 }));
        assert_eq!(tracker.current_rno(), 2);
    }

    #[test]
    fn test_duplicate_detection() {
        let tracker = SequenceTracker::new();

        // Process frame 0
        let result = tracker.validate_received(0);
        assert!(matches!(result, ReceivedValidation::Valid { .. }));
        assert_eq!(tracker.current_rno(), 1);

        // Duplicate of frame 0: (0+1)&0xff == 1 == rno
        let result = tracker.validate_received(0);
        assert!(matches!(result, ReceivedValidation::Duplicate { sequence: 0, expected: 1 }));
        assert!(result.should_ack());
        assert!(!result.should_process());
        assert_eq!(tracker.current_rno(), 1); // rno unchanged
    }

    #[test]
    fn test_out_of_order() {
        let tracker = SequenceTracker::new();

        // Skip to sequence 2 (distance = 2, < 5)
        let result = tracker.validate_received(2);
        assert!(matches!(result, ReceivedValidation::OutOfOrder { sequence: 2, expected: 0, distance: 2 }));
        assert!(result.should_process());
        assert!(result.should_ack());
    }

    #[test]
    fn test_fatal_desync() {
        let tracker = SequenceTracker::new();

        // Skip to sequence 5 (distance = 5, >= threshold)
        let result = tracker.validate_received(5);
        assert!(matches!(result, ReceivedValidation::FatalDesync { sequence: 5, expected: 0, distance: 5 }));
        assert!(result.requires_restart());
        assert!(!result.should_ack());
    }

    #[test]
    fn test_wrapping_at_255() {
        let tracker = SequenceTracker::new();

        // Advance rno to 255
        for i in 0..255u8 {
            let result = tracker.validate_received(i);
            assert!(matches!(result, ReceivedValidation::Valid { .. }));
        }
        assert_eq!(tracker.current_rno(), 255);

        // Frame 255 should be valid
        let result = tracker.validate_received(255);
        assert!(matches!(result, ReceivedValidation::Valid { .. }));
        assert_eq!(tracker.current_rno(), 0); // Wrapped!

        // Frame 0 after wrap should be valid
        let result = tracker.validate_received(0);
        assert!(matches!(result, ReceivedValidation::Valid { .. }));
        assert_eq!(tracker.current_rno(), 1);
    }

    #[test]
    fn test_duplicate_at_wrap_boundary() {
        let tracker = SequenceTracker::new();

        // Advance to rno=0 by wrapping around
        for i in 0..=255u8 {
            tracker.validate_received(i);
        }
        assert_eq!(tracker.current_rno(), 0);

        // 255 is now the duplicate: (255+1)&0xff == 0 == rno
        let result = tracker.validate_received(255);
        assert!(matches!(result, ReceivedValidation::Duplicate { sequence: 255, expected: 0 }));
    }

    #[test]
    fn test_sno_cas_wrapping() {
        let tracker = SequenceTracker::new();

        for expected in 0..=255u8 {
            let got = tracker.next_sno();
            assert_eq!(got, expected);
        }
        // After 256 calls, should wrap to 0
        assert_eq!(tracker.next_sno(), 0);
    }

    #[test]
    fn test_ack_validation() {
        let tracker = SequenceTracker::new();

        let seq = tracker.next_sno(); // sno goes to 1, returns 0
        assert_eq!(seq, 0);

        assert_eq!(tracker.validate_ack(0), AckValidation::Valid);
        assert!(matches!(tracker.validate_ack(1), AckValidation::Mismatch { expected: 0, actual: 1 }));
    }

    #[test]
    fn test_reset() {
        let tracker = SequenceTracker::new();

        tracker.next_sno();
        tracker.next_sno();
        tracker.validate_received(0);
        tracker.validate_received(1);

        assert_eq!(tracker.current_sno(), 2);
        assert_eq!(tracker.current_rno(), 2);

        tracker.reset();

        assert_eq!(tracker.current_sno(), 0);
        assert_eq!(tracker.current_rno(), 0);
        assert_eq!(tracker.stats_snapshot().resets, 1);
    }

    #[test]
    fn test_stats_tracking() {
        let tracker = SequenceTracker::new();

        tracker.validate_received(0); // Valid
        tracker.validate_received(0); // Duplicate
        tracker.validate_received(3); // OutOfOrder (distance=2)
        tracker.validate_received(10); // FatalDesync (distance=9)

        let stats = tracker.stats_snapshot();
        assert_eq!(stats.frames_received, 1);
        assert_eq!(stats.duplicates_detected, 1);
        assert_eq!(stats.out_of_order_detected, 1);
        assert_eq!(stats.fatal_desyncs, 1);
    }

    #[test]
    fn test_custom_threshold() {
        let tracker = SequenceTracker::with_desync_threshold(10);

        // Distance 5 is now OutOfOrder, not FatalDesync
        let result = tracker.validate_received(5);
        assert!(matches!(result, ReceivedValidation::OutOfOrder { .. }));

        // Distance 10 is FatalDesync
        let result = tracker.validate_received(10);
        assert!(matches!(result, ReceivedValidation::FatalDesync { .. }));
    }

    #[test]
    fn test_build_ack_frame() {
        let frame = SequenceTracker::build_ack_frame(3, 7, 0);
        assert_eq!(frame, vec![4, 3, 7, 0]);

        let error_frame = SequenceTracker::build_ack_frame(3, 7, 0x21);
        assert_eq!(error_frame, vec![4, 3, 7, 0x21]);
    }
}
