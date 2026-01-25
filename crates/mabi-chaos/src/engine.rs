//! Chaos engine for orchestrating fault injection.

use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::context::FaultContext;
use crate::error::{ChaosError, ChaosResult};
use crate::fault::{BoxedFault, Fault, FaultBehavior, FaultStatistics};
use crate::registry::{FaultFilter, FaultRegistry};
use crate::scheduler::{ChaosEvent, ChaosSchedule, ChaosScheduler, ChaosType};

// =============================================================================
// Engine State
// =============================================================================

/// State of the chaos engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EngineState {
    /// Engine is stopped.
    #[default]
    Stopped,

    /// Engine is running.
    Running,

    /// Engine is paused.
    Paused,
}

// =============================================================================
// Engine Event
// =============================================================================

/// Events emitted by the chaos engine.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// Engine started.
    Started,

    /// Engine stopped.
    Stopped,

    /// Engine paused.
    Paused,

    /// Engine resumed.
    Resumed,

    /// Fault registered.
    FaultRegistered { fault_id: String },

    /// Fault unregistered.
    FaultUnregistered { fault_id: String },

    /// Fault activated.
    FaultActivated { fault_id: String, target: String },

    /// Fault deactivated.
    FaultDeactivated { fault_id: String, target: String },

    /// Fault applied.
    FaultApplied {
        fault_id: String,
        target: String,
        behavior: FaultBehavior,
    },

    /// Schedule event.
    ScheduleEvent(ChaosEvent),

    /// Error occurred.
    Error { message: String },
}

// =============================================================================
// Chaos Engine
// =============================================================================

/// Central orchestrator for chaos engineering.
///
/// The `ChaosEngine` manages fault registration, activation, and application.
/// It coordinates between the fault registry and scheduler.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::prelude::*;
///
/// // Create engine
/// let engine = ChaosEngine::builder()
///     .add_fault("latency", latency_fault)
///     .add_fault("loss", loss_fault)
///     .build();
///
/// // Start the engine
/// engine.start().await?;
///
/// // Enable faults for targets
/// engine.enable("latency", "device-*").await?;
///
/// // Process a request through chaos
/// let behavior = engine.process(&mut context).await?;
/// ```
#[derive(Debug)]
pub struct ChaosEngine {
    /// Fault registry.
    registry: Arc<FaultRegistry>,

    /// Optional scheduler.
    scheduler: Arc<RwLock<Option<ChaosScheduler>>>,

    /// Engine state.
    state: Arc<RwLock<EngineState>>,

    /// Event broadcaster.
    event_tx: broadcast::Sender<EngineEvent>,

    /// Start time.
    started_at: Arc<RwLock<Option<Instant>>>,

    /// Whether to continue on fault errors.
    continue_on_error: bool,
}

