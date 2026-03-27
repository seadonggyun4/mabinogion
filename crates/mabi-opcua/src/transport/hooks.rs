use std::sync::Arc;

use crate::transport::metrics::TransportMetrics;

/// Internal transport hooks bundle.
///
/// This keeps metrics/event accounting out of the listener/connection loop so
/// the main runtime path can evolve without re-threading instrumentation.
#[derive(Debug, Clone, Default)]
pub(crate) struct TransportHooks {
    metrics: Arc<TransportMetrics>,
}

impl TransportHooks {
    pub(crate) fn new() -> Self {
        Self {
            metrics: Arc::new(TransportMetrics::new()),
        }
    }

    pub(crate) fn metrics(&self) -> &Arc<TransportMetrics> {
        &self.metrics
    }

    pub(crate) fn record_connection(&self) {
        self.metrics.record_connection();
    }

    pub(crate) fn record_disconnection(&self) {
        self.metrics.record_disconnection();
    }

    pub(crate) fn record_rejection(&self) {
        self.metrics.record_rejection();
    }

    pub(crate) fn record_message_received(&self, bytes: usize) {
        self.metrics.record_message_received(bytes);
    }

    pub(crate) fn record_message_sent(&self, bytes: usize) {
        self.metrics.record_message_sent(bytes);
    }

    pub(crate) fn record_error(&self) {
        self.metrics.record_error();
    }
}
