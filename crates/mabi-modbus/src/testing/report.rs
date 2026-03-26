//! Test reporting utilities.
//!
//! Provides structured test reports for performance validation,
//! with support for various output formats.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Test metrics collected during performance testing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestMetrics {
    /// Total number of requests sent.
    pub total_requests: u64,
    /// Number of successful requests.
    pub successful_requests: u64,
    /// Number of failed requests.
    pub failed_requests: u64,
    /// Total number of connections established.
    pub total_connections: u64,
    /// Peak concurrent connections.
    pub peak_connections: u64,
    /// Average transactions per second.
    pub avg_tps: u64,
    /// Peak transactions per second.
    pub peak_tps: u64,
    /// P50 latency in milliseconds.
    pub p50_latency_ms: f64,
    /// P95 latency in milliseconds.
    pub p95_latency_ms: f64,
    /// P99 latency in milliseconds.
    pub p99_latency_ms: f64,
    /// Error rate (0.0 - 1.0).
    pub error_rate: f64,
    /// Peak memory usage in megabytes.
    pub memory_peak_mb: f64,
}

impl TestMetrics {
    /// Check if metrics meet basic quality thresholds.
    pub fn is_healthy(&self) -> bool {
        self.error_rate < 0.05 && self.p99_latency_ms < 100.0
    }
}

/// Summary of a test run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSummary {
    /// Test name.
    pub name: String,
    /// Test description.
    pub description: String,
    /// Whether the test passed.
    pub passed: bool,
    /// Test duration.
    #[serde(with = "humantime_serde")]
    pub duration: Duration,
    /// Start time.
    pub start_time: DateTime<Utc>,
    /// End time.
    pub end_time: DateTime<Utc>,
    /// Number of targets checked.
    pub targets_checked: usize,
    /// Number of targets passed.
    pub targets_passed: usize,
    /// Failure reasons (if any).
    pub failure_reasons: Vec<String>,
}

/// Complete test report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestReport {
    /// Report metadata.
    pub metadata: ReportMetadata,
    /// Test summary.
    pub summary: TestSummary,
    /// Collected metrics.
    pub metrics: TestMetrics,
    /// Individual target results.
    pub target_results: Vec<TargetResultEntry>,
    /// Timeline of events.
    pub timeline: Vec<TimelineEvent>,
    /// Additional notes.
    pub notes: Vec<String>,
}

/// Report metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportMetadata {
    /// Report version.
    pub version: String,
    /// Generation timestamp.
    pub generated_at: DateTime<Utc>,
    /// Test environment.
    pub environment: String,
    /// Host information.
    pub host_info: HostInfo,
    /// Test configuration used.
    pub config: HashMap<String, String>,
}

/// Host information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    /// Operating system.
    pub os: String,
    /// CPU count.
    pub cpu_count: usize,
    /// Total memory in bytes.
    pub total_memory_bytes: u64,
    /// Hostname.
    pub hostname: String,
}

impl HostInfo {
    /// Collect host information.
    pub fn collect() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            cpu_count: num_cpus::get(),
            total_memory_bytes: Self::get_total_memory(),
            hostname: hostname::get()
                .map(|h| h.to_string_lossy().to_string())
                .unwrap_or_else(|_| "unknown".to_string()),
        }
    }

    #[cfg(target_os = "linux")]
    fn get_total_memory() -> u64 {
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("MemTotal:"))
                    .and_then(|l| {
                        l.split_whitespace()
                            .nth(1)
                            .and_then(|v| v.parse::<u64>().ok())
                            .map(|kb| kb * 1024)
                    })
            })
            .unwrap_or(0)
    }

    #[cfg(not(target_os = "linux"))]
    fn get_total_memory() -> u64 {
        // Placeholder for other OSes
        0
    }
}

/// Individual target result entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetResultEntry {
    /// Target name.
    pub name: String,
    /// Expected value/threshold.
    pub expected: String,
    /// Actual value.
    pub actual: String,
    /// Whether the target passed.
    pub passed: bool,
    /// Comparison operator used.
    pub comparison: String,
}

/// Timeline event during test execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    /// Event timestamp (relative to test start).
    #[serde(with = "humantime_serde")]
    pub timestamp: Duration,
    /// Event type.
    pub event_type: EventType,
    /// Event message.
    pub message: String,
    /// Associated metrics at this point.
    pub metrics: Option<HashMap<String, f64>>,
}

/// Event types in the timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    TestStart,
    TestEnd,
    RampUpComplete,
    TargetReached,
    TargetMissed,
    Error,
    Warning,
    Milestone,
    Snapshot,
}