impl ChaosEngine {
    /// Create a new chaos engine.
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        Self {
            registry: Arc::new(FaultRegistry::new()),
            scheduler: Arc::new(RwLock::new(None)),
            state: Arc::new(RwLock::new(EngineState::Stopped)),
            event_tx,
            started_at: Arc::new(RwLock::new(None)),
            continue_on_error: true,
        }
    }

    /// Create a builder.
    pub fn builder() -> ChaosEngineBuilder {
        ChaosEngineBuilder::new()
    }

    /// Get the fault registry.
    pub fn registry(&self) -> &FaultRegistry {
        &self.registry
    }

    /// Get current state.
    pub fn state(&self) -> EngineState {
        *self.state.read()
    }

    /// Check if running.
    pub fn is_running(&self) -> bool {
        *self.state.read() == EngineState::Running
    }

    /// Subscribe to events.
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.event_tx.subscribe()
    }

    /// Start the engine.
    pub async fn start(&self) -> ChaosResult<()> {
        let mut state = self.state.write();
        if *state == EngineState::Running {
            return Err(ChaosError::EngineAlreadyRunning);
        }

        *state = EngineState::Running;
        *self.started_at.write() = Some(Instant::now());

        // Start scheduler if present
        if let Some(ref mut scheduler) = *self.scheduler.write() {
            scheduler.start();
        }

        let _ = self.event_tx.send(EngineEvent::Started);
        tracing::info!("Chaos engine started");

        Ok(())
    }

    /// Stop the engine.
    pub async fn stop(&self) -> ChaosResult<()> {
        let mut state = self.state.write();
        if *state == EngineState::Stopped {
            return Err(ChaosError::EngineNotRunning);
        }

        // Stop scheduler
        if let Some(ref mut scheduler) = *self.scheduler.write() {
            scheduler.stop();
        }

        *state = EngineState::Stopped;
        *self.started_at.write() = None;

        // Reset all faults
        self.registry.reset_all();

        let _ = self.event_tx.send(EngineEvent::Stopped);
        tracing::info!("Chaos engine stopped");

        Ok(())
    }

    /// Pause the engine.
    pub fn pause(&self) -> ChaosResult<()> {
        let mut state = self.state.write();
        if *state != EngineState::Running {
            return Err(ChaosError::EngineNotRunning);
        }

        *state = EngineState::Paused;
        let _ = self.event_tx.send(EngineEvent::Paused);
        tracing::info!("Chaos engine paused");

        Ok(())
    }

    /// Resume the engine.
    pub fn resume(&self) -> ChaosResult<()> {
        let mut state = self.state.write();
        if *state != EngineState::Paused {
            return Err(ChaosError::InvalidStateTransition {
                from: format!("{:?}", *state),
                to: "Running".to_string(),
            });
        }

        *state = EngineState::Running;
        let _ = self.event_tx.send(EngineEvent::Resumed);
        tracing::info!("Chaos engine resumed");

        Ok(())
    }

    /// Register a fault.
    pub fn register(&self, id: impl Into<String>, fault: BoxedFault) -> ChaosResult<()> {
        let id = id.into();
        self.registry.register(&id, fault)?;
        let _ = self.event_tx.send(EngineEvent::FaultRegistered {
            fault_id: id,
        });
        Ok(())
    }

    /// Unregister a fault.
    pub fn unregister(&self, id: &str) -> ChaosResult<BoxedFault> {
        let fault = self.registry.unregister(id)?;
        let _ = self.event_tx.send(EngineEvent::FaultUnregistered {
            fault_id: id.to_string(),
        });
        Ok(fault)
    }

    /// Enable a fault for a target.
    pub async fn enable(&self, fault_id: &str, target: impl Into<String>) -> ChaosResult<()> {
        let target = target.into();
        self.registry.activate(fault_id, &target)?;
        let _ = self.event_tx.send(EngineEvent::FaultActivated {
            fault_id: fault_id.to_string(),
            target,
        });
        Ok(())
    }

    /// Disable a fault for a target.
    pub async fn disable(&self, fault_id: &str, target: &str) -> ChaosResult<()> {
        self.registry.deactivate(fault_id, target)?;
        let _ = self.event_tx.send(EngineEvent::FaultDeactivated {
            fault_id: fault_id.to_string(),
            target: target.to_string(),
        });
        Ok(())
    }

    /// Enable a fault globally.
    pub async fn enable_globally(&self, fault_id: &str) -> ChaosResult<()> {
        self.registry.activate_globally(fault_id)?;
        let _ = self.event_tx.send(EngineEvent::FaultActivated {
            fault_id: fault_id.to_string(),
            target: "*".to_string(),
        });
        Ok(())
    }

    /// Disable a fault globally.
    pub async fn disable_globally(&self, fault_id: &str) -> ChaosResult<()> {
        self.registry.deactivate_globally(fault_id)?;
        let _ = self.event_tx.send(EngineEvent::FaultDeactivated {
            fault_id: fault_id.to_string(),
            target: "*".to_string(),
        });
        Ok(())
    }

    /// Set a schedule.
    pub fn set_schedule(&self, schedule: ChaosSchedule) {
        *self.scheduler.write() = Some(ChaosScheduler::new(schedule));
    }

    /// Process scheduler tick.
    pub fn tick_scheduler(&self) -> Vec<ChaosEvent> {
        if let Some(ref mut scheduler) = *self.scheduler.write() {
            let events = scheduler.tick();
            for event in &events {
                let _ = self.event_tx.send(EngineEvent::ScheduleEvent(event.clone()));
            }
            events
        } else {
            Vec::new()
        }
    }

    /// Process a request through chaos injection.
    ///
    /// This is the main entry point for applying faults.
    pub async fn process(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        // Check state
        if *self.state.read() != EngineState::Running {
            return Ok(FaultBehavior::Continue);
        }

        // Get active faults for this target
        let active_fault_ids = self.registry.active_for(ctx.target.identifier());

        let mut final_behavior = FaultBehavior::Continue;

        for fault_id in active_fault_ids {
            if ctx.skip_remaining {
                break;
            }

            if let Some(entry) = self.registry.get(&fault_id) {
                // Check if should activate
                let should_activate = match entry.fault.should_activate(ctx).await {
                    Ok(v) => v,
                    Err(e) => {
                        if !self.continue_on_error {
                            return Err(e);
                        }
                        let _ = self.event_tx.send(EngineEvent::Error {
                            message: e.to_string(),
                        });
                        continue;
                    }
                };

                if should_activate {
                    // Apply the fault
                    let behavior = match entry.fault.before_operation(ctx).await {
                        Ok(b) => b,
                        Err(e) => {
                            if !self.continue_on_error {
                                return Err(e);
                            }
                            let _ = self.event_tx.send(EngineEvent::Error {
                                message: e.to_string(),
                            });
                            continue;
                        }
                    };

                    // Emit event
                    let _ = self.event_tx.send(EngineEvent::FaultApplied {
                        fault_id: fault_id.clone(),
                        target: ctx.target.device_id.clone(),
                        behavior: behavior.clone(),
                    });

                    // Determine final behavior (most severe wins)
                    final_behavior = merge_behaviors(final_behavior, behavior);

                    // Check if should stop processing
                    if !final_behavior.should_proceed() {
                        break;
                    }
                }
            }
        }

        Ok(final_behavior)
    }

    /// Process after operation (for response modification).
    pub async fn process_after(&self, ctx: &mut FaultContext) -> ChaosResult<()> {
        if *self.state.read() != EngineState::Running {
            return Ok(());
        }

        ctx.transition_to_after();

        let active_fault_ids = self.registry.active_for(ctx.target.identifier());

        for fault_id in active_fault_ids {
            if let Some(entry) = self.registry.get(&fault_id) {
                if let Err(e) = entry.fault.after_operation(ctx).await {
                    if !self.continue_on_error {
                        return Err(e);
                    }
                    let _ = self.event_tx.send(EngineEvent::Error {
                        message: e.to_string(),
                    });
                }
            }
        }

        Ok(())
    }

    /// Get statistics for all faults.
    pub fn statistics(&self) -> Vec<(String, FaultStatistics)> {
        self.registry
            .ids()
            .into_iter()
            .filter_map(|id| {
                self.registry.get(&id).map(|entry| (id, entry.fault.statistics()))
            })
            .collect()
    }
}

