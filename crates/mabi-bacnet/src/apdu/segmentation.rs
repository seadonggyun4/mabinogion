//! APDU Segmentation support for handling large messages.
//!
//! BACnet segmentation allows large messages to be split into smaller segments
//! for transmission when the message size exceeds the maximum APDU length.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                 Segmentation Manager                         │
//! │                                                             │
//! │  ┌─────────────────────────────────────────────────────┐   │
//! │  │            Segmentation State Machine               │   │
//! │  │                                                     │   │
//! │  │  IDLE → SENDING → WAIT_ACK → SENDING → COMPLETE    │   │
//! │  │              ↑                    │                 │   │
//! │  │              └────────────────────┘                 │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! │                                                             │
//! │  ┌────────────────────┐  ┌──────────────────────────┐      │
//! │  │  SegmentAssembler  │  │   SegmentTransmitter     │      │
//! │  │  (Receive side)    │  │   (Send side)            │      │
//! │  └────────────────────┘  └──────────────────────────┘      │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Features
//!
//! - **Automatic Segmentation**: Splits large responses into segments
//! - **Segment Reassembly**: Reconstructs segmented requests
//! - **Window Management**: Supports multiple outstanding segments
//! - **Timeout Handling**: Manages segment acknowledgment timeouts

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tracing::debug;

// ============================================================================
// Constants
// ============================================================================

/// Maximum segments in a single message.
pub const MAX_SEGMENTS_UNSPECIFIED: u8 = 0;
pub const MAX_SEGMENTS_2: u8 = 1;
pub const MAX_SEGMENTS_4: u8 = 2;
pub const MAX_SEGMENTS_8: u8 = 3;
pub const MAX_SEGMENTS_16: u8 = 4;
pub const MAX_SEGMENTS_32: u8 = 5;
pub const MAX_SEGMENTS_64: u8 = 6;
pub const MAX_SEGMENTS_MORE_THAN_64: u8 = 7;

/// Default segment timeout.
pub const DEFAULT_SEGMENT_TIMEOUT: Duration = Duration::from_secs(10);

/// Default proposed window size.
pub const DEFAULT_WINDOW_SIZE: u8 = 1;

// ============================================================================
// Segmentation Support Types
// ============================================================================

/// Segmentation support level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentationSupport {
    /// No segmentation supported.
    None,
    /// Transmit segmentation only.
    TransmitOnly,
    /// Receive segmentation only.
    ReceiveOnly,
    /// Both transmit and receive segmentation.
    Both,
}

impl SegmentationSupport {
    /// Create from a raw value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Both),
            1 => Some(Self::TransmitOnly),
            2 => Some(Self::ReceiveOnly),
            3 => Some(Self::None),
            _ => None,
        }
    }

    /// Convert to a raw value.
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Both => 0,
            Self::TransmitOnly => 1,
            Self::ReceiveOnly => 2,
            Self::None => 3,
        }
    }

    /// Check if transmit segmentation is supported.
    pub fn can_transmit(&self) -> bool {
        matches!(self, Self::Both | Self::TransmitOnly)
    }

    /// Check if receive segmentation is supported.
    pub fn can_receive(&self) -> bool {
        matches!(self, Self::Both | Self::ReceiveOnly)
    }
}

impl Default for SegmentationSupport {
    fn default() -> Self {
        Self::None
    }
}

/// Decode max segments from the encoded value.
pub fn decode_max_segments(encoded: u8) -> Option<usize> {
    match encoded {
        0 => None, // Unspecified
        1 => Some(2),
        2 => Some(4),
        3 => Some(8),
        4 => Some(16),
        5 => Some(32),
        6 => Some(64),
        7 => None, // More than 64
        _ => None,
    }
}

/// Encode max segments to the encoded value.
pub fn encode_max_segments(count: usize) -> u8 {
    match count {
        0..=2 => MAX_SEGMENTS_2,
        3..=4 => MAX_SEGMENTS_4,
        5..=8 => MAX_SEGMENTS_8,
        9..=16 => MAX_SEGMENTS_16,
        17..=32 => MAX_SEGMENTS_32,
        33..=64 => MAX_SEGMENTS_64,
        _ => MAX_SEGMENTS_MORE_THAN_64,
    }
}

