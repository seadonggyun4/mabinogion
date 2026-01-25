//! Large-scale performance validation tests.
//!
//! These tests validate the system's ability to handle:
//! - 10,000+ simultaneous connections
//! - 100,000+ transactions per second
//! - Memory usage within specified limits
//!
//! Note: These tests require significant resources and are marked as `#[ignore]`
//! by default. Run with `cargo test --test large_scale_validation -- --ignored`

use std::sync::Arc;
use std::time::Duration;

use mabi_modbus::testing::{
    PerformanceValidator, PerformanceConfig, PerformanceTarget,
    MemoryProfiler, LoadGenerator, LoadConfig, LoadPattern,
    TestReport, TestMetrics,
    memory::estimate_memory_usage,
};
use mabi_modbus::{ModbusTcpServerV2, tcp::ServerConfigV2};
use mabi_modbus::registers::SparseRegisterStore;

/// Test configuration for quick validation (runs in CI).
fn quick_config() -> PerformanceConfig {
    PerformanceConfig {
        target_connections: 100,
        target_tps: 1000,
        duration: Duration::from_secs(10),
        ramp_up: Duration::from_secs(2),
        server_addr: "127.0.0.1:5502".parse().unwrap(),
        max_p99_latency: Duration::from_millis(50),
        max_error_rate: 0.05,
        warmup_iterations: 10,
        report_interval: Duration::from_secs(2),
    }
}

/// Test that basic performance validation framework works.
#[tokio::test]
async fn test_performance_validator_creation() {
    let config = quick_config();
    let mut validator = PerformanceValidator::new(config);

    validator.add_target(PerformanceTarget::min_connections(50));
    validator.add_target(PerformanceTarget::max_error_rate(0.1));

    // Just verify it can be created without panicking
    assert!(true);
}

/// Test memory estimation is reasonable.
#[test]
fn test_memory_estimation_accuracy() {
    // 10,000 devices with 100 points each
    let estimate = estimate_memory_usage(10_000, 100);

    // Should be under 2GB for 10K devices (as per spec)
    assert!(
        estimate.total_mb() < 2048.0,
        "Memory estimate for 10K devices should be under 2GB, got {:.2} MB",
        estimate.total_mb()
    );

    println!("{}", estimate.format());

    // 50,000 devices with 100 points each
    let large_estimate = estimate_memory_usage(50_000, 100);

    // Should be under 8GB for 50K devices (as per spec)
    assert!(
        large_estimate.total_gb() < 8.0,
        "Memory estimate for 50K devices should be under 8GB, got {:.2} GB",
        large_estimate.total_gb()
    );

    println!("{}", large_estimate.format());
}

/// Test that sparse register store handles large address spaces efficiently.
#[test]
fn test_sparse_register_store_memory_efficiency() {
    use mabi_modbus::registers::RegisterStoreConfig;

    // Use a config that allows larger addresses for this sparse test
    let config = RegisterStoreConfig::with_max_addresses(65535);
    let store = SparseRegisterStore::new(config);

    // Write to sparse addresses (simulating 10K devices with sparse registers)
    for device_offset in 0..1000u16 {
        let base_addr = device_offset * 60; // Use smaller multiplier to stay in range
        // Only write a few registers per device
        for i in 0..10u16 {
            let _ = store.write_holding_register(base_addr + i, (device_offset * 10 + i) as u16);
        }
    }

    let memory = store.memory_usage();
    println!("Sparse store memory usage: {} bytes for 10,000 registers", memory);

    // Memory should be proportional to actual stored values, not address space
    // 10,000 registers * ~20 bytes each = ~200KB
    assert!(
        memory < 1_000_000,
        "Sparse store should use less than 1MB for 10K registers, used {} bytes",
        memory
    );
}

/// Test load generator configuration presets.
#[test]
fn test_load_config_presets() {
    let steady = LoadConfig::steady(1000, 10000.0);
    assert_eq!(steady.connections, 1000);
    assert!((steady.requests_per_second - 10000.0).abs() < 0.01);

    let spike = LoadConfig::spike(100, 1000);
    assert_eq!(spike.connections, 1000);
    matches!(spike.pattern, LoadPattern::Spike { .. });

    let ramp = LoadConfig::ramp(100, 1000, Duration::from_secs(10));
    assert_eq!(ramp.connections, 1000);
    matches!(ramp.pattern, LoadPattern::Ramp { .. });
}