impl TestReport {
    /// Create a new test report builder.
    pub fn builder(name: impl Into<String>) -> TestReportBuilder {
        TestReportBuilder::new(name)
    }

    /// Export report as JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Export report as YAML.
    pub fn to_yaml(&self) -> Result<String, serde_yaml::Error> {
        serde_yaml::to_string(self)
    }

    /// Export report as markdown.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str(&format!("# Test Report: {}\n\n", self.summary.name));
        md.push_str(&format!(
            "**Description:** {}\n\n",
            self.summary.description
        ));
        md.push_str(&format!(
            "**Status:** {}\n\n",
            if self.summary.passed {
                "PASSED"
            } else {
                "FAILED"
            }
        ));

        md.push_str("## Summary\n\n");
        md.push_str(&format!("| Metric | Value |\n"));
        md.push_str(&format!("|--------|-------|\n"));
        md.push_str(&format!("| Duration | {:?} |\n", self.summary.duration));
        md.push_str(&format!("| Start Time | {} |\n", self.summary.start_time));
        md.push_str(&format!("| End Time | {} |\n", self.summary.end_time));
        md.push_str(&format!(
            "| Targets Passed | {}/{} |\n",
            self.summary.targets_passed, self.summary.targets_checked
        ));

        md.push_str("\n## Metrics\n\n");
        md.push_str(&format!("| Metric | Value |\n"));
        md.push_str(&format!("|--------|-------|\n"));
        md.push_str(&format!(
            "| Total Requests | {} |\n",
            self.metrics.total_requests
        ));
        md.push_str(&format!(
            "| Successful | {} |\n",
            self.metrics.successful_requests
        ));
        md.push_str(&format!("| Failed | {} |\n", self.metrics.failed_requests));
        md.push_str(&format!("| Avg TPS | {} |\n", self.metrics.avg_tps));
        md.push_str(&format!("| Peak TPS | {} |\n", self.metrics.peak_tps));
        md.push_str(&format!(
            "| P50 Latency | {:.2}ms |\n",
            self.metrics.p50_latency_ms
        ));
        md.push_str(&format!(
            "| P95 Latency | {:.2}ms |\n",
            self.metrics.p95_latency_ms
        ));
        md.push_str(&format!(
            "| P99 Latency | {:.2}ms |\n",
            self.metrics.p99_latency_ms
        ));
        md.push_str(&format!(
            "| Error Rate | {:.2}% |\n",
            self.metrics.error_rate * 100.0
        ));
        md.push_str(&format!(
            "| Peak Memory | {:.2}MB |\n",
            self.metrics.memory_peak_mb
        ));

        md.push_str("\n## Target Results\n\n");
        md.push_str(&format!("| Target | Expected | Actual | Status |\n"));
        md.push_str(&format!("|--------|----------|--------|--------|\n"));
        for target in &self.target_results {
            let status = if target.passed { "PASSED" } else { "FAILED" };
            md.push_str(&format!(
                "| {} | {} {} | {} | {} |\n",
                target.name, target.comparison, target.expected, target.actual, status
            ));
        }

        if !self.summary.failure_reasons.is_empty() {
            md.push_str("\n## Failure Reasons\n\n");
            for reason in &self.summary.failure_reasons {
                md.push_str(&format!("- {}\n", reason));
            }
        }

        md.push_str("\n## Environment\n\n");
        md.push_str(&format!("- **OS:** {}\n", self.metadata.host_info.os));
        md.push_str(&format!(
            "- **CPUs:** {}\n",
            self.metadata.host_info.cpu_count
        ));
        md.push_str(&format!(
            "- **Memory:** {:.2}GB\n",
            self.metadata.host_info.total_memory_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        ));
        md.push_str(&format!(
            "- **Host:** {}\n",
            self.metadata.host_info.hostname
        ));

        md
    }

    /// Save report to file.
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let content = if path.ends_with(".json") {
            self.to_json()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
        } else if path.ends_with(".yaml") || path.ends_with(".yml") {
            self.to_yaml()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
        } else if path.ends_with(".md") {
            self.to_markdown()
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Unsupported file format",
            ));
        };

        std::fs::write(path, content)
    }
}

/// Builder for TestReport.
pub struct TestReportBuilder {
    name: String,
    description: String,
    environment: String,
    config: HashMap<String, String>,
    start_time: DateTime<Utc>,
    target_results: Vec<TargetResultEntry>,
    timeline: Vec<TimelineEvent>,
    notes: Vec<String>,
}

