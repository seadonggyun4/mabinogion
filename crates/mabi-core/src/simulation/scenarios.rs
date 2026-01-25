//! Pre-built simulation scenarios.
//!
//! Provides ready-to-use scenarios for common testing patterns.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::failure::{FailureConfig, FailureSchedule, FailureType};
use super::load::LoadPattern;
use super::memory_sim::MemoryPattern;
use super::scale::ScaleConfig;
use super::SimulationConfig;

/// Pre-built simulation scenarios.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Scenario {
    /// Quick smoke test.
    SmokeTest,

    /// Basic functionality test.
    BasicFunctionality,

    /// Stress test with high load.
    StressTest,

    /// Memory leak detection scenario.
    MemoryLeakDetection,

    /// Chaos engineering scenario.
    ChaosEngineering,

    /// Long-running stability test.
    Endurance,

    /// Scale-up test (increasing load).
    ScaleUp,

    /// Production-like daily traffic.
    ProductionSimulation,

    /// Peak load test.
    PeakLoad,

    /// Failure recovery test.
    FailureRecovery,

    /// Custom scenario.
    Custom(Box<SimulationConfig>),
}

impl Scenario {
    /// Convert scenario to simulation configuration.
    pub fn to_config(&self) -> SimulationConfig {
        match self {
            Self::SmokeTest => smoke_test_config(),
            Self::BasicFunctionality => basic_functionality_config(),
            Self::StressTest => stress_test_config(),
            Self::MemoryLeakDetection => memory_leak_detection_config(),
            Self::ChaosEngineering => chaos_engineering_config(),
            Self::Endurance => endurance_config(),
            Self::ScaleUp => scale_up_config(),
            Self::ProductionSimulation => production_simulation_config(),
            Self::PeakLoad => peak_load_config(),
            Self::FailureRecovery => failure_recovery_config(),
            Self::Custom(config) => (**config).clone(),
        }
    }

    /// Get scenario name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::SmokeTest => "Smoke Test",
            Self::BasicFunctionality => "Basic Functionality",
            Self::StressTest => "Stress Test",
            Self::MemoryLeakDetection => "Memory Leak Detection",
            Self::ChaosEngineering => "Chaos Engineering",
            Self::Endurance => "Endurance Test",
            Self::ScaleUp => "Scale-Up Test",
            Self::ProductionSimulation => "Production Simulation",
            Self::PeakLoad => "Peak Load Test",
            Self::FailureRecovery => "Failure Recovery",
            Self::Custom(_) => "Custom Scenario",
        }
    }

    /// Get scenario description.
    pub fn description(&self) -> &'static str {
        match self {
            Self::SmokeTest => "Quick test to verify basic system operation",
            Self::BasicFunctionality => "Tests core functionality with moderate load",
            Self::StressTest => "High load test to find breaking points",
            Self::MemoryLeakDetection => "Extended test to detect memory leaks",
            Self::ChaosEngineering => "Random failure injection to test resilience",
            Self::Endurance => "Long-running test for stability verification",
            Self::ScaleUp => "Gradually increasing load to test scaling",
            Self::ProductionSimulation => "Simulates realistic production traffic patterns",
            Self::PeakLoad => "Tests maximum expected load handling",
            Self::FailureRecovery => "Tests system recovery from various failures",
            Self::Custom(_) => "User-defined custom scenario",
        }
    }

    /// Get estimated duration.
    pub fn estimated_duration(&self) -> Duration {
        match self {
            Self::SmokeTest => Duration::from_secs(60),
            Self::BasicFunctionality => Duration::from_secs(300),
            Self::StressTest => Duration::from_secs(600),
            Self::MemoryLeakDetection => Duration::from_secs(3600),
            Self::ChaosEngineering => Duration::from_secs(1800),
            Self::Endurance => Duration::from_secs(86400), // 24 hours
            Self::ScaleUp => Duration::from_secs(1200),
            Self::ProductionSimulation => Duration::from_secs(86400), // 24 hours
            Self::PeakLoad => Duration::from_secs(300),
            Self::FailureRecovery => Duration::from_secs(900),
            Self::Custom(config) => config.max_duration,
        }
    }

    /// List all built-in scenarios.
    pub fn all_builtin() -> Vec<Self> {
        vec![
            Self::SmokeTest,
            Self::BasicFunctionality,
            Self::StressTest,
            Self::MemoryLeakDetection,
            Self::ChaosEngineering,
            Self::Endurance,
            Self::ScaleUp,
            Self::ProductionSimulation,
            Self::PeakLoad,
            Self::FailureRecovery,
        ]
    }
}

