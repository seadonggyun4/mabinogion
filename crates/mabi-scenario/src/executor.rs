//! Scenario Executor - Bridge between scenario player and devices.
//!
//! The executor coordinates scenario playback with actual device writes,
//! enabling generated values to flow through to protocol simulators.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    ScenarioExecutor                          │
//! │                                                             │
//! │  ┌─────────────────────┐  ┌─────────────────────────────┐  │
//! │  │  ScenarioPlayer     │  │      DeviceRegistry         │  │
//! │  │  (Value Generation) │  │      (Device Handles)       │  │
//! │  └──────────┬──────────┘  └────────────┬────────────────┘  │
//! │             │                          │                   │
//! │             │    ValueUpdate           │                   │
//! │             └──────────────────────────┘                   │
//! │                          │                                 │
//! │                          ▼                                 │
//! │             ┌────────────────────────┐                     │
//! │             │   Device.write()       │                     │
//! │             │   (Protocol Write)     │                     │
//! │             └────────────────────────┘                     │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Features
//!
//! - **Device Registry**: Manages device handles for scenario execution
//! - **Value Routing**: Routes generated values to correct devices
//! - **Async Execution**: Non-blocking scenario execution with tokio
//! - **Error Recovery**: Graceful handling of device write failures
//! - **Metrics**: Execution statistics and performance tracking

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use mabi_core::device::DeviceHandle;
use mabi_runtime::{CoreDevicePort, DeviceRegistry as RuntimeDeviceRegistry, DynDevicePort};

use crate::player::{PlayerConfig, PlayerState, ScenarioPlayer, ValueUpdate};
use crate::schema::Scenario;
use crate::{ScenarioError, ScenarioResult};

// ============================================================================
// Executor Configuration
// ============================================================================

/// Executor configuration.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Write timeout per device.
    pub write_timeout: Duration,
    /// Continue on write errors (don't stop scenario).
    pub continue_on_error: bool,
    /// Maximum concurrent writes.
    pub max_concurrent_writes: usize,
    /// Enable metrics collection.
    pub collect_metrics: bool,
    /// Buffer size for value updates.
    pub update_buffer_size: usize,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            write_timeout: Duration::from_secs(5),
            continue_on_error: true,
            max_concurrent_writes: 100,
            collect_metrics: true,
            update_buffer_size: 10_000,
        }
    }
}

impl ExecutorConfig {
    /// Create with strict error handling (stop on first error).
    pub fn strict() -> Self {
        Self {
            continue_on_error: false,
            ..Default::default()
        }
    }

    /// Set write timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.write_timeout = timeout;
        self
    }
}

// ============================================================================
// Execution Metrics
// ============================================================================

/// Execution metrics.
#[derive(Debug, Clone, Default)]
pub struct ExecutorMetrics {
    /// Total values generated.
    pub values_generated: u64,
    /// Total writes attempted.
    pub writes_attempted: u64,
    /// Successful writes.
    pub writes_successful: u64,
    /// Failed writes.
    pub writes_failed: u64,
    /// Total execution time.
    pub execution_time: Duration,
    /// Average write latency.
    pub avg_write_latency_us: u64,
    /// Device-specific statistics.
    pub device_stats: HashMap<String, DeviceWriteStats>,
}

/// Per-device write statistics.
#[derive(Debug, Clone, Default)]
pub struct DeviceWriteStats {
    pub writes: u64,
    pub errors: u64,
    pub last_value: Option<f64>,
    pub last_write_time: Option<Instant>,
}

impl ExecutorMetrics {
    /// Record a successful write.
    pub fn record_write_success(&mut self, device_id: &str, latency_us: u64) {
        self.writes_attempted += 1;
        self.writes_successful += 1;

        // Update average latency
        if self.writes_successful == 1 {
            self.avg_write_latency_us = latency_us;
        } else {
            self.avg_write_latency_us = (self.avg_write_latency_us * (self.writes_successful - 1)
                + latency_us)
                / self.writes_successful;
        }

        let stats = self.device_stats.entry(device_id.to_string()).or_default();
        stats.writes += 1;
        stats.last_write_time = Some(Instant::now());
    }

    /// Record a write failure.
    pub fn record_write_failure(&mut self, device_id: &str) {
        self.writes_attempted += 1;
        self.writes_failed += 1;

        let stats = self.device_stats.entry(device_id.to_string()).or_default();
        stats.errors += 1;
    }

    /// Get success rate as percentage.
    pub fn success_rate(&self) -> f64 {
        if self.writes_attempted == 0 {
            100.0
        } else {
            (self.writes_successful as f64 / self.writes_attempted as f64) * 100.0
        }
    }
}

