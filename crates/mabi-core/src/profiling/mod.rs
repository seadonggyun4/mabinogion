//! Memory and performance profiling module.
//!
//! This module provides comprehensive profiling capabilities for the TRAP simulator,
//! including memory usage tracking, allocation analysis, and performance monitoring.
//!
//! # Features
//!
//! - **Memory Profiling**: Track heap allocations, memory regions, and usage patterns
//! - **Leak Detection**: Identify potential memory leaks through allocation tracking
//! - **Performance Profiling**: Measure operation latencies and throughput
//! - **Report Generation**: Generate detailed profiling reports
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_core::profiling::{MemoryProfiler, ProfilerConfig};
//!
//! let config = ProfilerConfig::default();
//! let profiler = MemoryProfiler::new(config);
//!
//! // Start profiling
//! profiler.start();
//!
//! // ... run your workload ...
//!
//! // Get a snapshot
//! let snapshot = profiler.snapshot();
//! println!("Current memory: {} bytes", snapshot.current_bytes);
//!
//! // Generate report
//! let report = profiler.generate_report();
//! ```

pub mod leak_detector;
pub mod memory;
pub mod report;

pub use leak_detector::*;
pub use memory::*;
pub use report::*;

use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Profiler configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilerConfig {
    /// Enable memory profiling.
    #[serde(default = "default_true")]
    pub memory_enabled: bool,

    /// Enable leak detection.
    #[serde(default = "default_true")]
    pub leak_detection_enabled: bool,

    /// Sampling interval for periodic snapshots.
    #[serde(default = "default_sampling_interval")]
    pub sampling_interval: Duration,

    /// Maximum number of snapshots to retain.
    #[serde(default = "default_max_snapshots")]
    pub max_snapshots: usize,

    /// Allocation size threshold for tracking (bytes).
    /// Allocations smaller than this are not individually tracked.
    #[serde(default = "default_allocation_threshold")]
    pub allocation_threshold: usize,

    /// Enable stack trace capture for allocations (expensive).
    #[serde(default)]
    pub capture_stack_traces: bool,

    /// Maximum stack trace depth.
    #[serde(default = "default_stack_depth")]
    pub max_stack_depth: usize,
}

fn default_true() -> bool {
    true
}

fn default_sampling_interval() -> Duration {
    Duration::from_secs(10)
}

fn default_max_snapshots() -> usize {
    1000
}

fn default_allocation_threshold() -> usize {
    1024 // 1KB
}

fn default_stack_depth() -> usize {
    16
}

impl Default for ProfilerConfig {
    fn default() -> Self {
        Self {
            memory_enabled: true,
            leak_detection_enabled: true,
            sampling_interval: default_sampling_interval(),
            max_snapshots: default_max_snapshots(),
            allocation_threshold: default_allocation_threshold(),
            capture_stack_traces: false,
            max_stack_depth: default_stack_depth(),
        }
    }
}

/// Combined profiler that manages all profiling subsystems.
pub struct Profiler {
    config: ProfilerConfig,
    memory_profiler: Arc<RwLock<MemoryProfiler>>,
    leak_detector: Arc<RwLock<LeakDetector>>,
    started: Arc<std::sync::atomic::AtomicBool>,
}

impl Profiler {
    /// Create a new profiler with the given configuration.
    pub fn new(config: ProfilerConfig) -> Self {
        let memory_config = MemoryProfilerConfig {
            sampling_interval: config.sampling_interval,
            max_snapshots: config.max_snapshots,
            track_allocations: config.memory_enabled,
            allocation_threshold: config.allocation_threshold,
        };

        let leak_config = LeakDetectorConfig {
            enabled: config.leak_detection_enabled,
            check_interval: config.sampling_interval * 6, // Check every minute by default
            growth_threshold_percent: 10.0,
            min_samples_for_detection: 6,
        };

        Self {
            config,
            memory_profiler: Arc::new(RwLock::new(MemoryProfiler::new(memory_config))),
            leak_detector: Arc::new(RwLock::new(LeakDetector::new(leak_config))),
            started: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Create a profiler with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(ProfilerConfig::default())
    }

    /// Get the profiler configuration.
    pub fn config(&self) -> &ProfilerConfig {
        &self.config
    }

    /// Start profiling.
    pub fn start(&self) {
        self.started
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.memory_profiler.write().start();
        self.leak_detector.write().start();
    }

    /// Stop profiling.
    pub fn stop(&self) {
        self.started
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.memory_profiler.write().stop();
        self.leak_detector.write().stop();
    }

    /// Check if profiling is active.
    pub fn is_running(&self) -> bool {
        self.started.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Record a memory allocation.
    pub fn record_allocation(&self, label: &str, size: usize) {
        if self.is_running() {
            self.memory_profiler.write().record_allocation(label, size);
            self.leak_detector.write().record_allocation(label, size);
        }
    }

    /// Record a memory deallocation.
    pub fn record_deallocation(&self, label: &str, size: usize) {
        if self.is_running() {
            self.memory_profiler
                .write()
                .record_deallocation(label, size);
            self.leak_detector.write().record_deallocation(label, size);
        }
    }

    /// Take a memory snapshot.
    pub fn snapshot(&self) -> MemorySnapshot {
        self.memory_profiler.read().snapshot()
    }

    /// Check for potential memory leaks.
    pub fn check_leaks(&self) -> Vec<LeakWarning> {
        self.leak_detector.read().check()
    }

    /// Generate a comprehensive profiling report.
    pub fn generate_report(&self) -> ProfileReport {
        let memory_report = self.memory_profiler.read().generate_report();
        let leak_warnings = self.check_leaks();

        ProfileReport {
            generated_at: chrono::Utc::now(),
            config: self.config.clone(),
            memory: memory_report,
            leak_warnings,
            is_running: self.is_running(),
        }
    }

    /// Get the memory profiler.
    pub fn memory_profiler(&self) -> Arc<RwLock<MemoryProfiler>> {
        Arc::clone(&self.memory_profiler)
    }

    /// Get the leak detector.
    pub fn leak_detector(&self) -> Arc<RwLock<LeakDetector>> {
        Arc::clone(&self.leak_detector)
    }

    /// Reset all profiling data.
    pub fn reset(&self) {
        self.memory_profiler.write().reset();
        self.leak_detector.write().reset();
    }
}

impl std::fmt::Debug for Profiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Profiler")
            .field("config", &self.config)
            .field("is_running", &self.is_running())
            .finish()
    }
}