/// Quick smoke test - 1 minute, minimal load.
fn smoke_test_config() -> SimulationConfig {
    SimulationConfig {
        name: "Smoke Test".to_string(),
        description: "Quick verification of basic system operation".to_string(),
        scale: ScaleConfig::tiny(),
        memory_pattern: MemoryPattern::Steady,
        load_pattern: LoadPattern::steady(100),
        failure_config: None,
        max_duration: Duration::from_secs(60),
        report_interval: Duration::from_secs(10),
        detailed_metrics: true,
        seed: None,
    }
}

/// Basic functionality test - 5 minutes, moderate load.
fn basic_functionality_config() -> SimulationConfig {
    SimulationConfig {
        name: "Basic Functionality".to_string(),
        description: "Core functionality test with moderate load".to_string(),
        scale: ScaleConfig::small(),
        memory_pattern: MemoryPattern::Steady,
        load_pattern: LoadPattern::ramp(100, 1000, Duration::from_secs(60)),
        failure_config: None,
        max_duration: Duration::from_secs(300),
        report_interval: Duration::from_secs(30),
        detailed_metrics: true,
        seed: None,
    }
}

/// Stress test - 10 minutes, high load.
fn stress_test_config() -> SimulationConfig {
    SimulationConfig {
        name: "Stress Test".to_string(),
        description: "High load test to find breaking points".to_string(),
        scale: ScaleConfig::large(),
        memory_pattern: MemoryPattern::HighChurn,
        load_pattern: LoadPattern::ramp(1000, 100000, Duration::from_secs(300)),
        failure_config: None,
        max_duration: Duration::from_secs(600),
        report_interval: Duration::from_secs(30),
        detailed_metrics: true,
        seed: None,
    }
}

/// Memory leak detection - 1 hour, stable load.
fn memory_leak_detection_config() -> SimulationConfig {
    SimulationConfig {
        name: "Memory Leak Detection".to_string(),
        description: "Extended test to detect memory leaks".to_string(),
        scale: ScaleConfig::medium(),
        memory_pattern: MemoryPattern::GrowthOnly,
        load_pattern: LoadPattern::steady(5000),
        failure_config: None,
        max_duration: Duration::from_secs(3600),
        report_interval: Duration::from_secs(60),
        detailed_metrics: true,
        seed: None,
    }
}

/// Chaos engineering - 30 minutes, random failures.
fn chaos_engineering_config() -> SimulationConfig {
    SimulationConfig {
        name: "Chaos Engineering".to_string(),
        description: "Random failure injection to test resilience".to_string(),
        scale: ScaleConfig::medium(),
        memory_pattern: MemoryPattern::GrowthAndRelease,
        load_pattern: LoadPattern::wave(5000, 2000, Duration::from_secs(300)),
        failure_config: Some(FailureConfig {
            enabled: true,
            failure_rate: 0.05, // 5% failure rate
            failure_types: vec![
                FailureType::Timeout,
                FailureType::ConnectionReset,
                FailureType::ProtocolError,
                FailureType::DeviceOffline,
                FailureType::SlowResponse {
                    delay: Duration::from_secs(5),
                },
            ],
            schedule: FailureSchedule::Random {
                min_interval: Duration::from_secs(10),
                max_interval: Duration::from_secs(60),
            },
            cascade_probability: 0.1,
            recovery_time: Duration::from_secs(30),
            max_concurrent_failures: 10,
            seed: None,
        }),
        max_duration: Duration::from_secs(1800),
        report_interval: Duration::from_secs(60),
        detailed_metrics: true,
        seed: None,
    }
}