/// Shared device registry used by the scenario control plane.
pub type DeviceRegistry = RuntimeDeviceRegistry;

// ============================================================================
// Executor State
// ============================================================================

/// Executor state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorState {
    /// Not started.
    Idle,
    /// Running scenario.
    Running,
    /// Paused.
    Paused,
    /// Completed successfully.
    Completed,
    /// Stopped due to error.
    Error,
}

// ============================================================================
// Scenario Executor
// ============================================================================

/// Scenario executor that bridges scenario player with devices.
pub struct ScenarioExecutor {
    config: ExecutorConfig,
    scenario: Scenario,
    player: ScenarioPlayer,
    devices: DeviceRegistry,
    state: ExecutorState,
    metrics: Arc<parking_lot::RwLock<ExecutorMetrics>>,
    start_time: Option<Instant>,
    event_tx: broadcast::Sender<ExecutorEvent>,
}

/// Executor events.
#[derive(Debug, Clone)]
pub enum ExecutorEvent {
    /// Scenario started.
    Started { scenario_name: String },
    /// Value written to device.
    ValueWritten {
        device_id: String,
        point_id: String,
        value: f64,
    },
    /// Write error occurred.
    WriteError {
        device_id: String,
        point_id: String,
        error: String,
    },
    /// Scenario paused.
    Paused,
    /// Scenario resumed.
    Resumed,
    /// Scenario completed.
    Completed { metrics: ExecutorMetrics },
    /// Scenario stopped due to error.
    Error { message: String },
}

impl ScenarioExecutor {
    /// Create a new executor.
    pub fn new(scenario: Scenario, config: ExecutorConfig) -> Self {
        Self::new_with_player_config(scenario, config, PlayerConfig::default())
    }

    /// Create a new executor with an explicit player configuration.
    pub fn new_with_player_config(
        scenario: Scenario,
        config: ExecutorConfig,
        player_config: PlayerConfig,
    ) -> Self {
        let player = ScenarioPlayer::new(scenario.clone(), player_config);
        let (event_tx, _) = broadcast::channel(config.update_buffer_size);

        Self {
            config,
            scenario,
            player,
            devices: DeviceRegistry::new(),
            state: ExecutorState::Idle,
            metrics: Arc::new(parking_lot::RwLock::new(ExecutorMetrics::default())),
            start_time: None,
            event_tx,
        }
    }

    /// Get the executor configuration.
    pub fn config(&self) -> &ExecutorConfig {
        &self.config
    }

    /// Get the scenario.
    pub fn scenario(&self) -> &Scenario {
        &self.scenario
    }

    /// Get current state.
    pub fn state(&self) -> ExecutorState {
        self.state
    }

    /// Get metrics.
    pub fn metrics(&self) -> ExecutorMetrics {
        self.metrics.read().clone()
    }

    /// Subscribe to executor events.
    pub fn subscribe(&self) -> broadcast::Receiver<ExecutorEvent> {
        self.event_tx.subscribe()
    }

    /// Returns the shared stop signal used by external controllers.
    pub fn stop_signal(&self) -> Arc<AtomicBool> {
        self.player.stop_signal()
    }

    /// Requests a cooperative stop without needing mutable access.
    pub fn request_stop(&self) {
        self.player.stop_signal().store(true, Ordering::SeqCst);
    }

    /// Register a device for the scenario.
    pub fn register_device(&self, device_id: impl Into<String>, port: DynDevicePort) {
        self.devices.register(device_id, port);
    }

    /// Register a legacy core device handle for the scenario.
    pub fn register_device_handle(&self, device_id: impl Into<String>, handle: DeviceHandle) {
        self.devices
            .register(device_id, CoreDevicePort::new(handle).into_shared());
    }

    /// Get the device registry.
    pub fn devices(&self) -> DeviceRegistry {
        self.devices.clone()
    }

    /// Validate that all required devices are registered.
    pub fn validate_devices(&self) -> ScenarioResult<()> {
        let mut missing = Vec::new();

        for point in &self.scenario.points {
            if !self.devices.contains(&point.device_id) {
                if !missing.contains(&point.device_id) {
                    missing.push(point.device_id.clone());
                }
            }
        }

        if !missing.is_empty() {
            return Err(ScenarioError::Player(format!(
                "Missing devices: {}",
                missing.join(", ")
            )));
        }

        Ok(())
    }

