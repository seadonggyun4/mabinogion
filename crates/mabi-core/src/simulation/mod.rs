//! Simulation framework for comprehensive testing scenarios.
//!
//! This module provides a flexible framework for simulating various conditions
//! that the TRAP simulator might encounter in production environments.
//!
//! # Features
//!
//! - **Scale Simulation**: Test with 50,000+ virtual devices
//! - **Memory Patterns**: Simulate various memory allocation patterns
//! - **Failure Injection**: Inject faults and errors for resilience testing
//! - **Load Patterns**: Various load profiles (steady, burst, ramp, spike)
//! - **Resource Constraints**: Simulate limited memory/CPU environments
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_core::simulation::{
//!     Simulator, SimulationConfig, ScaleConfig,
//!     MemoryPattern, LoadPattern,
//! };
//!
//! let config = SimulationConfig::default()
//!     .with_scale(ScaleConfig::devices(50_000))
//!     .with_memory_pattern(MemoryPattern::GrowthAndRelease)
//!     .with_load_pattern(LoadPattern::Burst { peak: 100_000, duration_secs: 60 });
//!
//! let simulator = Simulator::new(config);
//! let results = simulator.run().await?;
//! ```

pub mod scale;
pub mod memory_sim;
pub mod failure;
pub mod load;
pub mod scenarios;

pub use scale::*;
pub use memory_sim::*;
pub use failure::*;
pub use load::*;
pub use scenarios::*;

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::error::Result;
use crate::profiling::{Profiler, ProfileReport};

/// Main simulation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationConfig {
    /// Name of the simulation.
    pub name: String,

    /// Description of what this simulation tests.
    pub description: String,

    /// Scale configuration (number of devices, points, etc.).
    pub scale: ScaleConfig,

    /// Memory simulation pattern.
    pub memory_pattern: MemoryPattern,

    /// Load pattern to apply.
    pub load_pattern: LoadPattern,

    /// Failure injection configuration.
    pub failure_config: Option<FailureConfig>,

    /// Maximum duration for the simulation.
    pub max_duration: Duration,

    /// Interval between progress reports.
    pub report_interval: Duration,

    /// Whether to collect detailed metrics.
    pub detailed_metrics: bool,

    /// Random seed for reproducibility.
    pub seed: Option<u64>,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            name: "default_simulation".into(),
            description: "Default simulation configuration".into(),
            scale: ScaleConfig::default(),
            memory_pattern: MemoryPattern::Steady,
            load_pattern: LoadPattern::Steady { ops_per_sec: 1000 },
            failure_config: None,
            max_duration: Duration::from_secs(300),
            report_interval: Duration::from_secs(10),
            detailed_metrics: true,
            seed: None,
        }
    }
}

impl SimulationConfig {
    /// Create a new simulation config with a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Set the scale configuration.
    pub fn with_scale(mut self, scale: ScaleConfig) -> Self {
        self.scale = scale;
        self
    }

    /// Set the memory pattern.
    pub fn with_memory_pattern(mut self, pattern: MemoryPattern) -> Self {
        self.memory_pattern = pattern;
        self
    }

    /// Set the load pattern.
    pub fn with_load_pattern(mut self, pattern: LoadPattern) -> Self {
        self.load_pattern = pattern;
        self
    }

    /// Set the failure configuration.
    pub fn with_failures(mut self, config: FailureConfig) -> Self {
        self.failure_config = Some(config);
        self
    }

    /// Set the maximum duration.
    pub fn with_max_duration(mut self, duration: Duration) -> Self {
        self.max_duration = duration;
        self
    }

    /// Set the random seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Create a quick test configuration.
    pub fn quick_test() -> Self {
        Self {
            name: "quick_test".into(),
            description: "Quick validation test".into(),
            scale: ScaleConfig::small(),
            max_duration: Duration::from_secs(10),
            ..Default::default()
        }
    }