// ============================================================================
// Segment Header
// ============================================================================

/// Segment header information.
#[derive(Debug, Clone, Copy)]
pub struct SegmentHeader {
    /// Is this a segmented message?
    pub segmented: bool,
    /// More segments follow?
    pub more_follows: bool,
    /// Is this a segment ACK?
    pub segment_ack: bool,
    /// Sequence number (0-255).
    pub sequence_number: u8,
    /// Proposed/actual window size.
    pub window_size: u8,
}

impl SegmentHeader {
    /// Create a new segment header.
    pub fn new(sequence_number: u8, more_follows: bool) -> Self {
        Self {
            segmented: true,
            more_follows,
            segment_ack: false,
            sequence_number,
            window_size: DEFAULT_WINDOW_SIZE,
        }
    }

    /// Create header for segment ACK.
    pub fn segment_ack(sequence_number: u8, window_size: u8) -> Self {
        Self {
            segmented: false,
            more_follows: false,
            segment_ack: true,
            sequence_number,
            window_size,
        }
    }

    /// Check if this is the first segment.
    pub fn is_first(&self) -> bool {
        self.segmented && self.sequence_number == 0
    }

    /// Check if this is the last segment.
    pub fn is_last(&self) -> bool {
        self.segmented && !self.more_follows
    }
}

// ============================================================================
// Segment
// ============================================================================

/// A single segment of a larger message.
#[derive(Debug, Clone)]
pub struct Segment {
    /// Segment header.
    pub header: SegmentHeader,
    /// Segment data (payload).
    pub data: Vec<u8>,
    /// Original invoke ID.
    pub invoke_id: u8,
    /// Service choice (if applicable).
    pub service_choice: Option<u8>,
}

impl Segment {
    /// Create a new segment.
    pub fn new(
        sequence_number: u8,
        more_follows: bool,
        invoke_id: u8,
        data: Vec<u8>,
    ) -> Self {
        Self {
            header: SegmentHeader::new(sequence_number, more_follows),
            data,
            invoke_id,
            service_choice: None,
        }
    }

    /// Set the service choice.
    pub fn with_service_choice(mut self, service_choice: u8) -> Self {
        self.service_choice = Some(service_choice);
        self
    }

    /// Get the segment size.
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

// ============================================================================
// Segment Assembler (Receive Side)
// ============================================================================

/// State of segment assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssemblyState {
    /// Waiting for first segment.
    Idle,
    /// Receiving segments.
    Receiving,
    /// Assembly complete.
    Complete,
    /// Assembly failed.
    Error,
}

/// Entry tracking a segmented message being assembled.
#[derive(Debug)]
struct AssemblyEntry {
    /// Expected total segments (if known).
    expected_segments: Option<usize>,
    /// Segments received so far, indexed by sequence number.
    segments: HashMap<u8, Vec<u8>>,
    /// Next expected sequence number.
    next_sequence: u8,
    /// Invoke ID of the transaction.
    invoke_id: u8,
    /// Service choice.
    service_choice: Option<u8>,
    /// Assembly state.
    state: AssemblyState,
    /// Time of last activity.
    last_activity: Instant,
    /// Actual window size being used.
    actual_window_size: u8,
}

impl AssemblyEntry {
    fn new(invoke_id: u8) -> Self {
        Self {
            expected_segments: None,
            segments: HashMap::new(),
            next_sequence: 0,
            invoke_id,
            service_choice: None,
            state: AssemblyState::Idle,
            last_activity: Instant::now(),
            actual_window_size: DEFAULT_WINDOW_SIZE,
        }
    }

