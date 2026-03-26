//! Memory leak detection implementation.
//!
//! Provides heuristic-based leak detection by analyzing memory growth patterns
//! over time. This is not a replacement for tools like Valgrind or AddressSanitizer,
//! but provides runtime monitoring capabilities.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Leak detector configuration.
#[derive(Debug, Clone)]
pub struct LeakDetectorConfig {
    /// Enable leak detection.
    pub enabled: bool,

    /// How often to check for leaks.
    pub check_interval: Duration,

    /// Memory growth threshold to trigger a warning (percent per check interval).
    pub growth_threshold_percent: f64,

    /// Minimum number of samples before making leak determinations.
    pub min_samples_for_detection: usize,
}

impl Default for LeakDetectorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval: Duration::from_secs(60),
            growth_threshold_percent: 10.0,
            min_samples_for_detection: 6,
        }
    }
}

/// Memory leak detector.
///
/// Tracks memory allocation patterns and identifies potential leaks
/// based on sustained memory growth.
pub struct LeakDetector {
    config: LeakDetectorConfig,

    /// Per-region tracking data.
    regions: RwLock<HashMap<String, RegionTracker>>,

    /// Global tracking data.
    global: RwLock<GlobalTracker>,

    /// Detected warnings.
    warnings: RwLock<Vec<LeakWarning>>,

    /// State
    started_at: RwLock<Option<Instant>>,
    last_check_at: RwLock<Option<Instant>>,
    is_running: std::sync::atomic::AtomicBool,
}