    /// Create a stress test configuration.
    pub fn stress_test() -> Self {
        Self {
            name: "stress_test".into(),
            description: "High-load stress test".into(),
            scale: ScaleConfig::large(),
            memory_pattern: MemoryPattern::HighChurn,
            load_pattern: LoadPattern::Spike {
                baseline: 10_000,
                peak: 100_000,
                spike_duration: Duration::from_secs(30),
            },
            max_duration: Duration::from_secs(600),
            ..Default::default()
        }
    }

    /// Create a memory leak detection configuration.
    pub fn memory_leak_test() -> Self {
        Self {
            name: "memory_leak_test".into(),
            description: "Test for memory leaks over time".into(),
            scale: ScaleConfig::medium(),
            memory_pattern: MemoryPattern::GrowthOnly,
            max_duration: Duration::from_secs(300),
            report_interval: Duration::from_secs(5),
            ..Default::default()
        }
    }
}

/// Simulation events for progress tracking.
#[derive(Debug, Clone)]
pub enum SimulationEvent {
    /// Simulation started.
    Started {
        config: SimulationConfig,
        start_time: Instant,
    },

    /// Progress update.
    Progress {
        elapsed: Duration,
        completion_percent: f64,
        current_metrics: SimulationMetrics,
    },

    /// Phase changed.
    PhaseChanged {
        from: SimulationPhase,
        to: SimulationPhase,
    },

    /// Error occurred.
    Error {
        message: String,
        recoverable: bool,
    },

    /// Simulation completed.
    Completed {
        result: SimulationResult,
    },
}

/// Current phase of the simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SimulationPhase {
    /// Initializing resources.
    Initializing,
    /// Warming up (pre-test stabilization).
    WarmUp,
    /// Main test execution.
    Running,
    /// Ramping down.
    RampDown,
    /// Collecting final metrics.
    Finalizing,
    /// Completed.
    Completed,
}

/// Real-time simulation metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SimulationMetrics {
    /// Current memory usage in bytes.
    pub memory_bytes: u64,
    /// Peak memory usage in bytes.
    pub peak_memory_bytes: u64,
    /// Current number of active devices.
    pub active_devices: u64,
    /// Total operations performed.
    pub total_operations: u64,
    /// Operations per second (current rate).
    pub ops_per_second: f64,
    /// Error count.
    pub error_count: u64,
    /// Average latency in microseconds.
    pub avg_latency_us: u64,
    /// P99 latency in microseconds.
    pub p99_latency_us: u64,
}

/// Final simulation result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    /// Simulation name.
    pub name: String,
    /// Whether the simulation passed all criteria.
    pub passed: bool,
    /// Total duration of the simulation.
    pub duration: Duration,
    /// Final metrics.
    pub final_metrics: SimulationMetrics,
    /// Memory profiling report.
    pub memory_report: Option<ProfileReport>,
    /// Any warnings generated.
    pub warnings: Vec<String>,
    /// Any errors that occurred.
    pub errors: Vec<String>,
    /// Per-phase timing.
    pub phase_durations: std::collections::HashMap<String, Duration>,
}

impl SimulationResult {
    /// Check if there were any errors.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Get a summary string.
    pub fn summary(&self) -> String {
        format!(
            "Simulation '{}': {} in {:?}\n  \
             Devices: {}, Ops: {}, Errors: {}\n  \
             Memory: {} MB (peak: {} MB)\n  \
             Latency: avg {}us, p99 {}us",
            self.name,
            if self.passed { "PASSED" } else { "FAILED" },
            self.duration,
            self.final_metrics.active_devices,
            self.final_metrics.total_operations,
            self.final_metrics.error_count,
            self.final_metrics.memory_bytes / 1024 / 1024,
            self.final_metrics.peak_memory_bytes / 1024 / 1024,
            self.final_metrics.avg_latency_us,
            self.final_metrics.p99_latency_us,
        )
    }
}

