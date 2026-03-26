//! Simulator engine - orchestrates all devices and protocols.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{debug, error, info, instrument};

use crate::config::EngineConfig;
use crate::device::{BoxedDevice, DeviceHandle, DeviceInfo, DeviceState};
use crate::error::{Error, Result};
use crate::metrics::MetricsCollector;
use crate::protocol::Protocol;
use crate::types::DataPoint;
use crate::value::Value;

/// Engine state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineState {
    /// Engine is stopped.
    Stopped,
    /// Engine is starting.
    Starting,
    /// Engine is running.
    Running,
    /// Engine is stopping.
    Stopping,
    /// Engine has an error.
    Error,
}

/// Simulator engine that manages all virtual devices.
pub struct SimulatorEngine {
    /// Engine configuration.
    config: EngineConfig,

    /// Current state.
    state: RwLock<EngineState>,

    /// All managed devices.
    devices: DashMap<String, DeviceHandle>,

    /// Devices by protocol.
    devices_by_protocol: DashMap<Protocol, Vec<String>>,

    /// Metrics collector.
    metrics: MetricsCollector,

    /// Start time.
    start_time: RwLock<Option<Instant>>,

    /// Shutdown flag.
    shutdown: AtomicBool,

    /// Event broadcaster.
    event_tx: broadcast::Sender<EngineEvent>,

    /// Tick counter.
    tick_count: AtomicU64,
}

impl SimulatorEngine {
    /// Create a new simulator engine.
    pub fn new(config: EngineConfig) -> Self {
        let (event_tx, _) = broadcast::channel(10_000);

        Self {
            config,
            state: RwLock::new(EngineState::Stopped),
            devices: DashMap::new(),
            devices_by_protocol: DashMap::new(),
            metrics: MetricsCollector::new(),
            start_time: RwLock::new(None),
            shutdown: AtomicBool::new(false),
            event_tx,
            tick_count: AtomicU64::new(0),
        }
    }

    /// Get the engine configuration.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Get the current engine state.
    pub fn state(&self) -> EngineState {
        *self.state.read()
    }

    /// Get metrics collector.
    pub fn metrics(&self) -> &MetricsCollector {
        &self.metrics
    }