impl LeakDetector {
    /// Create a new leak detector.
    pub fn new(config: LeakDetectorConfig) -> Self {
        Self {
            config,
            regions: RwLock::new(HashMap::new()),
            global: RwLock::new(GlobalTracker::new()),
            warnings: RwLock::new(Vec::new()),
            started_at: RwLock::new(None),
            last_check_at: RwLock::new(None),
            is_running: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Start leak detection.
    pub fn start(&mut self) {
        let now = Instant::now();
        *self.started_at.write() = Some(now);
        *self.last_check_at.write() = Some(now);
        self.is_running
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Stop leak detection.
    pub fn stop(&mut self) {
        self.is_running
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Check if leak detection is active.
    pub fn is_running(&self) -> bool {
        self.is_running.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Record an allocation for leak tracking.
    pub fn record_allocation(&self, label: &str, size: usize) {
        if !self.config.enabled {
            return;
        }

        let size = size as u64;
        let now = Instant::now();

        // Update global tracker
        self.global.write().record_allocation(size, now);

        // Update region tracker
        let mut regions = self.regions.write();
        regions
            .entry(label.to_string())
            .or_insert_with(|| RegionTracker::new(label))
            .record_allocation(size, now);
    }

    /// Record a deallocation for leak tracking.
    pub fn record_deallocation(&self, label: &str, size: usize) {
        if !self.config.enabled {
            return;
        }

        let size = size as u64;
        let now = Instant::now();

        // Update global tracker
        self.global.write().record_deallocation(size, now);

        // Update region tracker
        if let Some(region) = self.regions.write().get_mut(label) {
            region.record_deallocation(size, now);
        }
    }

    /// Check for potential memory leaks.
    pub fn check(&self) -> Vec<LeakWarning> {
        if !self.config.enabled {
            return Vec::new();
        }

        let mut warnings = Vec::new();

        // Check global memory
        {
            let global = self.global.read();
            if let Some(warning) = self.analyze_tracker(&*global, "global") {
                warnings.push(warning);
            }
        }

        // Check each region
        {
            let regions = self.regions.read();
            for (name, tracker) in regions.iter() {
                if let Some(warning) = self.analyze_tracker(tracker, name) {
                    warnings.push(warning);
                }
            }
        }

        // Store warnings
        *self.warnings.write() = warnings.clone();

        warnings
    }

    /// Analyze a tracker for potential leaks.
    fn analyze_tracker<T: MemoryTracker>(&self, tracker: &T, name: &str) -> Option<LeakWarning> {
        let samples = tracker.samples();

        // Need minimum samples
        if samples.len() < self.config.min_samples_for_detection {
            return None;
        }

        // Calculate growth rate
        let growth_analysis = analyze_growth(&samples);

        // Check for sustained growth
        if growth_analysis.is_growing
            && growth_analysis.growth_rate_per_minute > self.config.growth_threshold_percent
        {
            let severity = if growth_analysis.growth_rate_per_minute > 50.0 {
                LeakSeverity::Critical
            } else if growth_analysis.growth_rate_per_minute > 20.0 {
                LeakSeverity::High
            } else {
                LeakSeverity::Medium
            };

            return Some(LeakWarning {
                region: name.to_string(),
                severity,
                message: format!(
                    "Sustained memory growth detected: {:.1}% per minute",
                    growth_analysis.growth_rate_per_minute
                ),
                growth_rate_per_minute: growth_analysis.growth_rate_per_minute,
                current_bytes: tracker.current_bytes(),
                samples_analyzed: samples.len(),
                confidence: growth_analysis.confidence,
            });
        }

        // Check for allocation/deallocation imbalance
        let stats = tracker.stats();
        if stats.allocation_count > 0 && stats.deallocation_count == 0 {
            // Only allocations, no deallocations - potential leak
            return Some(LeakWarning {
                region: name.to_string(),
                severity: LeakSeverity::Low,
                message: "Allocations without deallocations detected".to_string(),
                growth_rate_per_minute: 0.0,
                current_bytes: tracker.current_bytes(),
                samples_analyzed: samples.len(),
                confidence: 0.5,
            });
        }

        None
    }

    /// Get all current warnings.
    pub fn warnings(&self) -> Vec<LeakWarning> {
        self.warnings.read().clone()
    }

    /// Reset leak detection data.
    pub fn reset(&mut self) {
        self.regions.write().clear();
        *self.global.write() = GlobalTracker::new();
        self.warnings.write().clear();
    }

    /// Get analysis for a specific region.
    pub fn region_analysis(&self, name: &str) -> Option<RegionAnalysis> {
        let regions = self.regions.read();
        regions.get(name).map(|tracker| {
            let samples = tracker.samples();
            let growth = analyze_growth(&samples);

            RegionAnalysis {
                name: name.to_string(),
                current_bytes: tracker.current_bytes(),
                allocation_count: tracker.stats().allocation_count,
                deallocation_count: tracker.stats().deallocation_count,
                growth_rate_per_minute: growth.growth_rate_per_minute,
                is_growing: growth.is_growing,
                confidence: growth.confidence,
                sample_count: samples.len(),
            }
        })
    }
}

/// Trait for memory trackers.
trait MemoryTracker {
    fn samples(&self) -> Vec<MemorySample>;
    fn current_bytes(&self) -> u64;
    fn stats(&self) -> TrackerStats;
}

/// Statistics from a tracker.
#[derive(Debug, Clone, Default)]
struct TrackerStats {
    allocation_count: u64,
    deallocation_count: u64,
}

/// Global memory tracker.
struct GlobalTracker {
    current_bytes: u64,
    allocation_count: u64,
    deallocation_count: u64,
    samples: VecDeque<MemorySample>,
    last_sample_at: Option<Instant>,
}

impl GlobalTracker {
    fn new() -> Self {
        Self {
            current_bytes: 0,
            allocation_count: 0,
            deallocation_count: 0,
            samples: VecDeque::with_capacity(100),
            last_sample_at: None,
        }
    }

    fn record_allocation(&mut self, size: u64, now: Instant) {
        self.current_bytes += size;
        self.allocation_count += 1;
        self.maybe_record_sample(now);
    }

    fn record_deallocation(&mut self, size: u64, now: Instant) {
        self.current_bytes = self.current_bytes.saturating_sub(size);
        self.deallocation_count += 1;
        self.maybe_record_sample(now);
    }

    fn maybe_record_sample(&mut self, now: Instant) {
        let should_sample = match self.last_sample_at {
            Some(last) => now.duration_since(last) >= Duration::from_secs(10),
            None => true,
        };

        if should_sample {
            self.samples.push_back(MemorySample {
                bytes: self.current_bytes,
                timestamp: now,
            });

            // Keep max 100 samples
            while self.samples.len() > 100 {
                self.samples.pop_front();
            }

            self.last_sample_at = Some(now);
        }
    }
}

impl MemoryTracker for GlobalTracker {
    fn samples(&self) -> Vec<MemorySample> {
        self.samples.iter().cloned().collect()
    }

    fn current_bytes(&self) -> u64 {
        self.current_bytes
    }

    fn stats(&self) -> TrackerStats {
        TrackerStats {
            allocation_count: self.allocation_count,
            deallocation_count: self.deallocation_count,
        }
    }
}

/// Per-region memory tracker.
struct RegionTracker {
    #[allow(dead_code)]
    name: String,
    current_bytes: u64,
    allocation_count: u64,
    deallocation_count: u64,
    samples: VecDeque<MemorySample>,
    last_sample_at: Option<Instant>,
}

impl RegionTracker {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            current_bytes: 0,
            allocation_count: 0,
            deallocation_count: 0,
            samples: VecDeque::with_capacity(100),
            last_sample_at: None,
        }
    }

    fn record_allocation(&mut self, size: u64, now: Instant) {
        self.current_bytes += size;
        self.allocation_count += 1;
        self.maybe_record_sample(now);
    }

    fn record_deallocation(&mut self, size: u64, now: Instant) {
        self.current_bytes = self.current_bytes.saturating_sub(size);
        self.deallocation_count += 1;
        self.maybe_record_sample(now);
    }

    fn maybe_record_sample(&mut self, now: Instant) {
        let should_sample = match self.last_sample_at {
            Some(last) => now.duration_since(last) >= Duration::from_secs(10),
            None => true,
        };

        if should_sample {
            self.samples.push_back(MemorySample {
                bytes: self.current_bytes,
                timestamp: now,
            });

            while self.samples.len() > 100 {
                self.samples.pop_front();
            }

            self.last_sample_at = Some(now);
        }
    }
}

impl MemoryTracker for RegionTracker {
    fn samples(&self) -> Vec<MemorySample> {
        self.samples.iter().cloned().collect()
    }

    fn current_bytes(&self) -> u64 {
        self.current_bytes
    }

    fn stats(&self) -> TrackerStats {
        TrackerStats {
            allocation_count: self.allocation_count,
            deallocation_count: self.deallocation_count,
        }
    }
}

/// Memory sample at a point in time.
#[derive(Debug, Clone)]
struct MemorySample {
    bytes: u64,
    timestamp: Instant,
}

/// Growth analysis result.
#[derive(Debug)]
struct GrowthAnalysis {
    is_growing: bool,
    growth_rate_per_minute: f64,
    confidence: f64,
}

/// Analyze memory growth from samples.
fn analyze_growth(samples: &[MemorySample]) -> GrowthAnalysis {
    if samples.len() < 2 {
        return GrowthAnalysis {
            is_growing: false,
            growth_rate_per_minute: 0.0,
            confidence: 0.0,
        };
    }

    // Use linear regression to determine growth trend
    let n = samples.len() as f64;

    // Convert timestamps to seconds from first sample
    let first_time = samples[0].timestamp;
    let points: Vec<(f64, f64)> = samples
        .iter()
        .map(|s| {
            let x = s.timestamp.duration_since(first_time).as_secs_f64();
            let y = s.bytes as f64;
            (x, y)
        })
        .collect();

    // Calculate means
    let mean_x: f64 = points.iter().map(|(x, _)| x).sum::<f64>() / n;
    let mean_y: f64 = points.iter().map(|(_, y)| y).sum::<f64>() / n;

    // Calculate slope (bytes per second)
    let numerator: f64 = points
        .iter()
        .map(|(x, y)| (x - mean_x) * (y - mean_y))
        .sum();
    let denominator: f64 = points.iter().map(|(x, _)| (x - mean_x).powi(2)).sum();

    if denominator == 0.0 {
        return GrowthAnalysis {
            is_growing: false,
            growth_rate_per_minute: 0.0,
            confidence: 0.0,
        };
    }

    let slope = numerator / denominator;
    let growth_rate_per_minute = slope * 60.0; // Convert to per minute

    // Calculate R-squared for confidence
    let ss_tot: f64 = points.iter().map(|(_, y)| (y - mean_y).powi(2)).sum();
    let ss_res: f64 = points
        .iter()
        .map(|(x, y)| {
            let predicted = mean_y + slope * (x - mean_x);
            (y - predicted).powi(2)
        })
        .sum();

    let r_squared = if ss_tot > 0.0 {
        1.0 - (ss_res / ss_tot)
    } else {
        0.0
    };

    // Consider growing if slope is positive and R-squared is decent
    let is_growing = slope > 0.0 && r_squared > 0.5;

    // Use percent growth relative to initial value
    let initial_bytes = samples[0].bytes as f64;
    let growth_percent_per_minute = if initial_bytes > 0.0 {
        (growth_rate_per_minute / initial_bytes) * 100.0
    } else {
        0.0
    };

    GrowthAnalysis {
        is_growing,
        growth_rate_per_minute: growth_percent_per_minute,
        confidence: r_squared.max(0.0).min(1.0),
    }
}

/// Memory leak warning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeakWarning {
    /// Affected memory region.
    pub region: String,

    /// Warning severity.
    pub severity: LeakSeverity,

    /// Human-readable message.
    pub message: String,

    /// Memory growth rate (percent per minute).
    pub growth_rate_per_minute: f64,

    /// Current memory usage in the region.
    pub current_bytes: u64,

    /// Number of samples analyzed.
    pub samples_analyzed: usize,

    /// Confidence level (0.0 - 1.0).
    pub confidence: f64,
}

/// Leak warning severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeakSeverity {
    /// Informational - may be normal behavior.
    Low,
    /// Medium - worth investigating.
    Medium,
    /// High - likely a leak.
    High,
    /// Critical - significant memory leak.
    Critical,
}

impl std::fmt::Display for LeakSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "LOW"),
            Self::Medium => write!(f, "MEDIUM"),
            Self::High => write!(f, "HIGH"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Analysis of a memory region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionAnalysis {
    /// Region name.
    pub name: String,

    /// Current memory usage.
    pub current_bytes: u64,

    /// Number of allocations.
    pub allocation_count: u64,

    /// Number of deallocations.
    pub deallocation_count: u64,

    /// Growth rate (percent per minute).
    pub growth_rate_per_minute: f64,

    /// Whether memory is growing.
    pub is_growing: bool,

    /// Confidence in the analysis.
    pub confidence: f64,

    /// Number of samples used.
    pub sample_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leak_detector_basic() {
        let mut detector = LeakDetector::new(LeakDetectorConfig::default());
        detector.start();

        detector.record_allocation("test", 1024);
        detector.record_deallocation("test", 512);

        // Should not detect leak with balanced operations
        let warnings = detector.check();
        assert!(warnings.is_empty() || warnings.iter().all(|w| w.severity == LeakSeverity::Low));
    }

    #[test]
    fn test_leak_detector_config() {
        let config = LeakDetectorConfig {
            enabled: true,
            check_interval: Duration::from_secs(30),
            growth_threshold_percent: 5.0,
            min_samples_for_detection: 3,
        };

        let detector = LeakDetector::new(config);
        assert!(detector.config.enabled);
        assert_eq!(detector.config.growth_threshold_percent, 5.0);
    }

    #[test]
    fn test_leak_detector_disabled() {
        let config = LeakDetectorConfig {
            enabled: false,
            ..Default::default()
        };

        let mut detector = LeakDetector::new(config);
        detector.start();

        detector.record_allocation("test", 1024);
        let warnings = detector.check();
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_leak_severity_display() {
        assert_eq!(LeakSeverity::Low.to_string(), "LOW");
        assert_eq!(LeakSeverity::Critical.to_string(), "CRITICAL");
    }

    #[test]
    fn test_growth_analysis_empty() {
        let analysis = analyze_growth(&[]);
        assert!(!analysis.is_growing);
        assert_eq!(analysis.confidence, 0.0);
    }

    #[test]
    fn test_growth_analysis_single() {
        let samples = vec![MemorySample {
            bytes: 1000,
            timestamp: Instant::now(),
        }];
        let analysis = analyze_growth(&samples);
        assert!(!analysis.is_growing);
    }

    #[test]
    fn test_leak_detector_reset() {
        let mut detector = LeakDetector::new(LeakDetectorConfig::default());
        detector.start();

        detector.record_allocation("test", 1024);
        detector.reset();

        assert!(detector.warnings().is_empty());
    }
}
