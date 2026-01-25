//! Profiling report generation and export.
//!
//! Provides utilities for generating, formatting, and exporting
//! profiling reports in various formats.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{MemoryReport, ProfileReport};

/// Report format options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReportFormat {
    /// JSON format.
    Json,
    /// Human-readable text format.
    Text,
    /// Markdown format.
    Markdown,
    /// CSV format (for data analysis).
    Csv,
}

/// Report exporter for various formats.
pub struct ReportExporter;

impl ReportExporter {
    /// Export a profile report to a string.
    pub fn export_to_string(report: &ProfileReport, format: ReportFormat) -> String {
        match format {
            ReportFormat::Json => Self::to_json(report),
            ReportFormat::Text => Self::to_text(report),
            ReportFormat::Markdown => Self::to_markdown(report),
            ReportFormat::Csv => Self::to_csv(report),
        }
    }

    /// Export a profile report to a file.
    pub fn export_to_file<P: AsRef<Path>>(
        report: &ProfileReport,
        path: P,
        format: ReportFormat,
    ) -> std::io::Result<()> {
        let content = Self::export_to_string(report, format);
        let mut file = std::fs::File::create(path)?;
        file.write_all(content.as_bytes())?;
        Ok(())
    }

    /// Convert report to JSON format.
    fn to_json(report: &ProfileReport) -> String {
        serde_json::to_string_pretty(report).unwrap_or_else(|e| format!("Error: {}", e))
    }

    /// Convert report to human-readable text format.
    fn to_text(report: &ProfileReport) -> String {
        report.to_summary()
    }

    /// Convert report to Markdown format.
    fn to_markdown(report: &ProfileReport) -> String {
        let mut md = String::new();

        md.push_str("# TRAP Simulator Profiling Report\n\n");
        md.push_str(&format!(
            "**Generated:** {}\n\n",
            report.generated_at.format("%Y-%m-%d %H:%M:%S UTC")
        ));
        md.push_str(&format!(
            "**Status:** {}\n\n",
            if report.is_running {
                "🟢 Running"
            } else {
                "🔴 Stopped"
            }
        ));

        md.push_str("## Memory Usage\n\n");
        md.push_str("| Metric | Value |\n");
        md.push_str("|--------|-------|\n");
        md.push_str(&format!(
            "| Current Memory | {} MB |\n",
            report.memory.current_bytes / 1024 / 1024
        ));
        md.push_str(&format!(
            "| Peak Memory | {} MB |\n",
            report.memory.peak_bytes / 1024 / 1024
        ));
        md.push_str(&format!(
            "| Total Allocated | {} MB |\n",
            report.memory.total_allocated / 1024 / 1024
        ));
        md.push_str(&format!(
            "| Total Deallocated | {} MB |\n",
            report.memory.total_deallocated / 1024 / 1024
        ));
        md.push_str(&format!(
            "| Allocation Count | {} |\n",
            report.memory.allocation_count
        ));
        md.push_str(&format!(
            "| Deallocation Count | {} |\n",
            report.memory.deallocation_count
        ));

        if let Some(rate) = report.memory.growth_rate_bytes_per_sec {
            md.push_str(&format!(
                "| Growth Rate | {:.2} KB/s |\n",
                rate / 1024.0
            ));
        }

        if !report.memory.regions.is_empty() {
            md.push_str("\n## Memory Regions\n\n");
            md.push_str("| Region | Current | Peak | Allocations |\n");
            md.push_str("|--------|---------|------|-------------|\n");

            let mut regions: Vec<_> = report.memory.regions.iter().collect();
            regions.sort_by(|a, b| b.1.current_bytes.cmp(&a.1.current_bytes));

            for (name, region) in regions {
                md.push_str(&format!(
                    "| {} | {} KB | {} KB | {} |\n",
                    name,
                    region.current_bytes / 1024,
                    region.peak_bytes / 1024,
                    region.allocation_count
                ));
            }
        }

        if !report.leak_warnings.is_empty() {
            md.push_str("\n## ⚠️ Leak Warnings\n\n");
            for warning in &report.leak_warnings {
                let emoji = match warning.severity {
                    super::LeakSeverity::Low => "ℹ️",
                    super::LeakSeverity::Medium => "⚠️",
                    super::LeakSeverity::High => "🔶",
                    super::LeakSeverity::Critical => "🔴",
                };
                md.push_str(&format!(
                    "- {} **{}** ({}): {}\n",
                    emoji, warning.region, warning.severity, warning.message
                ));
                md.push_str(&format!(
                    "  - Growth rate: {:.1}%/min\n",
                    warning.growth_rate_per_minute
                ));
                md.push_str(&format!(
                    "  - Confidence: {:.0}%\n",
                    warning.confidence * 100.0
                ));
            }
        } else {
            md.push_str("\n## ✅ No Leak Warnings\n\n");
            md.push_str("No potential memory leaks detected.\n");
        }

        md
    }

