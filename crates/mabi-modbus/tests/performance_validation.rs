//! Performance Validation Tests for Phase 2
//!
//! This module validates that the implementation meets the performance targets
//! specified in the Phase 2 documentation:
//!
//! | Operation                  | Target        | Method                        |
//! |---------------------------|---------------|-------------------------------|
//! | Single register read      | < 100ns       | `bench_register_read`         |
//! | 125 register read         | < 5µs         | `bench_register_read`         |
//! | Single register write     | < 200ns       | `bench_register_write`        |
//! | 123 register write        | < 10µs        | `bench_register_write`        |
//! | 16-thread concurrent      | < 100µs/1000  | `bench_concurrent_access`     |
//!
//! Run with:
//! `cargo test -p mabi-modbus --features performance-tests --test performance_validation --release -- --nocapture`

#![cfg(feature = "performance-tests")]

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use mabi_modbus::registers::{RegisterStoreConfig, SparseRegisterStore};

// =============================================================================
// Configuration
// =============================================================================

/// Number of iterations for timing measurements
const TIMING_ITERATIONS: usize = 10_000;

/// Number of warmup iterations
const WARMUP_ITERATIONS: usize = 1_000;

/// Margin multiplier for performance targets (2x = 100% margin)
const PERFORMANCE_MARGIN: f64 = 3.0;

// =============================================================================
// Performance Target Tests
// =============================================================================

#[test]
fn test_single_register_read_performance() {
    // Target: < 100ns (with margin: < 300ns)
    let target_ns = 100.0 * PERFORMANCE_MARGIN;

    let store = SparseRegisterStore::with_defaults();

    // Pre-populate
    store.write_holding_register(100, 12345).unwrap();

    // Warmup
    for _ in 0..WARMUP_ITERATIONS {
        let _ = store.read_holding_registers(100, 1);
    }

    // Measure
    let start = Instant::now();
    for _ in 0..TIMING_ITERATIONS {
        let _ = store.read_holding_registers(100, 1);
    }
    let elapsed = start.elapsed();

    let avg_ns = elapsed.as_nanos() as f64 / TIMING_ITERATIONS as f64;

    println!("\n=== Single Register Read Performance ===");
    println!(
        "Target: < {}ns (with {}x margin)",
        target_ns, PERFORMANCE_MARGIN
    );
    println!("Actual: {:.2}ns per operation", avg_ns);
    println!(
        "Status: {}",
        if avg_ns < target_ns {
            "PASS ✓"
        } else {
            "FAIL ✗"
        }
    );

    assert!(
        avg_ns < target_ns,
        "Single register read too slow: {:.2}ns (target: < {}ns)",
        avg_ns,
        target_ns
    );
}

#[test]
fn test_125_register_read_performance() {
    // Target: < 5µs (with margin: < 15µs)
    let target_us = 5.0 * PERFORMANCE_MARGIN;

    let store = SparseRegisterStore::with_defaults();

    // Pre-populate 125 registers
    for addr in 0..125u16 {
        store.write_holding_register(addr, addr).unwrap();
    }

    // Warmup
    for _ in 0..WARMUP_ITERATIONS {
        let _ = store.read_holding_registers(0, 125);
    }

    // Measure
    let start = Instant::now();
    for _ in 0..TIMING_ITERATIONS {
        let _ = store.read_holding_registers(0, 125);
    }
    let elapsed = start.elapsed();

    let avg_us = elapsed.as_nanos() as f64 / TIMING_ITERATIONS as f64 / 1000.0;

    println!("\n=== 125 Register Read Performance ===");
    println!(
        "Target: < {}µs (with {}x margin)",
        target_us, PERFORMANCE_MARGIN
    );
    println!("Actual: {:.2}µs per operation", avg_us);
    println!(
        "Status: {}",
        if avg_us < target_us {
            "PASS ✓"
        } else {
            "FAIL ✗"
        }
    );

    assert!(
        avg_us < target_us,
        "125 register read too slow: {:.2}µs (target: < {}µs)",
        avg_us,
        target_us
    );
}