    /// Subscribe to engine events.
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.event_tx.subscribe()
    }

    /// Get device count.
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    /// Get total data points across all devices.
    pub fn point_count(&self) -> usize {
        self.devices
            .iter()
            .map(|d| d.value().info().point_count)
            .sum()
    }

    /// Add a device to the engine.
    #[instrument(skip(self, device), fields(device_id))]
    pub async fn add_device(&self, device: BoxedDevice) -> Result<()> {
        let device_id = device.id().to_string();
        tracing::Span::current().record("device_id", &device_id);

        // Check capacity
        if self.devices.len() >= self.config.max_devices {
            return Err(Error::capacity_exceeded(
                self.devices.len(),
                self.config.max_devices,
                "devices",
            ));
        }

        // Check if device already exists
        if self.devices.contains_key(&device_id) {
            return Err(Error::DeviceAlreadyExists { device_id });
        }

        let protocol = device.protocol();
        let handle = DeviceHandle::new(device);

        // Initialize device
        handle.initialize().await?;

        // Add to maps
        self.devices.insert(device_id.clone(), handle);
        self.devices_by_protocol
            .entry(protocol)
            .or_default()
            .push(device_id.clone());

        // Update metrics
        self.metrics.set_devices_active(self.devices.len() as i64);

        // Broadcast event
        let _ = self.event_tx.send(EngineEvent::DeviceAdded {
            device_id: device_id.clone(),
            protocol,
        });

        info!(device_id = %device_id, protocol = ?protocol, "Device added");
        Ok(())
    }

    /// Remove a device from the engine.
    #[instrument(skip(self))]
    pub async fn remove_device(&self, device_id: &str) -> Result<()> {
        let (_, handle) = self
            .devices
            .remove(device_id)
            .ok_or_else(|| Error::device_not_found(device_id))?;

        let protocol = handle.info().protocol;

        // Stop device
        handle.stop().await?;

        // Remove from protocol map
        if let Some(mut devices) = self.devices_by_protocol.get_mut(&protocol) {
            devices.retain(|id| id != device_id);
        }

        // Update metrics
        self.metrics.set_devices_active(self.devices.len() as i64);

        // Broadcast event
        let _ = self.event_tx.send(EngineEvent::DeviceRemoved {
            device_id: device_id.to_string(),
        });

        info!(device_id = %device_id, "Device removed");
        Ok(())
    }

    /// Get device by ID.
    pub fn device(&self, device_id: &str) -> Option<DeviceHandle> {
        self.devices.get(device_id).map(|d| d.value().clone())
    }

    /// List all devices.
    pub fn list_devices(&self) -> Vec<DeviceInfo> {
        self.devices.iter().map(|d| d.value().info()).collect()
    }

    /// List devices by protocol.
    pub fn list_devices_by_protocol(&self, protocol: Protocol) -> Vec<DeviceInfo> {
        if let Some(device_ids) = self.devices_by_protocol.get(&protocol) {
            device_ids
                .iter()
                .filter_map(|id| self.devices.get(id))
                .map(|d| d.value().info())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Read a data point.
    #[instrument(skip(self))]
    pub async fn read_point(&self, device_id: &str, point_id: &str) -> Result<DataPoint> {
        let device = self
            .device(device_id)
            .ok_or_else(|| Error::device_not_found(device_id))?;

        let start = Instant::now();
        let result = device.read(point_id).await;
        let duration = start.elapsed();

        let protocol = device.info().protocol.to_string();
        self.metrics
            .record_read(&protocol, result.is_ok(), duration);

        result
    }

    /// Write a data point.
    #[instrument(skip(self, value))]
    pub async fn write_point(&self, device_id: &str, point_id: &str, value: Value) -> Result<()> {
        let device = self
            .device(device_id)
            .ok_or_else(|| Error::device_not_found(device_id))?;

        let start = Instant::now();
        let result = device.write(point_id, value).await;
        let duration = start.elapsed();

        let protocol = device.info().protocol.to_string();
        self.metrics
            .record_write(&protocol, result.is_ok(), duration);

        result
    }

    /// Start the engine.
    #[instrument(skip(self))]
    pub async fn start(&self) -> Result<()> {
        {
            let mut state = self.state.write();
            if *state != EngineState::Stopped {
                return Err(Error::Engine("Engine is not stopped".into()));
            }
            *state = EngineState::Starting;
        }

        info!("Starting simulator engine");
        self.shutdown.store(false, Ordering::SeqCst);
        *self.start_time.write() = Some(Instant::now());

        // Start all devices
        for device in self.devices.iter() {
            if let Err(e) = device.value().start().await {
                error!(device_id = %device.key(), error = %e, "Failed to start device");
            }
        }

        {
            let mut state = self.state.write();
            *state = EngineState::Running;
        }

        let _ = self.event_tx.send(EngineEvent::Started);
        info!("Simulator engine started");
        Ok(())
    }

    /// Stop the engine.
    #[instrument(skip(self))]
    pub async fn stop(&self) -> Result<()> {
        {
            let mut state = self.state.write();
            if *state != EngineState::Running {
                return Err(Error::Engine("Engine is not running".into()));
            }
            *state = EngineState::Stopping;
        }

        info!("Stopping simulator engine");
        self.shutdown.store(true, Ordering::SeqCst);

        // Stop all devices
        for device in self.devices.iter() {
            if let Err(e) = device.value().stop().await {
                error!(device_id = %device.key(), error = %e, "Failed to stop device");
            }
        }

        {
            let mut state = self.state.write();
            *state = EngineState::Stopped;
        }

        let _ = self.event_tx.send(EngineEvent::Stopped);
        info!("Simulator engine stopped");
        Ok(())
    }

    /// Process one tick (update all devices).
    #[instrument(skip(self))]
    pub async fn tick(&self) -> Result<()> {
        if self.state() != EngineState::Running {
            return Ok(());
        }

        let start = Instant::now();
        let tick_num = self.tick_count.fetch_add(1, Ordering::Relaxed);

        // Process all devices in parallel
        let futures: Vec<_> = self
            .devices
            .iter()
            .map(|d| {
                let handle = d.value().clone();
                async move {
                    if let Err(e) = handle.tick().await {
                        error!(device_id = %d.key(), error = %e, "Device tick failed");
                    }
                }
            })
            .collect();

        futures::future::join_all(futures).await;

        let duration = start.elapsed();
        self.metrics.record_tick(duration);

        if tick_num % 1000 == 0 {
            debug!(
                tick = tick_num,
                devices = self.devices.len(),
                duration_ms = duration.as_millis(),
                "Engine tick"
            );
        }

        Ok(())
    }

    /// Run the engine (blocking).
    #[instrument(skip(self))]
    pub async fn run(&self) -> Result<()> {
        self.start().await?;

        let tick_interval = self.config.tick_interval();

        while !self.shutdown.load(Ordering::SeqCst) {
            let tick_start = Instant::now();

            self.tick().await?;

            let tick_duration = tick_start.elapsed();
            if tick_duration < tick_interval {
                tokio::time::sleep(tick_interval - tick_duration).await;
            }
        }

        self.stop().await?;
        Ok(())
    }

    /// Get uptime.
    pub fn uptime(&self) -> Option<Duration> {
        self.start_time.read().map(|t| t.elapsed())
    }

    /// Get tick count.
    pub fn tick_count(&self) -> u64 {
        self.tick_count.load(Ordering::Relaxed)
    }

    /// Check if shutdown is requested.
    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Request shutdown.
    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

impl Drop for SimulatorEngine {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

/// Engine events.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// Engine started.
    Started,
    /// Engine stopped.
    Stopped,
    /// Device added.
    DeviceAdded {
        device_id: String,
        protocol: Protocol,
    },
    /// Device removed.
    DeviceRemoved { device_id: String },
    /// Device state changed.
    DeviceStateChanged {
        device_id: String,
        old_state: DeviceState,
        new_state: DeviceState,
    },
    /// Error occurred.
    Error { message: String },
}

/// Builder for creating SimulatorEngine with fluent API.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_core::engine::{SimulatorEngineBuilder, SimulatorEngine};
///
/// let engine = SimulatorEngineBuilder::new()
///     .name("HVAC Simulator")
///     .max_devices(10_000)
///     .tick_interval(Duration::from_millis(50))
///     .enable_metrics(true)
///     .build();
/// ```
#[derive(Default)]
pub struct SimulatorEngineBuilder {
    config: EngineConfig,
}

impl SimulatorEngineBuilder {
    /// Create a new engine builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the engine name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.config.name = name.into();
        self
    }

    /// Set the maximum number of devices.
    pub fn max_devices(mut self, max: usize) -> Self {
        self.config.max_devices = max;
        self
    }

    /// Set the maximum number of data points.
    pub fn max_points(mut self, max: usize) -> Self {
        self.config.max_points = max;
        self
    }

    /// Set the tick interval.
    pub fn tick_interval(mut self, interval: Duration) -> Self {
        self.config.tick_interval_ms = interval.as_millis() as u64;
        self
    }

    /// Set the tick interval in milliseconds.
    pub fn tick_interval_ms(mut self, ms: u64) -> Self {
        self.config.tick_interval_ms = ms;
        self
    }

    /// Set the number of worker threads.
    pub fn workers(mut self, count: usize) -> Self {
        self.config.workers = count;
        self
    }

    /// Enable or disable metrics collection.
    pub fn enable_metrics(mut self, enable: bool) -> Self {
        self.config.enable_metrics = enable;
        self
    }

    /// Set the metrics export interval.
    pub fn metrics_interval(mut self, interval: Duration) -> Self {
        self.config.metrics_interval_secs = interval.as_secs();
        self
    }

    /// Set the log level.
    pub fn log_level(mut self, level: impl Into<String>) -> Self {
        self.config.log_level = level.into();
        self
    }

    /// Apply a configuration preset.
    pub fn preset(mut self, preset: EnginePreset) -> Self {
        match preset {
            EnginePreset::Development => {
                self.config.max_devices = 100;
                self.config.max_points = 10_000;
                self.config.tick_interval_ms = 500;
                self.config.log_level = "debug".to_string();
            }
            EnginePreset::Production => {
                self.config.max_devices = 50_000;
                self.config.max_points = 5_000_000;
                self.config.tick_interval_ms = 100;
                self.config.log_level = "info".to_string();
            }
            EnginePreset::Testing => {
                self.config.max_devices = 10;
                self.config.max_points = 100;
                self.config.tick_interval_ms = 10;
                self.config.log_level = "trace".to_string();
            }
            EnginePreset::StressTest => {
                self.config.max_devices = 100_000;
                self.config.max_points = 10_000_000;
                self.config.tick_interval_ms = 50;
                self.config.log_level = "warn".to_string();
            }
        }
        self
    }

    /// Use an existing configuration.
    pub fn with_config(mut self, config: EngineConfig) -> Self {
        self.config = config;
        self
    }

    /// Build the simulator engine.
    pub fn build(self) -> SimulatorEngine {
        SimulatorEngine::new(self.config)
    }

    /// Build and start the engine.
    pub async fn build_and_start(self) -> Result<SimulatorEngine> {
        let engine = self.build();
        engine.start().await?;
        Ok(engine)
    }
}

/// Engine configuration presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnginePreset {
    /// Development preset: small scale, verbose logging.
    Development,
    /// Production preset: large scale, optimized settings.
    Production,
    /// Testing preset: minimal scale, fast ticks.
    Testing,
    /// Stress test preset: maximum scale.
    StressTest,
}