    /// Start the executor.
    pub async fn start(&mut self) -> ScenarioResult<()> {
        if self.state == ExecutorState::Running {
            return Ok(());
        }

        self.validate_devices()?;

        self.state = ExecutorState::Running;
        self.start_time = Some(Instant::now());
        self.player.start();

        info!(scenario = %self.scenario.name, "Executor started");

        let _ = self.event_tx.send(ExecutorEvent::Started {
            scenario_name: self.scenario.name.clone(),
        });

        Ok(())
    }

    /// Pause the executor.
    pub fn pause(&mut self) {
        if self.state == ExecutorState::Running {
            self.state = ExecutorState::Paused;
            self.player.pause();
            let _ = self.event_tx.send(ExecutorEvent::Paused);
            info!(scenario = %self.scenario.name, "Executor paused");
        }
    }

    /// Resume the executor.
    pub fn resume(&mut self) {
        if self.state == ExecutorState::Paused {
            self.state = ExecutorState::Running;
            self.player.start();
            let _ = self.event_tx.send(ExecutorEvent::Resumed);
            info!(scenario = %self.scenario.name, "Executor resumed");
        }
    }

    /// Stop the executor.
    pub fn stop(&mut self) {
        self.player.stop();
        self.state = ExecutorState::Idle;

        if let Some(start) = self.start_time {
            self.metrics.write().execution_time = start.elapsed();
        }

        info!(scenario = %self.scenario.name, "Executor stopped");
    }

    /// Run the executor to completion.
    pub async fn run(&mut self) -> ScenarioResult<()> {
        self.start().await?;

        // Subscribe to player updates
        let update_rx = self.player.subscribe();
        let devices = self.devices.clone();
        let metrics = Arc::clone(&self.metrics);
        let event_tx = self.event_tx.clone();
        let config = self.config.clone();
        let stop_flag = self.player.stop_signal();

        // Spawn value writer task
        let writer_handle = tokio::spawn(async move {
            Self::value_writer_loop(update_rx, devices, metrics, event_tx, config, stop_flag).await
        });

        // Run the player
        let player_result = self.player.run().await;

        // Wait for the writer to finish so we don't detach a blocked task.
        let writer_result = match tokio::time::timeout(Duration::from_secs(5), writer_handle).await
        {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => Err(ScenarioError::Player(format!("Writer task failed: {}", e))),
            Err(_) => Err(ScenarioError::Player(
                "Writer task did not shut down within 5 seconds".into(),
            )),
        };

        let result = match (player_result, writer_result) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        };

        // Update state based on the combined player/writer result
        match &result {
            Ok(()) => {
                if self.player.state() == PlayerState::Completed {
                    self.state = ExecutorState::Completed;
                    let metrics = self.metrics();
                    let _ = self.event_tx.send(ExecutorEvent::Completed { metrics });
                    info!(scenario = %self.scenario.name, "Executor completed");
                } else {
                    self.state = ExecutorState::Idle;
                }
            }
            Err(e) => {
                self.state = ExecutorState::Error;
                let _ = self.event_tx.send(ExecutorEvent::Error {
                    message: e.to_string(),
                });
                error!(scenario = %self.scenario.name, error = %e, "Executor error");
            }
        }

        if let Some(start) = self.start_time {
            self.metrics.write().execution_time = start.elapsed();
        }