/// Endurance test - 24 hours, steady load.
fn endurance_config() -> SimulationConfig {
    SimulationConfig {
        name: "Endurance Test".to_string(),
        description: "Long-running test for stability verification".to_string(),
        scale: ScaleConfig::medium(),
        memory_pattern: MemoryPattern::GrowthAndRelease,
        load_pattern: LoadPattern::DailyTraffic {
            peak: 10000,
            off_peak: 2000,
            day_duration: Duration::from_secs(86400),
        },
        failure_config: Some(FailureConfig {
            enabled: true,
            failure_rate: 0.001, // 0.1% failure rate
            failure_types: vec![FailureType::Timeout, FailureType::ConnectionReset],
            schedule: FailureSchedule::Random {
                min_interval: Duration::from_secs(300),
                max_interval: Duration::from_secs(3600),
            },
            cascade_probability: 0.01,
            recovery_time: Duration::from_secs(60),
            max_concurrent_failures: 5,
            seed: None,
        }),
        max_duration: Duration::from_secs(86400), // 24 hours
        report_interval: Duration::from_secs(3600),
        detailed_metrics: true,
        seed: None,
    }
}

/// Scale-up test - 20 minutes, increasing load.
fn scale_up_config() -> SimulationConfig {
    SimulationConfig {
        name: "Scale-Up Test".to_string(),
        description: "Gradually increasing load to test scaling".to_string(),
        scale: ScaleConfig::xlarge(),
        memory_pattern: MemoryPattern::GrowthOnly,
        load_pattern: LoadPattern::Step {
            initial: 1000,
            steps: vec![
                (Duration::from_secs(120), 5000),
                (Duration::from_secs(240), 10000),
                (Duration::from_secs(360), 25000),
                (Duration::from_secs(480), 50000),
                (Duration::from_secs(600), 75000),
                (Duration::from_secs(720), 100000),
            ],
        },
        failure_config: None,
        max_duration: Duration::from_secs(1200),
        report_interval: Duration::from_secs(60),
        detailed_metrics: true,
        seed: None,
    }
}

/// Production simulation - 24 hours, realistic traffic.
fn production_simulation_config() -> SimulationConfig {
    SimulationConfig {
        name: "Production Simulation".to_string(),
        description: "Simulates realistic production traffic patterns".to_string(),
        scale: ScaleConfig::large(),
        memory_pattern: MemoryPattern::GrowthAndRelease,
        load_pattern: LoadPattern::DailyTraffic {
            peak: 50000,
            off_peak: 5000,
            day_duration: Duration::from_secs(86400),
        },
        failure_config: Some(FailureConfig {
            enabled: true,
            failure_rate: 0.0001, // 0.01% failure rate (realistic)
            failure_types: vec![
                FailureType::Timeout,
                FailureType::SlowResponse {
                    delay: Duration::from_millis(500),
                },
            ],
            schedule: FailureSchedule::Random {
                min_interval: Duration::from_secs(600),
                max_interval: Duration::from_secs(7200),
            },
            cascade_probability: 0.001,
            recovery_time: Duration::from_secs(30),
            max_concurrent_failures: 3,
            seed: None,
        }),
        max_duration: Duration::from_secs(86400), // 24 hours
        report_interval: Duration::from_secs(3600),
        detailed_metrics: true,
        seed: None,
    }
}

/// Peak load test - 5 minutes, maximum expected load.
fn peak_load_config() -> SimulationConfig {
    SimulationConfig {
        name: "Peak Load Test".to_string(),
        description: "Tests maximum expected load handling".to_string(),
        scale: ScaleConfig::xlarge(),
        memory_pattern: MemoryPattern::HighChurn,
        load_pattern: LoadPattern::Spike {
            baseline: 10000,
            peak: 100000,
            spike_duration: Duration::from_secs(120),
        },
        failure_config: None,
        max_duration: Duration::from_secs(300),
        report_interval: Duration::from_secs(15),
        detailed_metrics: true,
        seed: None,
    }
}

/// Failure recovery test - 15 minutes, sequential failures.
fn failure_recovery_config() -> SimulationConfig {
    SimulationConfig {
        name: "Failure Recovery".to_string(),
        description: "Tests system recovery from various failures".to_string(),
        scale: ScaleConfig::medium(),
        memory_pattern: MemoryPattern::Steady,
        load_pattern: LoadPattern::steady(5000),
        failure_config: Some(FailureConfig {
            enabled: true,
            failure_rate: 0.2, // 20% failure rate (high for testing recovery)
            failure_types: vec![
                FailureType::Timeout,
                FailureType::ConnectionReset,
                FailureType::ConnectionRefused,
                FailureType::ProtocolError,
                FailureType::DeviceOffline,
                FailureType::NetworkPartition,
                FailureType::OutOfMemory,
            ],
            schedule: FailureSchedule::Periodic {
                interval: Duration::from_secs(60),
                duration: Duration::from_secs(10),
            },
            cascade_probability: 0.3,
            recovery_time: Duration::from_secs(30),
            max_concurrent_failures: 20,
            seed: None,
        }),
        max_duration: Duration::from_secs(900),
        report_interval: Duration::from_secs(30),
        detailed_metrics: true,
        seed: None,
    }
}