    /// Convert report to CSV format.
    fn to_csv(report: &ProfileReport) -> String {
        let mut csv = String::new();

        // Header
        csv.push_str("region,current_bytes,peak_bytes,total_allocated,total_deallocated,allocation_count,deallocation_count\n");

        // Global row
        csv.push_str(&format!(
            "global,{},{},{},{},{},{}\n",
            report.memory.current_bytes,
            report.memory.peak_bytes,
            report.memory.total_allocated,
            report.memory.total_deallocated,
            report.memory.allocation_count,
            report.memory.deallocation_count
        ));

        // Per-region rows
        for (name, region) in &report.memory.regions {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{}\n",
                name,
                region.current_bytes,
                region.peak_bytes,
                region.total_allocated,
                region.total_deallocated,
                region.allocation_count,
                region.deallocation_count
            ));
        }

        csv
    }
}

/// Memory report builder for custom reports.
#[derive(Default)]
pub struct MemoryReportBuilder {
    current_bytes: u64,
    peak_bytes: u64,
    total_allocated: u64,
    total_deallocated: u64,
    allocation_count: u64,
    deallocation_count: u64,
    growth_rate: Option<f64>,
}

impl MemoryReportBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set current bytes.
    pub fn current_bytes(mut self, bytes: u64) -> Self {
        self.current_bytes = bytes;
        self
    }

    /// Set peak bytes.
    pub fn peak_bytes(mut self, bytes: u64) -> Self {
        self.peak_bytes = bytes;
        self
    }

    /// Set total allocated.
    pub fn total_allocated(mut self, bytes: u64) -> Self {
        self.total_allocated = bytes;
        self
    }

    /// Set total deallocated.
    pub fn total_deallocated(mut self, bytes: u64) -> Self {
        self.total_deallocated = bytes;
        self
    }

    /// Set allocation count.
    pub fn allocation_count(mut self, count: u64) -> Self {
        self.allocation_count = count;
        self
    }

    /// Set deallocation count.
    pub fn deallocation_count(mut self, count: u64) -> Self {
        self.deallocation_count = count;
        self
    }

    /// Set growth rate.
    pub fn growth_rate(mut self, rate: f64) -> Self {
        self.growth_rate = Some(rate);
        self
    }

    /// Build the memory report.
    pub fn build(self) -> MemoryReport {
        MemoryReport {
            current_bytes: self.current_bytes,
            peak_bytes: self.peak_bytes,
            total_allocated: self.total_allocated,
            total_deallocated: self.total_deallocated,
            allocation_count: self.allocation_count,
            deallocation_count: self.deallocation_count,
            regions: std::collections::HashMap::new(),
            growth_rate_bytes_per_sec: self.growth_rate,
            snapshot_count: 0,
        }
    }
}

/// Comparison between two profile reports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportComparison {
    /// First report timestamp.
    pub first_timestamp: chrono::DateTime<chrono::Utc>,

    /// Second report timestamp.
    pub second_timestamp: chrono::DateTime<chrono::Utc>,

    /// Time difference.
    pub time_diff_secs: i64,

    /// Memory difference in bytes.
    pub memory_diff_bytes: i64,

    /// Memory change percentage.
    pub memory_change_percent: f64,

    /// Allocation count difference.
    pub allocation_diff: i64,

    /// New leak warnings in second report.
    pub new_warnings: usize,

    /// Resolved warnings (in first but not in second).
    pub resolved_warnings: usize,
}

impl ReportComparison {
    /// Compare two profile reports.
    pub fn compare(first: &ProfileReport, second: &ProfileReport) -> Self {
        let memory_diff =
            second.memory.current_bytes as i64 - first.memory.current_bytes as i64;
        let memory_change_percent = if first.memory.current_bytes > 0 {
            (memory_diff as f64 / first.memory.current_bytes as f64) * 100.0
        } else {
            0.0
        };

        let first_warnings: std::collections::HashSet<_> = first
            .leak_warnings
            .iter()
            .map(|w| &w.region)
            .collect();
        let second_warnings: std::collections::HashSet<_> = second
            .leak_warnings
            .iter()
            .map(|w| &w.region)
            .collect();

        let new_warnings = second_warnings.difference(&first_warnings).count();
        let resolved_warnings = first_warnings.difference(&second_warnings).count();

        Self {
            first_timestamp: first.generated_at,
            second_timestamp: second.generated_at,
            time_diff_secs: (second.generated_at - first.generated_at).num_seconds(),
            memory_diff_bytes: memory_diff,
            memory_change_percent,
            allocation_diff: second.memory.allocation_count as i64
                - first.memory.allocation_count as i64,
            new_warnings,
            resolved_warnings,
        }
    }