        result
    }

    /// Internal value writer loop.
    async fn value_writer_loop(
        mut update_rx: broadcast::Receiver<ValueUpdate>,
        devices: DeviceRegistry,
        metrics: Arc<parking_lot::RwLock<ExecutorMetrics>>,
        event_tx: broadcast::Sender<ExecutorEvent>,
        config: ExecutorConfig,
        stop_flag: Arc<AtomicBool>,
    ) -> ScenarioResult<()> {
        loop {
            let update = tokio::select! {
                result = update_rx.recv() => match result {
                    Ok(update) => update,
                    Err(broadcast::error::RecvError::Closed) => break Ok(()),
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!(skipped, "Scenario writer lagged behind the player");
                        continue;
                    }
                },
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    if stop_flag.load(Ordering::SeqCst) {
                        break Ok(());
                    }
                    continue;
                }
            };

            metrics.write().values_generated += 1;

            let device_id = &update.point_id.device_id;
            let point_id = &update.point_id.point_id;

            // Get device handle
            let device_opt = devices.get(device_id);

            let Some(device) = device_opt else {
                warn!(device_id, "Device not found for value update");
                continue;
            };

            // Write value with timeout
            let write_start = Instant::now();
            let write_result: Result<crate::ScenarioResult<()>, _> =
                tokio::time::timeout(config.write_timeout, async {
                    device
                        .write(point_id, update.value.clone())
                        .await
                        .map_err(|e| crate::ScenarioError::Core(e))
                })
                .await;

            let latency_us = write_start.elapsed().as_micros() as u64;

            match write_result {
                Ok(Ok(())) => {
                    metrics.write().record_write_success(device_id, latency_us);

                    if let Some(value) = update.value.as_f64() {
                        let _ = event_tx.send(ExecutorEvent::ValueWritten {
                            device_id: device_id.to_string(),
                            point_id: point_id.to_string(),
                            value,
                        });
                    }

                    debug!(device_id, point_id, ?update.value, "Value written");
                }
                Ok(Err(e)) => {
                    metrics.write().record_write_failure(device_id);

                    let _ = event_tx.send(ExecutorEvent::WriteError {
                        device_id: device_id.to_string(),
                        point_id: point_id.to_string(),
                        error: e.to_string(),
                    });

                    warn!(device_id, point_id, error = %e, "Write failed");

                    if !config.continue_on_error {
                        stop_flag.store(true, Ordering::SeqCst);
                        return Err(ScenarioError::Player(format!(
                            "Write failed for {}.{}: {}",
                            device_id, point_id, e
                        )));
                    }
                }
                Err(_) => {
                    metrics.write().record_write_failure(device_id);

                    let _ = event_tx.send(ExecutorEvent::WriteError {
                        device_id: device_id.to_string(),
                        point_id: point_id.to_string(),
                        error: "Write timeout".to_string(),
                    });

                    warn!(device_id, point_id, "Write timeout");

                    if !config.continue_on_error {
                        stop_flag.store(true, Ordering::SeqCst);
                        return Err(ScenarioError::Player(format!(
                            "Write timeout for {}.{}",
                            device_id, point_id
                        )));
                    }
                }
            }
        }
    }
}

// ============================================================================
// Executor Builder
// ============================================================================

/// Builder for creating scenario executors.
pub struct ExecutorBuilder {
    scenario: Option<Scenario>,
    config: ExecutorConfig,
    devices: DeviceRegistry,
}

impl ExecutorBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            scenario: None,
            config: ExecutorConfig::default(),
            devices: DeviceRegistry::new(),
        }
    }

    /// Set the scenario.
    pub fn scenario(mut self, scenario: Scenario) -> Self {
        self.scenario = Some(scenario);
        self
    }

    /// Set the configuration.
    pub fn config(mut self, config: ExecutorConfig) -> Self {
        self.config = config;
        self
    }

    /// Seed the builder with an existing runtime device registry.
    pub fn runtime_devices(mut self, devices: DeviceRegistry) -> Self {
        self.devices = devices;
        self
    }

    /// Register a device.
    pub fn device(self, device_id: impl Into<String>, handle: DeviceHandle) -> Self {
        self.devices
            .register(device_id, CoreDevicePort::new(handle).into_shared());
        self
    }

    /// Build the executor.
    pub fn build(self) -> ScenarioResult<ScenarioExecutor> {
        let scenario = self
            .scenario
            .ok_or_else(|| ScenarioError::Player("Scenario is required".into()))?;

        let mut executor = ScenarioExecutor::new(scenario, self.config);

        // Transfer devices
        executor.devices = self.devices;

        Ok(executor)
    }
}

impl Default for ExecutorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_executor_config() {
        let config = ExecutorConfig::default();
        assert!(config.continue_on_error);
        assert_eq!(config.max_concurrent_writes, 100);

        let strict = ExecutorConfig::strict();
        assert!(!strict.continue_on_error);
    }

    #[test]
    fn test_executor_metrics() {
        let mut metrics = ExecutorMetrics::default();

        metrics.record_write_success("device-1", 100);
        metrics.record_write_success("device-1", 200);
        metrics.record_write_failure("device-2");

        assert_eq!(metrics.writes_successful, 2);
        assert_eq!(metrics.writes_failed, 1);
        assert_eq!(metrics.avg_write_latency_us, 150);
        assert!((metrics.success_rate() - 66.666).abs() < 1.0);
    }

    #[test]
    fn test_device_registry() {
        let registry = DeviceRegistry::new();
        assert!(registry.is_empty());

        // Note: Can't test with actual DeviceHandle without a real device
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_executor_builder() {
        let result = ExecutorBuilder::new().build();
        assert!(result.is_err()); // No scenario set

        let scenario = Scenario {
            name: "Test".to_string(),
            ..Default::default()
        };

        let result = ExecutorBuilder::new().scenario(scenario).build();
        assert!(result.is_ok());
    }
}
