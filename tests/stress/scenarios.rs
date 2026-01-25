//! Pre-defined stress test scenarios.
//!
//! Common stress test scenarios for different use cases.

use super::load_generator::{LoadConfig, LoadGenerator, ModbusLoadGenerator};
use super::metrics::{MetricsCollector, MetricsSummary, Thresholds};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

/// Stress test scenario configuration.
#[derive(Debug, Clone)]
pub struct StressScenario {
    /// Scenario name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Load configuration.
    pub load_config: LoadConfig,
    /// Performance thresholds.
    pub thresholds: Thresholds,
}

impl StressScenario {
    /// Create a baseline performance test.
    pub fn baseline() -> Self {
        Self {
            name: "Baseline Performance".into(),
            description: "Establish baseline performance metrics".into(),
            load_config: LoadConfig {
                target_rps: 100,
                concurrency: 10,
                duration: Duration::from_secs(60),
                ramp_up: Duration::from_secs(5),
                ..Default::default()
            },
            thresholds: Thresholds::default(),
        }
    }

    /// Create a load test scenario.
    pub fn load_test() -> Self {
        Self {
            name: "Load Test".into(),
            description: "Test system under expected production load".into(),
            load_config: LoadConfig {
                target_rps: 1000,
                concurrency: 50,
                duration: Duration::from_secs(300),
                ramp_up: Duration::from_secs(30),
                ..Default::default()
            },
            thresholds: Thresholds::default(),
        }
    }

    /// Create a stress test scenario.
    pub fn stress_test() -> Self {
        Self {
            name: "Stress Test".into(),
            description: "Test system under extreme load".into(),
            load_config: LoadConfig::stress_test(),
            thresholds: Thresholds::relaxed(),
        }
    }

    /// Create a spike test scenario.
    pub fn spike_test() -> Self {
        Self {
            name: "Spike Test".into(),
            description: "Test system response to sudden load spikes".into(),
            load_config: LoadConfig {
                target_rps: 10000,
                concurrency: 200,
                duration: Duration::from_secs(120),
                ramp_up: Duration::from_secs(1), // Very fast ramp-up
                ..Default::default()
            },
            thresholds: Thresholds::relaxed(),
        }
    }

    /// Create a soak test scenario.
    pub fn soak_test() -> Self {
        Self {
            name: "Soak Test".into(),
            description: "Test system stability over extended period".into(),
            load_config: LoadConfig {
                target_rps: 500,
                concurrency: 25,
                duration: Duration::from_secs(3600), // 1 hour
                ramp_up: Duration::from_secs(60),
                ..Default::default()
            },
            thresholds: Thresholds::default(),
        }
    }

    /// Create a capacity test scenario.
    pub fn capacity_test() -> Self {
        Self {
            name: "Capacity Test".into(),
            description: "Find the maximum sustainable throughput".into(),
            load_config: LoadConfig {
                target_rps: 100000,
                concurrency: 1000,
                duration: Duration::from_secs(600),
                ramp_up: Duration::from_secs(120),
                ..Default::default()
            },
            thresholds: Thresholds {
                min_success_rate: 90.0,
                max_p95_latency: Duration::from_secs(1),
                max_p99_latency: Duration::from_secs(5),
                min_rps: 50000.0,
            },
        }
    }
}

/// Modbus-specific stress scenarios.
pub struct ModbusStressScenarios;

impl ModbusStressScenarios {
    /// Test reading holding registers under load.
    pub async fn read_holding_registers(
        addr: SocketAddr,
        scenario: &StressScenario,
    ) -> MetricsSummary {
        let generator = ModbusLoadGenerator::new(
            addr,
            vec![1, 2, 3, 4, 5], // Multiple unit IDs
            (0, 100),            // Register range
        );

        let load_gen = LoadGenerator::new(scenario.load_config.clone(), generator);
        load_gen.run().await;
        load_gen.stats()
    }

    /// Test mixed read/write operations.
    pub async fn mixed_operations(addr: SocketAddr, scenario: &StressScenario) -> MetricsSummary {
        // TODO: Implement mixed read/write generator
        Self::read_holding_registers(addr, scenario).await
    }

    /// Test multi-unit concurrent access.
    pub async fn multi_unit_access(addr: SocketAddr, scenario: &StressScenario) -> MetricsSummary {
        let generator = ModbusLoadGenerator::new(
            addr,
            (1..=100).collect(), // 100 unit IDs
            (0, 1000),           // Larger register range
        );

        let load_gen = LoadGenerator::new(scenario.load_config.clone(), generator);
        load_gen.run().await;
        load_gen.stats()
    }

    /// Test large register block reads.
    pub async fn large_block_reads(addr: SocketAddr, scenario: &StressScenario) -> MetricsSummary {
        let generator = ModbusLoadGenerator::new(
            addr,
            vec![1], // Single unit
            (0, 125), // Maximum block size
        );

        let load_gen = LoadGenerator::new(scenario.load_config.clone(), generator);
        load_gen.run().await;
        load_gen.stats()
    }
}

/// Stress test runner.
pub struct StressTestRunner {
    scenarios: Vec<StressScenario>,
    results: Vec<(String, MetricsSummary, bool)>,
}

impl StressTestRunner {
    pub fn new() -> Self {
        Self {
            scenarios: Vec::new(),
            results: Vec::new(),
        }
    }

    /// Add a scenario to run.
    pub fn add_scenario(&mut self, scenario: StressScenario) {
        self.scenarios.push(scenario);
    }

    /// Run all scenarios against a target.
    pub async fn run_modbus(&mut self, addr: SocketAddr) {
        for scenario in &self.scenarios {
            println!("\n{}", "=".repeat(60));
            println!("Running: {}", scenario.name);
            println!("{}", scenario.description);
            println!("{}", "=".repeat(60));

            let summary = ModbusStressScenarios::read_holding_registers(addr, scenario).await;
            let passed = summary.passed(&scenario.thresholds);

            summary.print();

            if passed {
                println!("\n✓ PASSED");
            } else {
                println!("\n✗ FAILED");
            }

            self.results
                .push((scenario.name.clone(), summary, passed));
        }
    }

    /// Print final report.
    pub fn print_report(&self) {
        println!("\n{}", "=".repeat(60));
        println!("STRESS TEST REPORT");
        println!("{}", "=".repeat(60));

        let mut total = 0;
        let mut passed = 0;

        for (name, summary, result) in &self.results {
            total += 1;
            if *result {
                passed += 1;
            }

            let status = if *result { "✓ PASS" } else { "✗ FAIL" };
            println!(
                "\n{} {} (RPS: {:.1}, P95: {:?})",
                status, name, summary.overall_rps, summary.latency.p95
            );
        }

        println!("\n{}", "-".repeat(60));
        println!("Total: {} | Passed: {} | Failed: {}", total, passed, total - passed);
        println!("{}", "=".repeat(60));
    }

    /// Check if all tests passed.
    pub fn all_passed(&self) -> bool {
        self.results.iter().all(|(_, _, passed)| *passed)
    }
}

impl Default for StressTestRunner {
    fn default() -> Self {
        Self::new()
    }
}