    fn add_segment(&mut self, segment: &Segment) -> Result<AssemblyResult, SegmentationError> {
        self.last_activity = Instant::now();

        // Check sequence number
        if segment.header.sequence_number != self.next_sequence {
            if segment.header.sequence_number < self.next_sequence {
                // Duplicate segment, ignore
                return Ok(AssemblyResult::Duplicate);
            }
            // Out of order or missing segment
            return Err(SegmentationError::SequenceError {
                expected: self.next_sequence,
                received: segment.header.sequence_number,
            });
        }

        // Store segment data
        self.segments
            .insert(segment.header.sequence_number, segment.data.clone());
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.state = AssemblyState::Receiving;

        // Update service choice if this is the first segment
        if segment.header.is_first() {
            self.service_choice = segment.service_choice;
        }

        // Check if complete
        if segment.header.is_last() {
            self.state = AssemblyState::Complete;
            return Ok(AssemblyResult::Complete);
        }

        // Need to send segment ACK
        Ok(AssemblyResult::NeedAck(self.next_sequence.wrapping_sub(1)))
    }

    fn assemble(&self) -> Result<Vec<u8>, SegmentationError> {
        if self.state != AssemblyState::Complete {
            return Err(SegmentationError::IncompleteAssembly);
        }

        let mut data = Vec::new();
        for i in 0..self.next_sequence {
            match self.segments.get(&i) {
                Some(segment_data) => data.extend_from_slice(segment_data),
                None => return Err(SegmentationError::MissingSegment(i)),
            }
        }

        Ok(data)
    }
}

/// Result of adding a segment to the assembler.
#[derive(Debug)]
pub enum AssemblyResult {
    /// Need to send segment ACK with this sequence number.
    NeedAck(u8),
    /// Assembly is complete.
    Complete,
    /// Duplicate segment received (ignored).
    Duplicate,
}

/// Assembles segmented messages back into complete messages.
pub struct SegmentAssembler {
    /// Active assembly entries, keyed by (source address hash, invoke ID).
    entries: HashMap<(u64, u8), AssemblyEntry>,
    /// Timeout for stale entries.
    timeout: Duration,
    /// Maximum concurrent assemblies.
    max_entries: usize,
}

impl SegmentAssembler {
    /// Create a new segment assembler.
    pub fn new(timeout: Duration, max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            timeout,
            max_entries,
        }
    }

    /// Process an incoming segment.
    pub fn process_segment(
        &mut self,
        source_hash: u64,
        segment: &Segment,
    ) -> Result<AssemblyResult, SegmentationError> {
        let key = (source_hash, segment.invoke_id);

        // Get or create entry
        let entry = self.entries.entry(key).or_insert_with(|| {
            AssemblyEntry::new(segment.invoke_id)
        });

        // Check for stale entry
        if entry.last_activity.elapsed() > self.timeout {
            debug!(invoke_id = segment.invoke_id, "Resetting stale assembly entry");
            *entry = AssemblyEntry::new(segment.invoke_id);
        }

        entry.add_segment(segment)
    }

    /// Get the assembled message if complete.
    pub fn get_complete(
        &mut self,
        source_hash: u64,
        invoke_id: u8,
    ) -> Option<(Vec<u8>, Option<u8>)> {
        let key = (source_hash, invoke_id);

        if let Some(entry) = self.entries.get(&key) {
            if entry.state == AssemblyState::Complete {
                if let Ok(data) = entry.assemble() {
                    let service_choice = entry.service_choice;
                    self.entries.remove(&key);
                    return Some((data, service_choice));
                }
            }
        }

        None
    }

    /// Clean up stale entries.
    pub fn cleanup(&mut self) -> usize {
        let stale: Vec<(u64, u8)> = self
            .entries
            .iter()
            .filter(|(_, e)| e.last_activity.elapsed() > self.timeout)
            .map(|(k, _)| *k)
            .collect();

        let count = stale.len();
        for key in stale {
            self.entries.remove(&key);
        }

        if count > 0 {
            debug!(count, "Cleaned up stale segment assembly entries");
        }

        count
    }

    /// Get the number of active assemblies.
    pub fn active_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for SegmentAssembler {
    fn default() -> Self {
        Self::new(DEFAULT_SEGMENT_TIMEOUT, 100)
    }
}