impl Default for ChaosEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Merge two fault behaviors, keeping the most severe.
fn merge_behaviors(a: FaultBehavior, b: FaultBehavior) -> FaultBehavior {
    use FaultBehavior::*;

    match (&a, &b) {
        // Abort always wins
        (Abort { .. }, _) => a,
        (_, Abort { .. }) => b,

        // Skip beats delay and continue
        (Skip, _) => a,
        (_, Skip) => b,

        // ReturnError beats delay and continue
        (ReturnError { .. }, _) => a,
        (_, ReturnError { .. }) => b,

        // Delay - take longer delay
        (Delay { duration_ms: d1 }, Delay { duration_ms: d2 }) => {
            Delay {
                duration_ms: (*d1).max(*d2),
            }
        }
        (Delay { .. }, _) => a,
        (_, Delay { .. }) => b,

        // Modify beats continue
        (Modify, _) => a,
        (_, Modify) => b,

        // Continue is default
        _ => Continue,
    }
}

// =============================================================================
// Builder
// =============================================================================

/// Builder for chaos engine.
#[derive(Debug, Default)]
pub struct ChaosEngineBuilder {
    faults: Vec<(String, BoxedFault)>,
    schedule: Option<ChaosSchedule>,
    continue_on_error: bool,
}

impl ChaosEngineBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            faults: Vec::new(),
            schedule: None,
            continue_on_error: true,
        }
    }

    /// Add a fault.
    pub fn add_fault(mut self, id: impl Into<String>, fault: impl Fault + 'static) -> Self {
        self.faults.push((id.into(), Box::new(fault)));
        self
    }

    /// Add a boxed fault.
    pub fn add_boxed_fault(mut self, id: impl Into<String>, fault: BoxedFault) -> Self {
        self.faults.push((id.into(), fault));
        self
    }

    /// Set a schedule.
    pub fn schedule(mut self, schedule: ChaosSchedule) -> Self {
        self.schedule = Some(schedule);
        self
    }

    /// Set whether to continue on error.
    pub fn continue_on_error(mut self, continue_on_error: bool) -> Self {
        self.continue_on_error = continue_on_error;
        self
    }

    /// Build the engine.
    pub fn build(self) -> ChaosEngine {
        let (event_tx, _) = broadcast::channel(1024);
        let registry = Arc::new(FaultRegistry::new());

        for (id, fault) in self.faults {
            let _ = registry.register(id, fault);
        }

        let scheduler = self.schedule.map(ChaosScheduler::new);

        ChaosEngine {
            registry,
            scheduler: Arc::new(RwLock::new(scheduler)),
            state: Arc::new(RwLock::new(EngineState::Stopped)),
            event_tx,
            started_at: Arc::new(RwLock::new(None)),
            continue_on_error: self.continue_on_error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{OperationType, TargetInfo};
    use crate::network::NetworkLatencyFault;
    use mabi_core::Protocol;

    #[tokio::test]
    async fn test_engine_lifecycle() {
        let engine = ChaosEngine::new();

        assert_eq!(engine.state(), EngineState::Stopped);

        engine.start().await.unwrap();
        assert_eq!(engine.state(), EngineState::Running);

        engine.pause().unwrap();
        assert_eq!(engine.state(), EngineState::Paused);

        engine.resume().unwrap();
        assert_eq!(engine.state(), EngineState::Running);

        engine.stop().await.unwrap();
        assert_eq!(engine.state(), EngineState::Stopped);
    }

    #[tokio::test]
    async fn test_fault_registration() {
        let engine = ChaosEngine::new();

        let fault = NetworkLatencyFault::builder()
            .id("test")
            .base_ms(100)
            .build();

        engine.register("test", Box::new(fault)).unwrap();
        assert!(engine.registry().contains("test"));

        engine.unregister("test").unwrap();
        assert!(!engine.registry().contains("test"));
    }

    #[tokio::test]
    async fn test_fault_activation() {
        let engine = ChaosEngine::new();

        let fault = NetworkLatencyFault::builder()
            .id("test")
            .base_ms(10)
            .build();

        engine.register("test", Box::new(fault)).unwrap();
        engine.start().await.unwrap();
        engine.enable("test", "device-001").await.unwrap();

        let mut ctx = FaultContext::new(
            TargetInfo::device("device-001"),
            OperationType::Read {
                point_id: "temp".to_string(),
            },
            Protocol::ModbusTcp,
        );

        let behavior = engine.process(&mut ctx).await.unwrap();
        assert!(ctx.was_affected());
    }

    #[test]
    fn test_behavior_merging() {
        use FaultBehavior::*;

        // Abort wins
        assert!(matches!(
            merge_behaviors(Abort { error: "a".into() }, Continue),
            Abort { .. }
        ));

        // Skip beats delay
        assert!(matches!(
            merge_behaviors(Delay { duration_ms: 100 }, Skip),
            Skip
        ));

        // Longer delay wins
        match merge_behaviors(Delay { duration_ms: 100 }, Delay { duration_ms: 200 }) {
            Delay { duration_ms } => assert_eq!(duration_ms, 200),
            _ => panic!("Expected Delay"),
        }
    }
}
