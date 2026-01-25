//! End-to-end integration tests.
//!
//! Tests for complete simulation scenarios.

mod framework;
mod helpers;
mod fixtures;

use async_trait::async_trait;
use framework::*;
use fixtures::*;
use helpers::*;
use std::time::Duration;

// =============================================================================
// Modbus E2E Tests
// =============================================================================

struct ModbusServerStartTest {
    port: u16,
}

#[async_trait]
impl TestCase for ModbusServerStartTest {
    fn name(&self) -> &str {
        "modbus_server_start"
    }

    fn description(&self) -> &str {
        "Test that Modbus TCP server starts and accepts connections"
    }

    async fn setup(&self, ctx: &TestContext) -> Result<(), String> {
        ctx.set("port", self.port).await;
        ctx.log(format!("Allocated port: {}", self.port)).await;
        Ok(())
    }

    async fn run(&self, ctx: &TestContext) -> Result<(), String> {
        let port: u16 = ctx.get("port").await.unwrap();
        let addr = format!("127.0.0.1:{}", port).parse().unwrap();

        // TODO: Start actual Modbus server here
        // For now, just verify port is available
        ctx.assert_true("port_available", !wait_for_port(addr, Duration::from_millis(100)).await).await;

        Ok(())
    }
}

struct ModbusReadRegistersTest;

#[async_trait]
impl TestCase for ModbusReadRegistersTest {
    fn name(&self) -> &str {
        "modbus_read_registers"
    }

    fn description(&self) -> &str {
        "Test reading holding registers from Modbus server"
    }

    fn should_skip(&self) -> Option<String> {
        // Skip if no server is running
        Some("Server integration not yet implemented".into())
    }

    async fn run(&self, ctx: &TestContext) -> Result<(), String> {
        let port: u16 = ctx.get("port").await.unwrap_or(5502);
        let addr = format!("127.0.0.1:{}", port).parse().unwrap();

        let helper = ModbusTestHelper::new(addr);

        // Read registers
        let values = helper
            .read_holding_registers(1, 0, 10)
            .await
            .map_err(|e| e.to_string())?;

        ctx.assert_eq("register_count", 10, values.len()).await;

        Ok(())
    }
}

struct ModbusWriteRegisterTest;

#[async_trait]
impl TestCase for ModbusWriteRegisterTest {
    fn name(&self) -> &str {
        "modbus_write_register"
    }

    fn description(&self) -> &str {
        "Test writing a single register to Modbus server"
    }

    fn should_skip(&self) -> Option<String> {
        Some("Server integration not yet implemented".into())
    }

    async fn run(&self, ctx: &TestContext) -> Result<(), String> {
        let port: u16 = ctx.get("port").await.unwrap_or(5502);
        let addr = format!("127.0.0.1:{}", port).parse().unwrap();

        let helper = ModbusTestHelper::new(addr);

        // Write value
        helper
            .write_single_register(1, 100, 12345)
            .await
            .map_err(|e| e.to_string())?;

        // Read back and verify
        let values = helper
            .read_holding_registers(1, 100, 1)
            .await
            .map_err(|e| e.to_string())?;

        ctx.assert_eq("written_value", 12345u16, values[0]).await;

        Ok(())
    }
}

// =============================================================================
// Scenario E2E Tests
// =============================================================================

struct ScenarioLoadTest;

#[async_trait]
impl TestCase for ScenarioLoadTest {
    fn name(&self) -> &str {
        "scenario_load"
    }

    fn description(&self) -> &str {
        "Test loading and parsing a scenario file"
    }

    async fn run(&self, ctx: &TestContext) -> Result<(), String> {
        let scenario_content = ScenarioFactory::minimal();
        let scenario_path = create_temp_scenario(&scenario_content)
            .await
            .map_err(|e| e.to_string())?;

        ctx.log(format!("Created temp scenario at: {:?}", scenario_path)).await;

        // Parse scenario
        let content = tokio::fs::read_to_string(&scenario_path)
            .await
            .map_err(|e| e.to_string())?;

        let scenario: serde_yaml::Value = serde_yaml::from_str(&content)
            .map_err(|e| e.to_string())?;

        ctx.assert_true("has_name", scenario.get("name").is_some()).await;
        ctx.assert_true("has_devices", scenario.get("devices").is_some()).await;

        // Cleanup
        let _ = tokio::fs::remove_file(&scenario_path).await;

        Ok(())
    }
}

struct MultiDeviceScenarioTest;