// ============================================================================
// Segment Transmitter (Send Side)
// ============================================================================

/// Splits a large message into segments for transmission.
pub struct SegmentTransmitter {
    /// Maximum segment size.
    max_segment_size: usize,
    /// Proposed window size.
    proposed_window_size: u8,
}

impl SegmentTransmitter {
    /// Create a new segment transmitter.
    pub fn new(max_segment_size: usize) -> Self {
        Self {
            max_segment_size,
            proposed_window_size: DEFAULT_WINDOW_SIZE,
        }
    }

    /// Set the proposed window size.
    pub fn with_window_size(mut self, size: u8) -> Self {
        self.proposed_window_size = size;
        self
    }

    /// Check if segmentation is needed for the given data size.
    pub fn needs_segmentation(&self, data_len: usize) -> bool {
        data_len > self.max_segment_size
    }

    /// Segment a message into multiple segments.
    pub fn segment(&self, data: &[u8], invoke_id: u8) -> Vec<Segment> {
        if !self.needs_segmentation(data.len()) {
            // No segmentation needed, return single segment
            return vec![Segment::new(0, false, invoke_id, data.to_vec())];
        }

        let mut segments = Vec::new();
        let mut sequence_number: u8 = 0;
        let mut offset = 0;

        while offset < data.len() {
            let end = (offset + self.max_segment_size).min(data.len());
            let segment_data = data[offset..end].to_vec();
            let more_follows = end < data.len();

            segments.push(Segment::new(
                sequence_number,
                more_follows,
                invoke_id,
                segment_data,
            ));

            sequence_number = sequence_number.wrapping_add(1);
            offset = end;
        }

        debug!(
            total_size = data.len(),
            segment_count = segments.len(),
            "Message segmented"
        );

        segments
    }

    /// Calculate the number of segments needed.
    pub fn calculate_segment_count(&self, data_len: usize) -> usize {
        if data_len == 0 {
            return 1;
        }
        (data_len + self.max_segment_size - 1) / self.max_segment_size
    }
}

impl Default for SegmentTransmitter {
    fn default() -> Self {
        // Default to common APDU size minus header overhead
        Self::new(480)
    }
}

// ============================================================================
// Errors
// ============================================================================

/// Segmentation errors.
#[derive(Debug, thiserror::Error)]
pub enum SegmentationError {
    #[error("Sequence error: expected {expected}, received {received}")]
    SequenceError { expected: u8, received: u8 },

    #[error("Missing segment: {0}")]
    MissingSegment(u8),

    #[error("Incomplete assembly")]
    IncompleteAssembly,

    #[error("Segment timeout")]
    Timeout,

    #[error("Too many segments")]
    TooManySegments,

    #[error("Segment too large")]
    SegmentTooLarge,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segmentation_support() {
        assert!(SegmentationSupport::Both.can_transmit());
        assert!(SegmentationSupport::Both.can_receive());
        assert!(SegmentationSupport::TransmitOnly.can_transmit());
        assert!(!SegmentationSupport::TransmitOnly.can_receive());
        assert!(!SegmentationSupport::None.can_transmit());
        assert!(!SegmentationSupport::None.can_receive());
    }

    #[test]
    fn test_segment_header() {
        let header = SegmentHeader::new(0, true);
        assert!(header.is_first());
        assert!(!header.is_last());

        let header2 = SegmentHeader::new(5, false);
        assert!(!header2.is_first());
        assert!(header2.is_last());
    }

    #[test]
    fn test_transmitter_no_segmentation() {
        let transmitter = SegmentTransmitter::new(100);
        let data = vec![0u8; 50];

        assert!(!transmitter.needs_segmentation(data.len()));

        let segments = transmitter.segment(&data, 1);
        assert_eq!(segments.len(), 1);
        assert!(!segments[0].header.more_follows);
    }