impl TestReportBuilder {
    /// Create a new builder.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            environment: "development".to_string(),
            config: HashMap::new(),
            start_time: Utc::now(),
            target_results: Vec::new(),
            timeline: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Set description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Set environment.
    pub fn environment(mut self, env: impl Into<String>) -> Self {
        self.environment = env.into();
        self
    }

    /// Add configuration entry.
    pub fn config(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.insert(key.into(), value.into());
        self
    }

    /// Add a target result.
    pub fn add_target_result(
        mut self,
        name: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
        comparison: impl Into<String>,
        passed: bool,
    ) -> Self {
        self.target_results.push(TargetResultEntry {
            name: name.into(),
            expected: expected.into(),
            actual: actual.into(),
            comparison: comparison.into(),
            passed,
        });
        self
    }

    /// Add a timeline event.
    pub fn add_event(
        mut self,
        timestamp: Duration,
        event_type: EventType,
        message: impl Into<String>,
    ) -> Self {
        self.timeline.push(TimelineEvent {
            timestamp,
            event_type,
            message: message.into(),
            metrics: None,
        });
        self
    }

    /// Add a note.
    pub fn add_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Build the report.
    pub fn build(self, metrics: TestMetrics, passed: bool, duration: Duration) -> TestReport {
        let end_time = Utc::now();
        let targets_passed = self.target_results.iter().filter(|t| t.passed).count();

        let failure_reasons: Vec<String> = self
            .target_results
            .iter()
            .filter(|t| !t.passed)
            .map(|t| {
                format!(
                    "{}: expected {} {}, got {}",
                    t.name, t.comparison, t.expected, t.actual
                )
            })
            .collect();

        TestReport {
            metadata: ReportMetadata {
                version: "1.0.0".to_string(),
                generated_at: end_time,
                environment: self.environment,
                host_info: HostInfo::collect(),
                config: self.config,
            },
            summary: TestSummary {
                name: self.name,
                description: self.description,
                passed,
                duration,
                start_time: self.start_time,
                end_time,
                targets_checked: self.target_results.len(),
                targets_passed,
                failure_reasons,
            },
            metrics,
            target_results: self.target_results,
            timeline: self.timeline,
            notes: self.notes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_test_metrics_healthy() {
        let healthy = TestMetrics {
            total_requests: 10000,
            successful_requests: 9900,
            failed_requests: 100,
            total_connections: 100,
            peak_connections: 100,
            avg_tps: 1000,
            peak_tps: 1500,
            p50_latency_ms: 5.0,
            p95_latency_ms: 15.0,
            p99_latency_ms: 30.0,
            error_rate: 0.01,
            memory_peak_mb: 256.0,
        };
        assert!(healthy.is_healthy());

        let unhealthy = TestMetrics {
            error_rate: 0.10,
            ..healthy.clone()
        };
        assert!(!unhealthy.is_healthy());
    }

    #[test]
    fn test_report_builder() {
        let metrics = TestMetrics {
            total_requests: 10000,
            successful_requests: 9900,
            failed_requests: 100,
            total_connections: 100,
            peak_connections: 100,
            avg_tps: 1000,
            peak_tps: 1500,
            p50_latency_ms: 5.0,
            p95_latency_ms: 15.0,
            p99_latency_ms: 30.0,
            error_rate: 0.01,
            memory_peak_mb: 256.0,
        };

        let report = TestReport::builder("Test Run")
            .description("Performance test")
            .environment("test")
            .config("connections", "100")
            .add_target_result("Min TPS", "1000", "1000", ">=", true)
            .add_note("Test completed successfully")
            .build(metrics, true, Duration::from_secs(60));

        assert!(report.summary.passed);
        assert_eq!(report.summary.targets_passed, 1);
    }

    #[test]
    fn test_report_to_markdown() {
        let metrics = TestMetrics {
            total_requests: 10000,
            successful_requests: 9900,
            failed_requests: 100,
            total_connections: 100,
            peak_connections: 100,
            avg_tps: 1000,
            peak_tps: 1500,
            p50_latency_ms: 5.0,
            p95_latency_ms: 15.0,
            p99_latency_ms: 30.0,
            error_rate: 0.01,
            memory_peak_mb: 256.0,
        };

        let report = TestReport::builder("Test").build(metrics, true, Duration::from_secs(60));

        let md = report.to_markdown();
        assert!(md.contains("# Test Report: Test"));
        assert!(md.contains("PASSED"));
    }

    #[test]
    fn test_host_info_collect() {
        let info = HostInfo::collect();
        assert!(!info.os.is_empty());
        assert!(info.cpu_count > 0);
    }
}
