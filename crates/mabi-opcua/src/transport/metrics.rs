//! Transport layer metrics for OPC UA TCP connections.

use std::sync::atomic::{AtomicU64, Ordering};

/// Metrics for the OPC UA TCP transport layer.
#[derive(Debug, Default)]
pub struct TransportMetrics {
    pub connections_total: AtomicU64,
    pub connections_active: AtomicU64,
    pub connections_rejected: AtomicU64,
    pub messages_received: AtomicU64,
    pub messages_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub errors: AtomicU64,
}

impl TransportMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_connection(&self) {
        self.connections_total.fetch_add(1, Ordering::Relaxed);
        self.connections_active.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_disconnection(&self) {
        self.connections_active.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn record_rejection(&self) {
        self.connections_rejected.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_message_received(&self, bytes: usize) {
        self.messages_received.fetch_add(1, Ordering::Relaxed);
        self.bytes_received
            .fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn record_message_sent(&self, bytes: usize) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn active_connections(&self) -> u64 {
        self.connections_active.load(Ordering::Relaxed)
    }
}
