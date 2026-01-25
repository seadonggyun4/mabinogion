//! Application state.

use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;
use tokio::sync::broadcast;

use mabi_chaos::ChaosScheduler;
use mabi_core::engine::SimulatorEngine;

/// Application state shared across handlers.
pub struct AppState {
    /// Simulator engine.
    pub engine: Arc<RwLock<SimulatorEngine>>,

    /// Chaos manager (optional).
    pub chaos_scheduler: Option<Arc<RwLock<ChaosScheduler>>>,

    /// Server start time.
    pub start_time: Instant,

    /// Event broadcaster for WebSocket.
    pub event_tx: broadcast::Sender<String>,
}

impl AppState {
    /// Create new app state.
    pub fn new(engine: SimulatorEngine) -> Self {
        let (event_tx, _) = broadcast::channel(1000);

        Self {
            engine: Arc::new(RwLock::new(engine)),
            chaos_scheduler: None,
            start_time: Instant::now(),
            event_tx,
        }
    }

    /// Set chaos scheduler.
    pub fn with_chaos_scheduler(mut self, scheduler: ChaosScheduler) -> Self {
        self.chaos_scheduler = Some(Arc::new(RwLock::new(scheduler)));
        self
    }

    /// Get uptime.
    pub fn uptime(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }
}