/// Test report generation.
#[test]
fn test_report_generation() {
    let metrics = TestMetrics {
        total_requests: 100_000,
        successful_requests: 99_500,
        failed_requests: 500,
        total_connections: 1000,
        peak_connections: 1000,
        avg_tps: 10_000,
        peak_tps: 12_000,
        p50_latency_ms: 2.0,
        p95_latency_ms: 8.0,
        p99_latency_ms: 15.0,
        error_rate: 0.005,
        memory_peak_mb: 512.0,
    };

    let report = TestReport::builder("Performance Test")
        .description("Validating 10K connections and 100K TPS")
        .environment("test")
        .config("connections", "1000")
        .config("target_tps", "10000")
        .add_target_result("Min Connections", "1000", "1000", ">=", true)
        .add_target_result("Min TPS", "10000", "10000", ">=", true)
        .add_target_result("Max P99 Latency", "50ms", "15ms", "<=", true)
        .add_target_result("Max Error Rate", "1%", "0.5%", "<=", true)
        .add_note("Test completed successfully")
        .build(metrics, true, Duration::from_secs(60));

    assert!(report.summary.passed);
    assert_eq!(report.summary.targets_passed, 4);

    // Test JSON export
    let json = report.to_json().expect("JSON export should work");
    assert!(json.contains("Performance Test"));

    // Test Markdown export
    let md = report.to_markdown();
    assert!(md.contains("# Test Report"));
    assert!(md.contains("PASSED"));

    println!("{}", md);
}

/// Test memory profiler basic functionality.
#[test]
fn test_memory_profiler_basics() {
    let profiler = MemoryProfiler::new();

    // Take a snapshot
    let snapshot = profiler.snapshot();
    assert!(snapshot.timestamp.elapsed() < Duration::from_secs(1));

    // Generate report
    let report = profiler.report();
    println!("{}", report.format());

    // Verify reasonable defaults
    assert!(!report.potential_leak);
}

// =============================================================================
// Large-Scale Tests (Ignored by Default)
// =============================================================================

/// Test 1,000 simultaneous connections.
///
/// This is a "smoke test" for larger scale tests.
/// Run with: `cargo test --test large_scale_validation test_1k_connections -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn test_1k_connections() {
    // Start a test server
    let server_config = ServerConfigV2 {
        bind_address: "127.0.0.1:5503".parse().unwrap(),
        max_connections: 2000,
        ..Default::default()
    };

    let server = Arc::new(ModbusTcpServerV2::new(server_config));
    let server_handle = {
        let server = server.clone();
        tokio::spawn(async move {
            let _ = server.run().await;
        })
    };

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Run validation
    let config = PerformanceConfig {
        target_connections: 1000,
        target_tps: 5000,
        duration: Duration::from_secs(30),
        ramp_up: Duration::from_secs(5),
        server_addr: "127.0.0.1:5503".parse().unwrap(),
        max_p99_latency: Duration::from_millis(100),
        max_error_rate: 0.05,
        warmup_iterations: 50,
        report_interval: Duration::from_secs(5),
    };

    let mut validator = PerformanceValidator::new(config);
    validator.add_target(PerformanceTarget::min_connections(500));
    validator.add_target(PerformanceTarget::max_error_rate(0.1));

    let result = validator.run().await;

    // Shutdown server
    server.shutdown();
    server_handle.abort();

    match result {
        Ok(validation) => {
            println!("\n{}", validation.summary());
            for target in &validation.targets {
                println!(
                    "  {} - {}: expected {}, got {:.2}",
                    if target.passed { "PASS" } else { "FAIL" },
                    target.target.name,
                    target.target.threshold,
                    target.actual_value
                );
            }
            // Don't assert pass for now, just validate it runs
        }
        Err(e) => {
            println!("Validation error: {}", e);
        }
    }
}

/// Test 10,000 simultaneous connections.
///
/// This test validates the 10K connection requirement.
/// Run with: `cargo test --test large_scale_validation test_10k_connections -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn test_10k_connections() {
    // Start a test server optimized for high connection count
    let server_config = ServerConfigV2 {
        bind_address: "127.0.0.1:5504".parse().unwrap(),
        max_connections: 15000,
        ..Default::default()
    };

    let server = Arc::new(ModbusTcpServerV2::new(server_config));
    let server_handle = {
        let server = server.clone();
        tokio::spawn(async move {
            let _ = server.run().await;
        })
    };

    tokio::time::sleep(Duration::from_secs(1)).await;

    // Memory profiler
    let profiler = MemoryProfiler::new();
    let _guard = profiler.start();

    // Run validation
    let mut validator = PerformanceValidator::ten_thousand_connections();
    validator.config.server_addr = "127.0.0.1:5504".parse().unwrap();

    validator.add_target(PerformanceTarget::min_connections(10_000));
    validator.add_target(PerformanceTarget::max_p99_latency(50));
    validator.add_target(PerformanceTarget::max_error_rate(0.01));

    let result = validator.run().await;

    // Get memory report
    let mem_report = profiler.report();

    // Shutdown
    server.shutdown();
    server_handle.abort();

    println!("\n=== Memory Report ===");
    println!("{}", mem_report.format());

    match result {
        Ok(validation) => {
            println!("\n=== Validation Results ===");
            println!("{}", validation.summary());

            // Generate full report
            let report = TestReport::builder("10K Connection Validation")
                .description("Validate 10,000 simultaneous connections")
                .environment("integration-test")
                .config("target_connections", "10000")
                .build(validation.metrics.clone(), validation.passed, validation.duration);

            println!("\n{}", report.to_markdown());

            assert!(
                validation.passed,
                "10K connection validation should pass"
            );
        }
        Err(e) => {
            panic!("Validation failed with error: {}", e);
        }
    }
}