#[test]
fn test_single_register_write_performance() {
    // Target: < 200ns (with margin: < 600ns)
    let target_ns = 200.0 * PERFORMANCE_MARGIN;

    let store = SparseRegisterStore::with_defaults();

    // Warmup
    for i in 0..WARMUP_ITERATIONS {
        let _ = store.write_holding_register(0, i as u16);
    }

    // Measure
    let start = Instant::now();
    for i in 0..TIMING_ITERATIONS {
        let _ = store.write_holding_register(0, i as u16);
    }
    let elapsed = start.elapsed();

    let avg_ns = elapsed.as_nanos() as f64 / TIMING_ITERATIONS as f64;

    println!("\n=== Single Register Write Performance ===");
    println!(
        "Target: < {}ns (with {}x margin)",
        target_ns, PERFORMANCE_MARGIN
    );
    println!("Actual: {:.2}ns per operation", avg_ns);
    println!(
        "Status: {}",
        if avg_ns < target_ns {
            "PASS ✓"
        } else {
            "FAIL ✗"
        }
    );

    assert!(
        avg_ns < target_ns,
        "Single register write too slow: {:.2}ns (target: < {}ns)",
        avg_ns,
        target_ns
    );
}

#[test]
fn test_123_register_write_performance() {
    // Target: < 10µs (with margin: < 30µs)
    let target_us = 10.0 * PERFORMANCE_MARGIN;

    let store = SparseRegisterStore::with_defaults();
    let values: Vec<u16> = (0..123).collect();

    // Warmup
    for _ in 0..WARMUP_ITERATIONS {
        let _ = store.write_holding_registers(0, &values);
    }

    // Measure
    let start = Instant::now();
    for _ in 0..TIMING_ITERATIONS {
        let _ = store.write_holding_registers(0, &values);
    }
    let elapsed = start.elapsed();

    let avg_us = elapsed.as_nanos() as f64 / TIMING_ITERATIONS as f64 / 1000.0;

    println!("\n=== 123 Register Write Performance ===");
    println!(
        "Target: < {}µs (with {}x margin)",
        target_us, PERFORMANCE_MARGIN
    );
    println!("Actual: {:.2}µs per operation", avg_us);
    println!(
        "Status: {}",
        if avg_us < target_us {
            "PASS ✓"
        } else {
            "FAIL ✗"
        }
    );

    assert!(
        avg_us < target_us,
        "123 register write too slow: {:.2}µs (target: < {}µs)",
        avg_us,
        target_us
    );
}