/// Comprehensive profiling report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileReport {
    /// When the report was generated.
    pub generated_at: chrono::DateTime<chrono::Utc>,

    /// Profiler configuration used.
    pub config: ProfilerConfig,

    /// Memory profiling report.
    pub memory: MemoryReport,

    /// Detected leak warnings.
    pub leak_warnings: Vec<LeakWarning>,

    /// Whether profiling is currently running.
    pub is_running: bool,
}

impl ProfileReport {
    /// Check if there are any leak warnings.
    pub fn has_leak_warnings(&self) -> bool {
        !self.leak_warnings.is_empty()
    }

    /// Get the current memory usage in bytes.
    pub fn current_memory_bytes(&self) -> u64 {
        self.memory.current_bytes
    }

    /// Get the peak memory usage in bytes.
    pub fn peak_memory_bytes(&self) -> u64 {
        self.memory.peak_bytes
    }

    /// Export report as JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Export report as a human-readable string.
    pub fn to_summary(&self) -> String {
        let mut summary = String::new();
        summary.push_str("=== TRAP Simulator Profiling Report ===\n\n");

        summary.push_str(&format!(
            "Generated: {}\n",
            self.generated_at.format("%Y-%m-%d %H:%M:%S UTC")
        ));
        summary.push_str(&format!(
            "Status: {}\n",
            if self.is_running {
                "Running"
            } else {
                "Stopped"
            }
        ));

        summary.push_str("\n--- Memory Usage ---\n");
        summary.push_str(&format!(
            "Current: {} MB\n",
            self.memory.current_bytes / 1024 / 1024
        ));
        summary.push_str(&format!(
            "Peak: {} MB\n",
            self.memory.peak_bytes / 1024 / 1024
        ));
        summary.push_str(&format!(
            "Total Allocated: {} MB\n",
            self.memory.total_allocated / 1024 / 1024
        ));
        summary.push_str(&format!(
            "Total Deallocated: {} MB\n",
            self.memory.total_deallocated / 1024 / 1024
        ));
        summary.push_str(&format!(
            "Allocation Count: {}\n",
            self.memory.allocation_count
        ));

        if !self.memory.regions.is_empty() {
            summary.push_str("\n--- Memory Regions ---\n");
            for (name, region) in &self.memory.regions {
                summary.push_str(&format!(
                    "  {}: {} KB (peak: {} KB, allocs: {})\n",
                    name,
                    region.current_bytes / 1024,
                    region.peak_bytes / 1024,
                    region.allocation_count
                ));
            }
        }

        if !self.leak_warnings.is_empty() {
            summary.push_str("\n--- Leak Warnings ---\n");
            for warning in &self.leak_warnings {
                summary.push_str(&format!(
                    "  [{}] {}: {} (growth: {:.1}%/min)\n",
                    warning.severity,
                    warning.region,
                    warning.message,
                    warning.growth_rate_per_minute
                ));
            }
        } else {
            summary.push_str("\n--- No leak warnings detected ---\n");
        }

        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profiler_config_default() {
        let config = ProfilerConfig::default();
        assert!(config.memory_enabled);
        assert!(config.leak_detection_enabled);
        assert_eq!(config.sampling_interval, Duration::from_secs(10));
    }

    #[test]
    fn test_profiler_lifecycle() {
        let profiler = Profiler::with_defaults();
        assert!(!profiler.is_running());

        profiler.start();
        assert!(profiler.is_running());

        profiler.stop();
        assert!(!profiler.is_running());
    }

    #[test]
    fn test_profiler_record_allocation() {
        let profiler = Profiler::with_defaults();
        profiler.start();

        profiler.record_allocation("test_region", 1024);
        profiler.record_allocation("test_region", 2048);

        let snapshot = profiler.snapshot();
        assert!(snapshot.current_bytes >= 3072);

        profiler.record_deallocation("test_region", 1024);

        let snapshot = profiler.snapshot();
        assert!(snapshot.current_bytes >= 2048);
    }

    #[test]
    fn test_profiler_report() {
        let profiler = Profiler::with_defaults();
        profiler.start();

        profiler.record_allocation("devices", 1024 * 1024);
        profiler.record_allocation("registers", 512 * 1024);

        let report = profiler.generate_report();
        assert!(report.is_running);
        assert!(!report.memory.regions.is_empty());

        let summary = report.to_summary();
        assert!(summary.contains("Memory Usage"));
    }

    #[test]
    fn test_profiler_reset() {
        let profiler = Profiler::with_defaults();
        profiler.start();

        profiler.record_allocation("test", 1024);
        profiler.reset();

        let snapshot = profiler.snapshot();
        assert_eq!(snapshot.current_bytes, 0);
    }
}