/// Scenario builder for custom scenarios.
#[derive(Debug, Clone)]
pub struct ScenarioBuilder {
    config: SimulationConfig,
}

impl Default for ScenarioBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ScenarioBuilder {
    /// Create a new scenario builder with defaults.
    pub fn new() -> Self {
        Self {
            config: SimulationConfig::default(),
        }
    }

    /// Start from an existing scenario.
    pub fn from_scenario(scenario: Scenario) -> Self {
        Self {
            config: scenario.to_config(),
        }
    }

    /// Set scenario name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.config.name = name.into();
        self
    }

    /// Set scenario description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.config.description = description.into();
        self
    }

    /// Set scale configuration.
    pub fn scale(mut self, scale: ScaleConfig) -> Self {
        self.config.scale = scale;
        self
    }

    /// Set maximum duration.
    pub fn max_duration(mut self, duration: Duration) -> Self {
        self.config.max_duration = duration;
        self
    }

    /// Set load pattern.
    pub fn load_pattern(mut self, pattern: LoadPattern) -> Self {
        self.config.load_pattern = pattern;
        self
    }

    /// Set memory pattern.
    pub fn memory_pattern(mut self, pattern: MemoryPattern) -> Self {
        self.config.memory_pattern = pattern;
        self
    }

    /// Set failure config.
    pub fn failure_config(mut self, config: FailureConfig) -> Self {
        self.config.failure_config = Some(config);
        self
    }

    /// Set report interval.
    pub fn report_interval(mut self, interval: Duration) -> Self {
        self.config.report_interval = interval;
        self
    }

    /// Enable or disable detailed metrics collection.
    pub fn detailed_metrics(mut self, enabled: bool) -> Self {
        self.config.detailed_metrics = enabled;
        self
    }

    /// Set random seed for reproducibility.
    pub fn seed(mut self, seed: u64) -> Self {
        self.config.seed = Some(seed);
        self
    }

    /// Build the scenario.
    pub fn build(self) -> Scenario {
        Scenario::Custom(Box::new(self.config))
    }

    /// Get the configuration directly.
    pub fn into_config(self) -> SimulationConfig {
        self.config
    }
}

/// Scenario suite for running multiple scenarios.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioSuite {
    /// Suite name.
    pub name: String,

    /// Suite description.
    pub description: Option<String>,

    /// Scenarios in this suite.
    pub scenarios: Vec<Scenario>,

    /// Whether to stop on first failure.
    pub stop_on_failure: bool,

    /// Delay between scenarios.
    pub inter_scenario_delay: Duration,
}