/// Shorthand function for creating an engine builder.
pub fn engine() -> SimulatorEngineBuilder {
    SimulatorEngineBuilder::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_engine_creation() {
        let config = EngineConfig::default();
        let engine = SimulatorEngine::new(config);

        assert_eq!(engine.state(), EngineState::Stopped);
        assert_eq!(engine.device_count(), 0);
    }

    #[tokio::test]
    async fn test_engine_lifecycle() {
        let config = EngineConfig::default();
        let engine = SimulatorEngine::new(config);

        // Start
        engine.start().await.unwrap();
        assert_eq!(engine.state(), EngineState::Running);

        // Run a few ticks
        for _ in 0..5 {
            engine.tick().await.unwrap();
        }
        assert_eq!(engine.tick_count(), 5);

        // Stop
        engine.stop().await.unwrap();
        assert_eq!(engine.state(), EngineState::Stopped);
    }

    #[tokio::test]
    async fn test_engine_capacity() {
        let config = EngineConfig::default().with_max_devices(5);
        let engine = SimulatorEngine::new(config);

        assert_eq!(engine.config().max_devices, 5);
    }

    #[tokio::test]
    async fn test_engine_builder() {
        let engine = SimulatorEngineBuilder::new()
            .name("Test Engine")
            .max_devices(1000)
            .tick_interval_ms(50)
            .build();

        assert_eq!(engine.config().name, "Test Engine");
        assert_eq!(engine.config().max_devices, 1000);
        assert_eq!(engine.config().tick_interval_ms, 50);
    }

    #[tokio::test]
    async fn test_engine_builder_presets() {
        let dev_engine = SimulatorEngineBuilder::new()
            .preset(EnginePreset::Development)
            .build();
        assert_eq!(dev_engine.config().max_devices, 100);
        assert_eq!(dev_engine.config().log_level, "debug");

        let prod_engine = SimulatorEngineBuilder::new()
            .preset(EnginePreset::Production)
            .build();
        assert_eq!(prod_engine.config().max_devices, 50_000);
        assert_eq!(prod_engine.config().log_level, "info");

        let stress_engine = SimulatorEngineBuilder::new()
            .preset(EnginePreset::StressTest)
            .build();
        assert_eq!(stress_engine.config().max_devices, 100_000);
    }

    #[tokio::test]
    async fn test_engine_shorthand() {
        let e = engine().name("Quick Engine").max_devices(500).build();

        assert_eq!(e.config().name, "Quick Engine");
        assert_eq!(e.config().max_devices, 500);
    }
}