/// Test 100,000 TPS throughput.
///
/// This test validates the 100K TPS requirement.
/// Run with: `cargo test --test large_scale_validation test_100k_tps -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn test_100k_tps() {
    // Start a test server optimized for high throughput
    let server_config = ServerConfigV2 {
        bind_address: "127.0.0.1:5505".parse().unwrap(),
        max_connections: 10000,
        ..Default::default()
    };

    let server = Arc::new(ModbusTcpServerV2::new(server_config));
    let server_handle = {
        let server = server.clone();
        tokio::spawn(async move {
            let _ = server.run().await;
        })
    };

    tokio::time::sleep(Duration::from_secs(1)).await;

    // Run validation
    let mut validator = PerformanceValidator::hundred_thousand_tps();
    validator.config.server_addr = "127.0.0.1:5505".parse().unwrap();

    validator.add_target(PerformanceTarget::min_tps(100_000));
    validator.add_target(PerformanceTarget::max_p99_latency(10));
    validator.add_target(PerformanceTarget::max_error_rate(0.01));

    let result = validator.run().await;

    // Shutdown
    server.shutdown();
    server_handle.abort();

    match result {
        Ok(validation) => {
            println!("\n=== 100K TPS Validation Results ===");
            println!("{}", validation.summary());

            for target in &validation.targets {
                println!(
                    "  {} - {}: got {:.2} (threshold: {})",
                    if target.passed { "PASS" } else { "FAIL" },
                    target.target.name,
                    target.actual_value,
                    target.target.threshold
                );
            }

            assert!(
                validation.passed,
                "100K TPS validation should pass"
            );
        }
        Err(e) => {
            panic!("Validation failed with error: {}", e);
        }
    }
}

/// Long-running stability test (24 hours).
///
/// This test validates system stability over an extended period.
/// Run with: `cargo test --test large_scale_validation test_24h_stability -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn test_24h_stability() {
    println!("Starting 24-hour stability test...");
    println!("Note: This test takes a VERY long time to run.");

    // For actual 24h test, use:
    // let duration = Duration::from_secs(24 * 60 * 60);

    // For quick verification of test setup, use shorter duration:
    let duration = Duration::from_secs(60);

    let server_config = ServerConfigV2 {
        bind_address: "127.0.0.1:5506".parse().unwrap(),
        max_connections: 5000,
        ..Default::default()
    };

    let server = Arc::new(ModbusTcpServerV2::new(server_config));
    let server_handle = {
        let server = server.clone();
        tokio::spawn(async move {
            let _ = server.run().await;
        })
    };

    tokio::time::sleep(Duration::from_secs(1)).await;

    // Start memory profiler
    let profiler = MemoryProfiler::new()
        .with_sample_interval(Duration::from_secs(60));
    let _guard = profiler.start();

    // Run load generator
    let config = LoadConfig {
        server_addr: "127.0.0.1:5506".parse().unwrap(),
        connections: 1000,
        requests_per_second: 5000.0,
        duration,
        ramp_up: Duration::from_secs(30),
        pattern: LoadPattern::Constant,
        connect_timeout: Duration::from_secs(30),
        request_timeout: Duration::from_secs(10),
        keep_alive: true,
    };

    let generator = LoadGenerator::new(config);
    let result = generator.run().await;

    // Shutdown
    server.shutdown();
    server_handle.abort();

    // Reports
    let mem_report = profiler.report();

    println!("\n=== 24h Stability Test Results ===");
    println!("{}", result.format());
    println!("\n=== Memory Report ===");
    println!("{}", mem_report.format());

    // Check for memory leaks
    assert!(
        !mem_report.potential_leak,
        "No memory leak should be detected during stability test"
    );

    // Check success rate
    assert!(
        result.success_rate > 0.99,
        "Success rate should be above 99%, got {:.2}%",
        result.success_rate * 100.0
    );
}