/// Main simulator engine.
pub struct Simulator {
    config: SimulationConfig,
    profiler: Arc<Profiler>,
    metrics: Arc<RwLock<SimulationMetrics>>,
    phase: Arc<RwLock<SimulationPhase>>,
    event_tx: broadcast::Sender<SimulationEvent>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl Simulator {
    /// Create a new simulator with the given configuration.
    pub fn new(config: SimulationConfig) -> Self {
        let (event_tx, _) = broadcast::channel(256);

        Self {
            config,
            profiler: Arc::new(Profiler::with_defaults()),
            metrics: Arc::new(RwLock::new(SimulationMetrics::default())),
            phase: Arc::new(RwLock::new(SimulationPhase::Initializing)),
            event_tx,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Subscribe to simulation events.
    pub fn subscribe(&self) -> broadcast::Receiver<SimulationEvent> {
        self.event_tx.subscribe()
    }

    /// Get current metrics.
    pub fn metrics(&self) -> SimulationMetrics {
        self.metrics.read().clone()
    }

    /// Get current phase.
    pub fn phase(&self) -> SimulationPhase {
        *self.phase.read()
    }

    /// Check if simulation is running.
    pub fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Run the simulation.
    pub async fn run(&self) -> Result<SimulationResult> {
        let start_time = Instant::now();

        // Start profiling
        self.profiler.start();
        self.running.store(true, std::sync::atomic::Ordering::SeqCst);

        // Emit start event
        let _ = self.event_tx.send(SimulationEvent::Started {
            config: self.config.clone(),
            start_time,
        });

        let mut phase_durations = std::collections::HashMap::new();
        let mut warnings = Vec::new();
        let mut errors = Vec::new();

        // Phase 1: Initialize
        self.set_phase(SimulationPhase::Initializing);
        let init_start = Instant::now();
        if let Err(e) = self.run_initialization().await {
            errors.push(format!("Initialization failed: {}", e));
        }
        phase_durations.insert("initialization".into(), init_start.elapsed());

        // Phase 2: Warm-up
        if errors.is_empty() {
            self.set_phase(SimulationPhase::WarmUp);
            let warmup_start = Instant::now();
            if let Err(e) = self.run_warmup().await {
                warnings.push(format!("Warm-up warning: {}", e));
            }
            phase_durations.insert("warmup".into(), warmup_start.elapsed());
        }

        // Phase 3: Main execution
        if errors.is_empty() {
            self.set_phase(SimulationPhase::Running);
            let run_start = Instant::now();

            // Run until max duration or completion
            let run_result = self.run_main_phase(start_time).await;
            if let Err(e) = run_result {
                errors.push(format!("Execution error: {}", e));
            }
            phase_durations.insert("execution".into(), run_start.elapsed());
        }

        // Phase 4: Ramp down
        self.set_phase(SimulationPhase::RampDown);
        let rampdown_start = Instant::now();
        self.run_rampdown().await;
        phase_durations.insert("rampdown".into(), rampdown_start.elapsed());

        // Phase 5: Finalize
        self.set_phase(SimulationPhase::Finalizing);
        let finalize_start = Instant::now();

        // Stop profiling and get report
        self.profiler.stop();
        let memory_report = Some(self.profiler.generate_report());

        // Check for memory leaks
        let leak_warnings = self.profiler.check_leaks();
        for leak in leak_warnings {
            warnings.push(format!("Potential memory leak: {} - {}", leak.region, leak.message));
        }

        phase_durations.insert("finalization".into(), finalize_start.elapsed());

        // Build result
        self.set_phase(SimulationPhase::Completed);
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);

        let final_metrics = self.metrics.read().clone();
        let passed = errors.is_empty() && self.check_success_criteria(&final_metrics);

        let result = SimulationResult {
            name: self.config.name.clone(),
            passed,
            duration: start_time.elapsed(),
            final_metrics,
            memory_report,
            warnings,
            errors,
            phase_durations,
        };

        // Emit completion event
        let _ = self.event_tx.send(SimulationEvent::Completed {
            result: result.clone(),
        });

        Ok(result)
    }

    fn set_phase(&self, new_phase: SimulationPhase) {
        let old_phase = *self.phase.read();
        *self.phase.write() = new_phase;

        let _ = self.event_tx.send(SimulationEvent::PhaseChanged {
            from: old_phase,
            to: new_phase,
        });
    }

    async fn run_initialization(&self) -> Result<()> {
        // Initialize based on scale config
        let target_devices = self.config.scale.device_count;
        let batch_size = self.config.scale.batch_size;

        tracing::info!(
            "Initializing simulation: {} devices in batches of {}",
            target_devices,
            batch_size
        );

        let mut current = 0u64;
        while current < target_devices as u64 {
            let batch = (target_devices as u64 - current).min(batch_size as u64);

            // Simulate device creation
            self.profiler.record_allocation(
                "devices",
                (batch as usize) * self.config.scale.memory_per_device,
            );

            current += batch;

            // Update metrics
            {
                let mut metrics = self.metrics.write();
                metrics.active_devices = current;
                metrics.memory_bytes = self.profiler.snapshot().current_bytes;
            }

            // Yield to allow other tasks
            tokio::task::yield_now().await;
        }

        Ok(())
    }

    async fn run_warmup(&self) -> Result<()> {
        // Brief warm-up period
        let warmup_duration = Duration::from_secs(5);
        let start = Instant::now();

        while start.elapsed() < warmup_duration {
            // Simulate some operations
            self.simulate_operations(100).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Ok(())
    }

    async fn run_main_phase(&self, start_time: Instant) -> Result<()> {
        let mut last_report = Instant::now();

        while start_time.elapsed() < self.config.max_duration {
            // Apply load pattern
            let ops = self.config.load_pattern.ops_for_elapsed(start_time.elapsed());
            self.simulate_operations(ops).await;

            // Apply memory pattern
            self.apply_memory_pattern(start_time.elapsed()).await;

            // Inject failures if configured
            if let Some(ref failure_config) = self.config.failure_config {
                self.inject_failures(failure_config).await;
            }

            // Report progress
            if last_report.elapsed() >= self.config.report_interval {
                let metrics = self.metrics.read().clone();
                let completion = start_time.elapsed().as_secs_f64()
                    / self.config.max_duration.as_secs_f64()
                    * 100.0;

                let _ = self.event_tx.send(SimulationEvent::Progress {
                    elapsed: start_time.elapsed(),
                    completion_percent: completion.min(100.0),
                    current_metrics: metrics,
                });

                last_report = Instant::now();
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        Ok(())
    }

    async fn run_rampdown(&self) {
        // Gracefully release resources
        let current_devices = self.metrics.read().active_devices;
        let batch_size = self.config.scale.batch_size as u64;

        let mut remaining = current_devices;
        while remaining > 0 {
            let batch = remaining.min(batch_size);

            self.profiler.record_deallocation(
                "devices",
                (batch as usize) * self.config.scale.memory_per_device,
            );

            remaining -= batch;

            {
                let mut metrics = self.metrics.write();
                metrics.active_devices = remaining;
                metrics.memory_bytes = self.profiler.snapshot().current_bytes;
            }

            tokio::task::yield_now().await;
        }
    }

    async fn simulate_operations(&self, count: u64) {
        let mut metrics = self.metrics.write();
        metrics.total_operations += count;

        // Simulate latency
        let base_latency = 100u64; // 100us base
        let jitter = (count % 50) * 2;
        metrics.avg_latency_us = base_latency + jitter;
        metrics.p99_latency_us = base_latency * 3 + jitter * 2;
    }

    async fn apply_memory_pattern(&self, elapsed: Duration) {
        match &self.config.memory_pattern {
            MemoryPattern::Steady => {
                // No additional allocations
            }
            MemoryPattern::GrowthOnly => {
                // Continuous growth
                let growth = (elapsed.as_secs() * 1024) as usize;
                self.profiler.record_allocation("growth", growth);
            }
            MemoryPattern::GrowthAndRelease => {
                // Periodic allocation and deallocation
                let cycle = elapsed.as_secs() % 60;
                if cycle < 30 {
                    self.profiler.record_allocation("cyclic", 10240);
                } else {
                    self.profiler.record_deallocation("cyclic", 10240);
                }
            }
            MemoryPattern::HighChurn => {
                // Frequent small allocations and deallocations
                self.profiler.record_allocation("churn", 1024);
                self.profiler.record_deallocation("churn", 512);
            }
            MemoryPattern::Fragmentation => {
                // Create fragmentation by varying sizes
                let sizes = [64, 256, 1024, 4096, 16384];
                let idx = (elapsed.as_millis() as usize) % sizes.len();
                self.profiler.record_allocation("frag", sizes[idx]);
                if elapsed.as_millis() % 2 == 0 {
                    self.profiler.record_deallocation("frag", sizes[(idx + 2) % sizes.len()]);
                }
            }
            MemoryPattern::Custom(pattern) => {
                pattern.apply(&self.profiler, elapsed);
            }
        }

        // Update memory metrics
        let snapshot = self.profiler.snapshot();
        let mut metrics = self.metrics.write();
        metrics.memory_bytes = snapshot.current_bytes;
        if snapshot.current_bytes > metrics.peak_memory_bytes {
            metrics.peak_memory_bytes = snapshot.current_bytes;
        }
    }

    async fn inject_failures(&self, config: &FailureConfig) {
        if config.should_inject() {
            let mut metrics = self.metrics.write();
            metrics.error_count += 1;

            let _ = self.event_tx.send(SimulationEvent::Error {
                message: config.next_failure_type().to_string(),
                recoverable: true,
            });
        }
    }

    fn check_success_criteria(&self, metrics: &SimulationMetrics) -> bool {
        // Default success criteria
        let max_error_rate = 0.01; // 1% error rate
        let error_rate = if metrics.total_operations > 0 {
            metrics.error_count as f64 / metrics.total_operations as f64
        } else {
            0.0
        };

        error_rate <= max_error_rate
    }

    /// Stop the simulation early.
    pub fn stop(&self) {
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl std::fmt::Debug for Simulator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Simulator")
            .field("config", &self.config)
            .field("phase", &self.phase())
            .field("running", &self.is_running())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulation_config_default() {
        let config = SimulationConfig::default();
        assert_eq!(config.name, "default_simulation");
        assert_eq!(config.max_duration, Duration::from_secs(300));
    }

    #[test]
    fn test_simulation_config_builders() {
        let config = SimulationConfig::new("test")
            .with_scale(ScaleConfig::devices(1000))
            .with_memory_pattern(MemoryPattern::HighChurn)
            .with_max_duration(Duration::from_secs(60));

        assert_eq!(config.name, "test");
        assert_eq!(config.scale.device_count, 1000);
    }

    #[test]
    fn test_quick_test_config() {
        let config = SimulationConfig::quick_test();
        assert_eq!(config.max_duration, Duration::from_secs(10));
    }

    #[test]
    fn test_stress_test_config() {
        let config = SimulationConfig::stress_test();
        assert_eq!(config.name, "stress_test");
        matches!(config.memory_pattern, MemoryPattern::HighChurn);
    }

    #[test]
    fn test_simulator_creation() {
        let config = SimulationConfig::quick_test();
        let simulator = Simulator::new(config);
        assert!(!simulator.is_running());
        assert_eq!(simulator.phase(), SimulationPhase::Initializing);
    }

    #[test]
    fn test_simulation_result_summary() {
        let result = SimulationResult {
            name: "test".into(),
            passed: true,
            duration: Duration::from_secs(10),
            final_metrics: SimulationMetrics {
                memory_bytes: 1024 * 1024,
                peak_memory_bytes: 2 * 1024 * 1024,
                active_devices: 100,
                total_operations: 10000,
                ops_per_second: 1000.0,
                error_count: 0,
                avg_latency_us: 100,
                p99_latency_us: 500,
            },
            memory_report: None,
            warnings: vec![],
            errors: vec![],
            phase_durations: std::collections::HashMap::new(),
        };

        let summary = result.summary();
        assert!(summary.contains("PASSED"));
        assert!(summary.contains("10000"));
    }

    #[tokio::test]
    async fn test_simulator_quick_run() {
        let config = SimulationConfig::new("quick")
            .with_scale(ScaleConfig::tiny())
            .with_max_duration(Duration::from_millis(100));

        let simulator = Simulator::new(config);
        let result = simulator.run().await.unwrap();

        assert!(result.passed);
        assert!(result.final_metrics.active_devices == 0); // Should be cleaned up
    }
}