#[test]
fn test_16_thread_concurrent_access_performance() {
    // Target: < 100µs per 1000 operations (with margin: < 300µs)
    let target_us = 100.0 * PERFORMANCE_MARGIN;
    let thread_count = 16;
    let ops_per_thread = 1000;

    let store = Arc::new(SparseRegisterStore::with_defaults());

    // Pre-populate
    for addr in 0..10000u16 {
        store.write_holding_register(addr, addr).unwrap();
    }

    // Warmup
    {
        let handles: Vec<_> = (0..thread_count)
            .map(|t| {
                let store = store.clone();
                thread::spawn(move || {
                    for i in 0..100 {
                        let addr = ((t * 100 + i) % 10000) as u16;
                        let _ = store.write_holding_register(addr, i as u16);
                        let _ = store.read_holding_registers(addr, 1);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }

    // Measure
    let start = Instant::now();
    let handles: Vec<_> = (0..thread_count)
        .map(|t| {
            let store = store.clone();
            thread::spawn(move || {
                for i in 0..ops_per_thread {
                    let addr = ((t * ops_per_thread + i) % 10000) as u16;
                    store.write_holding_register(addr, i as u16).unwrap();
                    store.read_holding_registers(addr, 1).unwrap();
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }
    let elapsed = start.elapsed();

    // Calculate average per 1000 operations (each thread does 1000 ops = 2000 total ops per thread)
    let total_ops = thread_count * ops_per_thread * 2; // read + write
    let us_per_1000 = elapsed.as_micros() as f64 / (total_ops as f64 / 1000.0);

    println!("\n=== 16-Thread Concurrent Access Performance ===");
    println!(
        "Target: < {}µs per 1000 ops (with {}x margin)",
        target_us, PERFORMANCE_MARGIN
    );
    println!("Actual: {:.2}µs per 1000 ops", us_per_1000);
    println!("Total time: {:?} for {} ops", elapsed, total_ops);
    println!(
        "Status: {}",
        if us_per_1000 < target_us {
            "PASS ✓"
        } else {
            "FAIL ✗"
        }
    );

    assert!(
        us_per_1000 < target_us,
        "16-thread concurrent access too slow: {:.2}µs/1000ops (target: < {}µs/1000ops)",
        us_per_1000,
        target_us
    );
}

// =============================================================================
// Coil Performance Tests
// =============================================================================

#[test]
fn test_single_coil_read_performance() {
    let target_ns = 100.0 * PERFORMANCE_MARGIN;

    let store = SparseRegisterStore::with_defaults();
    store.write_coil(100, true).unwrap();

    // Warmup
    for _ in 0..WARMUP_ITERATIONS {
        let _ = store.read_coils(100, 1);
    }

    let start = Instant::now();
    for _ in 0..TIMING_ITERATIONS {
        let _ = store.read_coils(100, 1);
    }
    let elapsed = start.elapsed();

    let avg_ns = elapsed.as_nanos() as f64 / TIMING_ITERATIONS as f64;

    println!("\n=== Single Coil Read Performance ===");
    println!("Actual: {:.2}ns per operation", avg_ns);
    println!(
        "Status: {}",
        if avg_ns < target_ns {
            "PASS ✓"
        } else {
            "FAIL ✗"
        }
    );

    assert!(
        avg_ns < target_ns,
        "Single coil read too slow: {:.2}ns",
        avg_ns
    );
}

#[test]
fn test_2000_coil_read_performance() {
    // 2000 coils is the max per Modbus spec
    let target_us = 50.0 * PERFORMANCE_MARGIN; // Expect similar to 125 registers

    let store = SparseRegisterStore::with_defaults();

    // Pre-populate
    for addr in 0..2000u16 {
        store.write_coil(addr, addr % 2 == 0).unwrap();
    }

    // Warmup
    for _ in 0..WARMUP_ITERATIONS {
        let _ = store.read_coils(0, 2000);
    }

    let start = Instant::now();
    for _ in 0..TIMING_ITERATIONS {
        let _ = store.read_coils(0, 2000);
    }
    let elapsed = start.elapsed();

    let avg_us = elapsed.as_nanos() as f64 / TIMING_ITERATIONS as f64 / 1000.0;

    println!("\n=== 2000 Coil Read Performance ===");
    println!("Actual: {:.2}µs per operation", avg_us);
    println!(
        "Status: {}",
        if avg_us < target_us {
            "PASS ✓"
        } else {
            "FAIL ✗"
        }
    );

    assert!(
        avg_us < target_us,
        "2000 coil read too slow: {:.2}µs",
        avg_us
    );
}

#[test]
fn test_single_coil_write_performance() {
    let target_ns = 200.0 * PERFORMANCE_MARGIN;

    let store = SparseRegisterStore::with_defaults();

    // Warmup
    for i in 0..WARMUP_ITERATIONS {
        let _ = store.write_coil(0, i % 2 == 0);
    }

    let start = Instant::now();
    for i in 0..TIMING_ITERATIONS {
        let _ = store.write_coil(0, i % 2 == 0);
    }
    let elapsed = start.elapsed();

    let avg_ns = elapsed.as_nanos() as f64 / TIMING_ITERATIONS as f64;

    println!("\n=== Single Coil Write Performance ===");
    println!("Actual: {:.2}ns per operation", avg_ns);
    println!(
        "Status: {}",
        if avg_ns < target_ns {
            "PASS ✓"
        } else {
            "FAIL ✗"
        }
    );

    assert!(
        avg_ns < target_ns,
        "Single coil write too slow: {:.2}ns",
        avg_ns
    );
}

// =============================================================================
// Memory Usage Tests
// =============================================================================

#[test]
fn test_sparse_memory_efficiency() {
    let store = SparseRegisterStore::with_defaults();

    // Access only 100 registers out of possible 10,000
    for addr in (0..10000u16).step_by(100) {
        store.write_holding_register(addr, addr).unwrap();
    }

    let entries = store.entry_count();
    let memory = store.memory_usage();

    println!("\n=== Sparse Memory Efficiency ===");
    println!("Entries stored: {}", entries);
    println!("Memory usage: {} bytes", memory);
    println!("Bytes per entry: {:.1}", memory as f64 / entries as f64);

    // Should only store ~100 entries, not 10,000
    assert_eq!(entries, 100, "Sparse storage not working correctly");

    // Memory should be less than 10KB for 100 entries
    assert!(
        memory < 10_000,
        "Memory usage too high for sparse storage: {} bytes",
        memory
    );
}

#[test]
fn test_large_scale_memory_usage() {
    // Simulate accessing 100,000 registers
    let store = SparseRegisterStore::with_defaults();

    println!("\n=== Large Scale Memory Usage ===");

    // Write 10,000 holding registers
    let start = Instant::now();
    for addr in 0..10000u16 {
        store.write_holding_register(addr, addr).unwrap();
    }
    let write_time = start.elapsed();

    // Write 10,000 input registers
    for addr in 0..10000u16 {
        store.set_input_register(addr, addr).unwrap();
    }

    // Write 10,000 coils
    for addr in 0..10000u16 {
        store.write_coil(addr, addr % 2 == 0).unwrap();
    }

    // Write 10,000 discrete inputs
    for addr in 0..10000u16 {
        store.set_discrete_input(addr, addr % 2 == 0).unwrap();
    }

    let total_entries = store.entry_count();
    let memory = store.memory_usage();
    let mb = memory as f64 / (1024.0 * 1024.0);

    println!("Total entries: {}", total_entries);
    println!("Memory usage: {:.2} MB ({} bytes)", mb, memory);
    println!("Write time for 10K registers: {:?}", write_time);

    // Target: < 2GB for 10K devices (Phase 2 requirement)
    // Per device (assuming 40K entries): ~1MB
    // So for 40K entries, we expect < 2MB
    assert!(
        memory < 2 * 1024 * 1024,
        "Memory usage too high: {:.2} MB (expected < 2MB for 40K entries)",
        mb
    );
}

// =============================================================================
// Throughput Tests
// =============================================================================

#[test]
fn test_read_throughput() {
    let store = Arc::new(SparseRegisterStore::with_defaults());

    // Pre-populate
    for addr in 0..10000u16 {
        store.write_holding_register(addr, addr).unwrap();
    }

    let duration = Duration::from_secs(1);
    let thread_count = 4;

    let start = Instant::now();
    let handles: Vec<_> = (0..thread_count)
        .map(|t| {
            let store = store.clone();
            thread::spawn(move || {
                let mut count = 0u64;
                let start_time = Instant::now();
                while start_time.elapsed() < duration {
                    let addr = ((t * 1000 + count) % 10000) as u16;
                    let _ = store.read_holding_registers(addr, 1);
                    count += 1;
                }
                count
            })
        })
        .collect();

    let total_ops: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
    let elapsed = start.elapsed();

    let ops_per_sec = total_ops as f64 / elapsed.as_secs_f64();

    println!("\n=== Read Throughput ===");
    println!("Threads: {}", thread_count);
    println!("Duration: {:?}", elapsed);
    println!("Total operations: {}", total_ops);
    println!("Throughput: {:.0} ops/sec", ops_per_sec);

    // Target: 100,000+ TPS (for full system, register ops should be much faster)
    // Individual register reads should exceed 1M ops/sec
    assert!(
        ops_per_sec > 100_000.0,
        "Read throughput too low: {:.0} ops/sec (expected > 100K)",
        ops_per_sec
    );
}

#[test]
fn test_write_throughput() {
    let store = Arc::new(SparseRegisterStore::with_defaults());

    let duration = Duration::from_secs(1);
    let thread_count = 4;

    let start = Instant::now();
    let handles: Vec<_> = (0..thread_count)
        .map(|t| {
            let store = store.clone();
            thread::spawn(move || {
                let mut count = 0u64;
                let start_time = Instant::now();
                while start_time.elapsed() < duration {
                    let addr = ((t * 1000 + count) % 10000) as u16;
                    let _ = store.write_holding_register(addr, count as u16);
                    count += 1;
                }
                count
            })
        })
        .collect();

    let total_ops: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
    let elapsed = start.elapsed();

    let ops_per_sec = total_ops as f64 / elapsed.as_secs_f64();

    println!("\n=== Write Throughput ===");
    println!("Threads: {}", thread_count);
    println!("Duration: {:?}", elapsed);
    println!("Total operations: {}", total_ops);
    println!("Throughput: {:.0} ops/sec", ops_per_sec);

    // Target: 100,000+ TPS
    assert!(
        ops_per_sec > 100_000.0,
        "Write throughput too low: {:.0} ops/sec (expected > 100K)",
        ops_per_sec
    );
}

#[test]
fn test_mixed_throughput() {
    let store = Arc::new(SparseRegisterStore::with_defaults());

    // Pre-populate
    for addr in 0..10000u16 {
        store.write_holding_register(addr, addr).unwrap();
    }

    let duration = Duration::from_secs(1);
    let thread_count = 8; // 4 readers, 4 writers

    let start = Instant::now();
    let handles: Vec<_> = (0..thread_count)
        .map(|t| {
            let store = store.clone();
            let is_writer = t % 2 == 0;
            thread::spawn(move || {
                let mut count = 0u64;
                let start_time = Instant::now();
                while start_time.elapsed() < duration {
                    let addr = ((t * 1000 + count) % 10000) as u16;
                    if is_writer {
                        let _ = store.write_holding_register(addr, count as u16);
                    } else {
                        let _ = store.read_holding_registers(addr, 1);
                    }
                    count += 1;
                }
                count
            })
        })
        .collect();

    let total_ops: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
    let elapsed = start.elapsed();

    let ops_per_sec = total_ops as f64 / elapsed.as_secs_f64();

    println!("\n=== Mixed Throughput (4 readers, 4 writers) ===");
    println!("Threads: {}", thread_count);
    println!("Duration: {:?}", elapsed);
    println!("Total operations: {}", total_ops);
    println!("Throughput: {:.0} ops/sec", ops_per_sec);

    // Target: 100,000+ TPS even with mixed workload
    assert!(
        ops_per_sec > 100_000.0,
        "Mixed throughput too low: {:.0} ops/sec (expected > 100K)",
        ops_per_sec
    );
}

// =============================================================================
// Batch Operation Tests
// =============================================================================

#[test]
fn test_batch_read_performance() {
    let store = SparseRegisterStore::with_defaults();

    // Pre-populate
    for addr in 0..10000u16 {
        store.write_holding_register(addr, addr).unwrap();
    }

    // Test various batch sizes
    let batch_sizes = [1, 10, 50, 100, 125];

    println!("\n=== Batch Read Performance ===");
    println!(
        "{:>10} {:>15} {:>15}",
        "Batch Size", "Avg Time (µs)", "Throughput"
    );

    for &batch_size in &batch_sizes {
        let iterations = 10_000;

        let start = Instant::now();
        for _ in 0..iterations {
            let _ = store.read_holding_registers(0, batch_size);
        }
        let elapsed = start.elapsed();

        let avg_us = elapsed.as_nanos() as f64 / iterations as f64 / 1000.0;
        let regs_per_sec = (iterations * batch_size as usize) as f64 / elapsed.as_secs_f64();

        println!("{:>10} {:>15.2} {:>15.0}", batch_size, avg_us, regs_per_sec);
    }
}

#[test]
fn test_batch_write_performance() {
    let store = SparseRegisterStore::with_defaults();

    // Test various batch sizes
    let batch_sizes = [1, 10, 50, 100, 123];

    println!("\n=== Batch Write Performance ===");
    println!(
        "{:>10} {:>15} {:>15}",
        "Batch Size", "Avg Time (µs)", "Throughput"
    );

    for &batch_size in &batch_sizes {
        let iterations = 10_000;
        let values: Vec<u16> = (0..batch_size).collect();

        let start = Instant::now();
        for _ in 0..iterations {
            let _ = store.write_holding_registers(0, &values);
        }
        let elapsed = start.elapsed();

        let avg_us = elapsed.as_nanos() as f64 / iterations as f64 / 1000.0;
        let regs_per_sec = (iterations * batch_size as usize) as f64 / elapsed.as_secs_f64();

        println!("{:>10} {:>15.2} {:>15.0}", batch_size, avg_us, regs_per_sec);
    }
}

// =============================================================================
// Summary Report
// =============================================================================

#[test]
fn test_performance_summary() {
    println!("\n");
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║           PHASE 2 PERFORMANCE VALIDATION SUMMARY                ║");
    println!("╠════════════════════════════════════════════════════════════════╣");
    println!("║ Target                  │ Spec      │ Margin    │ Method       ║");
    println!("╠─────────────────────────┼───────────┼───────────┼──────────────╣");
    println!(
        "║ Single register read    │ < 100ns   │ {}x       │ Direct test  ║",
        PERFORMANCE_MARGIN
    );
    println!(
        "║ 125 register read       │ < 5µs     │ {}x       │ Direct test  ║",
        PERFORMANCE_MARGIN
    );
    println!(
        "║ Single register write   │ < 200ns   │ {}x       │ Direct test  ║",
        PERFORMANCE_MARGIN
    );
    println!(
        "║ 123 register write      │ < 10µs    │ {}x       │ Direct test  ║",
        PERFORMANCE_MARGIN
    );
    println!(
        "║ 16-thread concurrent    │ < 100µs   │ {}x       │ Thread pool  ║",
        PERFORMANCE_MARGIN
    );
    println!("╠─────────────────────────┴───────────┴───────────┴──────────────╣");
    println!("║ Run: cargo test --test performance_validation --release       ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
}