#[async_trait]
impl TestCase for MultiDeviceScenarioTest {
    fn name(&self) -> &str {
        "multi_device_scenario"
    }

    fn description(&self) -> &str {
        "Test scenario with multiple devices"
    }

    async fn run(&self, ctx: &TestContext) -> Result<(), String> {
        let scenario_content = ScenarioFactory::multi_device(10);
        let scenario_path = create_temp_scenario(&scenario_content)
            .await
            .map_err(|e| e.to_string())?;

        let content = tokio::fs::read_to_string(&scenario_path)
            .await
            .map_err(|e| e.to_string())?;

        let scenario: serde_yaml::Value = serde_yaml::from_str(&content)
            .map_err(|e| e.to_string())?;

        let devices = scenario["devices"].as_sequence().unwrap();
        ctx.assert_eq("device_count", 10, devices.len()).await;

        // Cleanup
        let _ = tokio::fs::remove_file(&scenario_path).await;

        Ok(())
    }
}

struct PatternScenarioTest;

#[async_trait]
impl TestCase for PatternScenarioTest {
    fn name(&self) -> &str {
        "pattern_scenario"
    }

    fn description(&self) -> &str {
        "Test scenario with data patterns"
    }

    async fn run(&self, ctx: &TestContext) -> Result<(), String> {
        let scenario_content = ScenarioFactory::with_patterns();
        let scenario_path = create_temp_scenario(&scenario_content)
            .await
            .map_err(|e| e.to_string())?;

        let content = tokio::fs::read_to_string(&scenario_path)
            .await
            .map_err(|e| e.to_string())?;

        let scenario: serde_yaml::Value = serde_yaml::from_str(&content)
            .map_err(|e| e.to_string())?;

        ctx.assert_true("has_pattern_device", scenario.get("devices").is_some()).await;

        // Cleanup
        let _ = tokio::fs::remove_file(&scenario_path).await;

        Ok(())
    }
}

// =============================================================================
// Configuration E2E Tests
// =============================================================================

struct ConfigValidationTest;

#[async_trait]
impl TestCase for ConfigValidationTest {
    fn name(&self) -> &str {
        "config_validation"
    }

    fn description(&self) -> &str {
        "Test configuration validation"
    }

    async fn run(&self, ctx: &TestContext) -> Result<(), String> {
        // Test valid config
        let valid_config = ConfigFactory::modbus_tcp(5502);
        let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(&valid_config);
        ctx.assert_true("valid_config_parses", parsed.is_ok()).await;

        // Test engine config
        let engine_config = ConfigFactory::engine();
        let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(&engine_config);
        ctx.assert_true("engine_config_parses", parsed.is_ok()).await;

        Ok(())
    }
}

// =============================================================================
// Test Runner
// =============================================================================

#[tokio::test]
async fn run_e2e_test_suite() {
    let config = TestConfig::default()
        .with_timeout(Duration::from_secs(30))
        .with_verbose(true);

    let mut runner = TestRunner::new(config);

    // Add test cases
    let port = get_free_port().await.unwrap_or(5502);
    runner.add_test(ModbusServerStartTest { port });
    runner.add_test(ModbusReadRegistersTest);
    runner.add_test(ModbusWriteRegisterTest);
    runner.add_test(ScenarioLoadTest);
    runner.add_test(MultiDeviceScenarioTest);
    runner.add_test(PatternScenarioTest);
    runner.add_test(ConfigValidationTest);

    // Run tests
    let report = runner.run().await;
    report.print();

    // Assert all non-skipped tests passed
    assert!(
        report.failed == 0,
        "Some tests failed: {} failed out of {}",
        report.failed,
        report.reports.len()
    );
}

// =============================================================================
// Benchmark Tests
// =============================================================================

#[tokio::test]
async fn benchmark_scenario_parsing() {
    let scenario_content = ScenarioFactory::multi_device(100);

    let result = benchmark(100, || async {
        let _: serde_yaml::Value = serde_yaml::from_str(&scenario_content).unwrap();
    })
    .await;

    result.print();

    // Assert reasonable performance
    assert!(
        result.p95 < Duration::from_millis(10),
        "Scenario parsing too slow: {:?}",
        result.p95
    );
}

#[tokio::test]
async fn benchmark_data_generation() {
    let result = benchmark(1000, || async {
        DataFactory::random_registers(1000);
    })
    .await;

    result.print();

    assert!(
        result.p95 < Duration::from_millis(1),
        "Data generation too slow: {:?}",
        result.p95
    );
}