    #[test]
    fn test_transmitter_with_segmentation() {
        let transmitter = SegmentTransmitter::new(100);
        let data = vec![0u8; 250];

        assert!(transmitter.needs_segmentation(data.len()));
        assert_eq!(transmitter.calculate_segment_count(data.len()), 3);

        let segments = transmitter.segment(&data, 1);
        assert_eq!(segments.len(), 3);

        assert_eq!(segments[0].header.sequence_number, 0);
        assert!(segments[0].header.more_follows);

        assert_eq!(segments[1].header.sequence_number, 1);
        assert!(segments[1].header.more_follows);

        assert_eq!(segments[2].header.sequence_number, 2);
        assert!(!segments[2].header.more_follows);

        // Verify total data
        let total: usize = segments.iter().map(|s| s.data.len()).sum();
        assert_eq!(total, 250);
    }

    #[test]
    fn test_assembler_simple() {
        let mut assembler = SegmentAssembler::default();
        let source_hash = 12345u64;
        let invoke_id = 1;

        // First segment
        let seg1 = Segment::new(0, true, invoke_id, vec![1, 2, 3]);
        let result = assembler.process_segment(source_hash, &seg1).unwrap();
        assert!(matches!(result, AssemblyResult::NeedAck(0)));

        // Second segment
        let seg2 = Segment::new(1, true, invoke_id, vec![4, 5, 6]);
        let result = assembler.process_segment(source_hash, &seg2).unwrap();
        assert!(matches!(result, AssemblyResult::NeedAck(1)));

        // Last segment
        let seg3 = Segment::new(2, false, invoke_id, vec![7, 8, 9]);
        let result = assembler.process_segment(source_hash, &seg3).unwrap();
        assert!(matches!(result, AssemblyResult::Complete));

        // Get complete message
        let (data, _service) = assembler.get_complete(source_hash, invoke_id).unwrap();
        assert_eq!(data, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn test_assembler_out_of_order() {
        let mut assembler = SegmentAssembler::default();
        let source_hash = 12345u64;
        let invoke_id = 1;

        // First segment
        let seg1 = Segment::new(0, true, invoke_id, vec![1, 2, 3]);
        assembler.process_segment(source_hash, &seg1).unwrap();

        // Skip segment 1, send segment 2
        let seg3 = Segment::new(2, false, invoke_id, vec![7, 8, 9]);
        let result = assembler.process_segment(source_hash, &seg3);

        assert!(matches!(
            result,
            Err(SegmentationError::SequenceError {
                expected: 1,
                received: 2
            })
        ));
    }

    #[test]
    fn test_max_segments_encoding() {
        assert_eq!(decode_max_segments(1), Some(2));
        assert_eq!(decode_max_segments(4), Some(16));
        assert_eq!(decode_max_segments(6), Some(64));
        assert_eq!(decode_max_segments(0), None); // Unspecified

        assert_eq!(encode_max_segments(2), MAX_SEGMENTS_2);
        assert_eq!(encode_max_segments(16), MAX_SEGMENTS_16);
        assert_eq!(encode_max_segments(100), MAX_SEGMENTS_MORE_THAN_64);
    }

    #[test]
    fn test_round_trip() {
        // Test that segmentation and reassembly produces the original data
        let transmitter = SegmentTransmitter::new(100);
        let mut assembler = SegmentAssembler::default();
        let source_hash = 99999u64;
        let invoke_id = 42;

        let original_data: Vec<u8> = (0..500).map(|i| (i % 256) as u8).collect();

        // Segment the data
        let segments = transmitter.segment(&original_data, invoke_id);
        assert_eq!(segments.len(), 5);

        // Feed segments to assembler
        for segment in &segments {
            let result = assembler.process_segment(source_hash, segment).unwrap();
            if segment.header.is_last() {
                assert!(matches!(result, AssemblyResult::Complete));
            } else {
                assert!(matches!(result, AssemblyResult::NeedAck(_)));
            }
        }

        // Get reassembled data
        let (reassembled, _) = assembler.get_complete(source_hash, invoke_id).unwrap();
        assert_eq!(reassembled, original_data);
    }
}