impl ScenarioSuite {
    /// Create a new scenario suite.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            scenarios: Vec::new(),
            stop_on_failure: false,
            inter_scenario_delay: Duration::from_secs(60),
        }
    }

    /// Add a scenario to the suite.
    pub fn add_scenario(mut self, scenario: Scenario) -> Self {
        self.scenarios.push(scenario);
        self
    }

    /// Set description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set stop on failure behavior.
    pub fn stop_on_failure(mut self, stop: bool) -> Self {
        self.stop_on_failure = stop;
        self
    }

    /// Set delay between scenarios.
    pub fn inter_scenario_delay(mut self, delay: Duration) -> Self {
        self.inter_scenario_delay = delay;
        self
    }

    /// Get total estimated duration.
    pub fn estimated_duration(&self) -> Duration {
        let scenario_time: Duration = self.scenarios.iter().map(|s| s.estimated_duration()).sum();
        let delay_time = self.inter_scenario_delay * self.scenarios.len().saturating_sub(1) as u32;
        scenario_time + delay_time
    }

    /// Create a quick test suite.
    pub fn quick_test() -> Self {
        Self::new("Quick Test Suite")
            .description("Quick validation of core functionality")
            .add_scenario(Scenario::SmokeTest)
            .add_scenario(Scenario::BasicFunctionality)
            .stop_on_failure(true)
            .inter_scenario_delay(Duration::from_secs(10))
    }

    /// Create a full test suite.
    pub fn full_test() -> Self {
        Self::new("Full Test Suite")
            .description("Comprehensive testing of all scenarios")
            .add_scenario(Scenario::SmokeTest)
            .add_scenario(Scenario::BasicFunctionality)
            .add_scenario(Scenario::StressTest)
            .add_scenario(Scenario::MemoryLeakDetection)
            .add_scenario(Scenario::ChaosEngineering)
            .add_scenario(Scenario::ScaleUp)
            .add_scenario(Scenario::PeakLoad)
            .add_scenario(Scenario::FailureRecovery)
            .stop_on_failure(false)
            .inter_scenario_delay(Duration::from_secs(60))
    }

    /// Create a regression test suite.
    pub fn regression_test() -> Self {
        Self::new("Regression Test Suite")
            .description("Standard regression testing")
            .add_scenario(Scenario::SmokeTest)
            .add_scenario(Scenario::BasicFunctionality)
            .add_scenario(Scenario::StressTest)
            .stop_on_failure(true)
            .inter_scenario_delay(Duration::from_secs(30))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scenario_configs() {
        for scenario in Scenario::all_builtin() {
            let config = scenario.to_config();
            assert!(!config.name.is_empty());
            assert!(config.max_duration > Duration::ZERO);
        }
    }

    #[test]
    fn test_scenario_metadata() {
        for scenario in Scenario::all_builtin() {
            assert!(!scenario.name().is_empty());
            assert!(!scenario.description().is_empty());
            assert!(scenario.estimated_duration() > Duration::ZERO);
        }
    }

    #[test]
    fn test_scenario_builder() {
        let scenario = ScenarioBuilder::new()
            .name("Custom Test")
            .description("A custom test scenario")
            .scale(ScaleConfig::medium())
            .max_duration(Duration::from_secs(600))
            .load_pattern(LoadPattern::steady(5000))
            .detailed_metrics(true)
            .build();

        let config = scenario.to_config();
        assert_eq!(config.name, "Custom Test");
        assert_eq!(config.max_duration, Duration::from_secs(600));
    }

    #[test]
    fn test_scenario_builder_from_existing() {
        let scenario = ScenarioBuilder::from_scenario(Scenario::StressTest)
            .name("Modified Stress Test")
            .max_duration(Duration::from_secs(1200))
            .build();

        let config = scenario.to_config();
        assert_eq!(config.name, "Modified Stress Test");
        assert_eq!(config.max_duration, Duration::from_secs(1200));
        // Should inherit scale from StressTest
        assert_eq!(config.scale.device_count, ScaleConfig::large().device_count);
    }

    #[test]
    fn test_scenario_suite() {
        let suite = ScenarioSuite::quick_test();

        assert_eq!(suite.scenarios.len(), 2);
        assert!(suite.stop_on_failure);
        assert!(suite.estimated_duration() > Duration::ZERO);
    }

    #[test]
    fn test_full_suite() {
        let suite = ScenarioSuite::full_test();

        assert!(suite.scenarios.len() >= 5);
        assert!(!suite.stop_on_failure);
    }

    #[test]
    fn test_custom_scenario_serialization() {
        let scenario = Scenario::Custom(Box::new(SimulationConfig {
            name: "Serialization Test".to_string(),
            description: "Test serialization".to_string(),
            scale: ScaleConfig::tiny(),
            memory_pattern: MemoryPattern::Steady,
            load_pattern: LoadPattern::steady(100),
            failure_config: None,
            max_duration: Duration::from_secs(60),
            report_interval: Duration::from_secs(10),
            detailed_metrics: true,
            seed: None,
        }));

        let json = serde_json::to_string(&scenario).unwrap();
        let deserialized: Scenario = serde_json::from_str(&json).unwrap();

        let config = deserialized.to_config();
        assert_eq!(config.name, "Serialization Test");
    }

    #[test]
    fn test_suite_estimated_duration() {
        let suite = ScenarioSuite::new("Test")
            .add_scenario(Scenario::SmokeTest)
            .add_scenario(Scenario::BasicFunctionality)
            .inter_scenario_delay(Duration::from_secs(30));

        let expected = Scenario::SmokeTest.estimated_duration()
            + Scenario::BasicFunctionality.estimated_duration()
            + Duration::from_secs(30);

        assert_eq!(suite.estimated_duration(), expected);
    }
}