    /// Check if memory usage is increasing.
    pub fn is_memory_increasing(&self) -> bool {
        self.memory_diff_bytes > 0
    }

    /// Get human-readable summary.
    pub fn summary(&self) -> String {
        let direction = if self.memory_diff_bytes > 0 {
            "increased"
        } else if self.memory_diff_bytes < 0 {
            "decreased"
        } else {
            "unchanged"
        };

        format!(
            "Memory {} by {} bytes ({:.1}%) over {} seconds. {} new warnings, {} resolved.",
            direction,
            self.memory_diff_bytes.abs(),
            self.memory_change_percent.abs(),
            self.time_diff_secs,
            self.new_warnings,
            self.resolved_warnings
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiling::{LeakSeverity, LeakWarning, ProfilerConfig};

    fn create_test_report(current_bytes: u64, warnings: Vec<LeakWarning>) -> ProfileReport {
        ProfileReport {
            generated_at: chrono::Utc::now(),
            config: ProfilerConfig::default(),
            memory: MemoryReport {
                current_bytes,
                peak_bytes: current_bytes + 1000,
                total_allocated: current_bytes * 2,
                total_deallocated: current_bytes,
                allocation_count: 100,
                deallocation_count: 50,
                regions: std::collections::HashMap::new(),
                growth_rate_bytes_per_sec: Some(100.0),
                snapshot_count: 10,
            },
            leak_warnings: warnings,
            is_running: true,
        }
    }

    #[test]
    fn test_export_json() {
        let report = create_test_report(1024, vec![]);
        let json = ReportExporter::export_to_string(&report, ReportFormat::Json);
        assert!(json.contains("current_bytes"));
        assert!(json.contains("1024"));
    }

    #[test]
    fn test_export_text() {
        let report = create_test_report(1024 * 1024, vec![]);
        let text = ReportExporter::export_to_string(&report, ReportFormat::Text);
        assert!(text.contains("Memory Usage"));
        assert!(text.contains("MB"));
    }

    #[test]
    fn test_export_markdown() {
        let report = create_test_report(1024 * 1024, vec![]);
        let md = ReportExporter::export_to_string(&report, ReportFormat::Markdown);
        assert!(md.contains("# TRAP Simulator"));
        assert!(md.contains("| Metric | Value |"));
    }

    #[test]
    fn test_export_csv() {
        let report = create_test_report(1024, vec![]);
        let csv = ReportExporter::export_to_string(&report, ReportFormat::Csv);
        assert!(csv.contains("region,current_bytes"));
        assert!(csv.contains("global,1024"));
    }

    #[test]
    fn test_memory_report_builder() {
        let report = MemoryReportBuilder::new()
            .current_bytes(1000)
            .peak_bytes(2000)
            .allocation_count(10)
            .build();

        assert_eq!(report.current_bytes, 1000);
        assert_eq!(report.peak_bytes, 2000);
        assert_eq!(report.allocation_count, 10);
    }

    #[test]
    fn test_report_comparison() {
        let report1 = create_test_report(1000, vec![]);
        let report2 = create_test_report(1500, vec![]);

        let comparison = ReportComparison::compare(&report1, &report2);
        assert!(comparison.is_memory_increasing());
        assert_eq!(comparison.memory_diff_bytes, 500);
    }

    #[test]
    fn test_report_comparison_with_warnings() {
        let warning = LeakWarning {
            region: "test".to_string(),
            severity: LeakSeverity::Medium,
            message: "test warning".to_string(),
            growth_rate_per_minute: 5.0,
            current_bytes: 1000,
            samples_analyzed: 10,
            confidence: 0.8,
        };

        let report1 = create_test_report(1000, vec![]);
        let report2 = create_test_report(1500, vec![warning]);

        let comparison = ReportComparison::compare(&report1, &report2);
        assert_eq!(comparison.new_warnings, 1);
        assert_eq!(comparison.resolved_warnings, 0);
    }
}
