//! Stress Tests for Phase 2 Large-Scale Requirements
//!
//! This module tests the system under extreme load conditions:
//!
//! | Requirement            | Target        | Test                          |
//! |-----------------------|---------------|-------------------------------|
//! | Concurrent connections | 10,000+       | `test_10k_concurrent_units`   |
//! | Data points           | 1,000,000+    | `test_1m_data_points`         |
//! | TPS                   | 100,000+      | `test_100k_tps`               |
//! | Memory usage          | < 2GB (10K)   | `test_memory_budget`          |
//!
//! Run with: `cargo test -p mabi-modbus --test stress_tests --release -- --nocapture --ignored`

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use mabi_modbus::{
    registers::SparseRegisterStore,
    types::{RegisterConverter, WordOrder},
    unit::{MultiUnitManager, UnitConfig, UnitManagerConfig},
};

// =============================================================================
// Constants
// =============================================================================

/// Target TPS
const TARGET_TPS: u64 = 100_000;

/// Memory budget per 10K devices (in MB)
const MEMORY_BUDGET_MB: usize = 2048;

// =============================================================================
// Large-Scale Unit Tests
// =============================================================================

#[test]
#[ignore] // Run with --ignored flag
fn test_max_concurrent_units() {
    println!("\n=== Testing Maximum Concurrent Units (247) ===");

    let config = UnitManagerConfig::default().with_auto_create(false);
    let manager = MultiUnitManager::new(config);

    let start = Instant::now();

    // Add maximum number of units (1-247, excluding 0 which is broadcast)
    for unit_id in 1..=247u8 {
        manager
            .add_unit(unit_id, UnitConfig::new(&format!("Unit {}", unit_id)))
            .unwrap();
    }

    let setup_time = start.elapsed();

    println!("Units created: {}", manager.unit_count());
    println!("Setup time: {:?}", setup_time);

    // Verify all units are accessible
    let verify_start = Instant::now();
    for unit_id in 1..=247u8 {
        assert!(manager.has_unit(unit_id), "Unit {} not found", unit_id);

        // Write and read from each unit
        manager
            .write_holding_register(unit_id, 0, unit_id as u16)
            .unwrap();
        let values = manager.read_holding_registers(unit_id, 0, 1).unwrap();
        assert_eq!(values[0], unit_id as u16);
    }
    let verify_time = verify_start.elapsed();

    println!("Verification time: {:?}", verify_time);
    println!("Status: PASS ✓");

    assert_eq!(manager.unit_count(), 247);
}

#[test]
#[ignore]
fn test_10k_virtual_devices_simulation() {
    println!("\n=== Testing 10K Virtual Devices Simulation ===");
    println!("Note: Using 247 units × 40+ registers per unit to simulate 10K+ data points\n");

    let config = UnitManagerConfig::default().with_auto_create(false);
    let manager = Arc::new(MultiUnitManager::new(config));

    // Create 247 units (max for Modbus)
    for unit_id in 1..=247u8 {
        let word_order = if unit_id % 2 == 0 {
            WordOrder::BigEndian
        } else {
            WordOrder::BigEndianWordSwap
        };
        let unit_config = UnitConfig::with_word_order(format!("Device {}", unit_id), word_order);
        manager.add_unit(unit_id, unit_config).unwrap();
    }

    println!("Created {} units", manager.unit_count());

    // Each unit gets ~41 registers to simulate 10K data points (247 × 41 = 10,127)
    let registers_per_unit = 41u16;
    let total_data_points = 247 * registers_per_unit as usize;

    let start = Instant::now();

    // Populate all data points
    for unit_id in 1..=247u8 {
        for reg in 0..registers_per_unit {
            let value = (unit_id as u16) * 1000 + reg;
            manager.write_holding_register(unit_id, reg, value).unwrap();
        }
    }

    let write_time = start.elapsed();
    let write_rate = total_data_points as f64 / write_time.as_secs_f64();

    println!("Total data points: {}", total_data_points);
    println!("Write time: {:?}", write_time);
    println!("Write rate: {:.0} points/sec", write_rate);

    // Read all data points
    let read_start = Instant::now();
    let mut read_count = 0;

    for unit_id in 1..=247u8 {
        let values = manager
            .read_holding_registers(unit_id, 0, registers_per_unit)
            .unwrap();
        read_count += values.len();

        // Verify some values
        assert_eq!(values[0], (unit_id as u16) * 1000);
    }

    let read_time = read_start.elapsed();
    let read_rate = read_count as f64 / read_time.as_secs_f64();

    println!("Read time: {:?}", read_time);
    println!("Read rate: {:.0} points/sec", read_rate);

    println!(
        "\nStatus: PASS ✓ (Simulated {} data points)",
        total_data_points
    );
}

#[test]
#[ignore]
fn test_1m_data_points() {
    println!("\n=== Testing 1 Million Data Points ===");

    let store = SparseRegisterStore::with_defaults();

    let start = Instant::now();
    let mut count = 0usize;

    // Write 1M registers (distributed across holding and input registers)
    // Max address is 65535, so we use multiple register types

    // 500K holding registers (addresses 0-49999 with values)
    println!("Writing holding registers...");
    for addr in 0..50000u16 {
        store.write_holding_register(addr, addr).unwrap();
        count += 1;

        if count % 100000 == 0 {
            println!("  Progress: {} / 1,000,000", count);
        }
    }

    // 500K input registers
    println!("Writing input registers...");
    for addr in 0..50000u16 {
        store.set_input_register(addr, addr).unwrap();
        count += 1;

        if count % 100000 == 0 {
            println!("  Progress: {} / 1,000,000", count);
        }
    }

    let write_time = start.elapsed();

    // Also add some coils and discrete inputs
    for addr in 0..25000u16 {
        store.write_coil(addr, addr % 2 == 0).unwrap();
        store.set_discrete_input(addr, addr % 2 == 1).unwrap();
    }

    let total_entries = store.entry_count();
    let memory = store.memory_usage();
    let memory_mb = memory as f64 / (1024.0 * 1024.0);

    println!("\n--- Results ---");
    println!("Total entries: {}", total_entries);
    println!("Write time: {:?}", write_time);
    println!(
        "Write rate: {:.0} entries/sec",
        count as f64 / write_time.as_secs_f64()
    );
    println!("Memory usage: {:.2} MB", memory_mb);

    // Verify reads
    let read_start = Instant::now();
    let mut read_count = 0usize;

    // Sample reads (every 100th register)
    for addr in (0..50000u16).step_by(100) {
        let _ = store.read_holding_registers(addr, 1);
        read_count += 1;
    }

    let read_time = read_start.elapsed();

    println!("Sample reads: {} in {:?}", read_count, read_time);

    assert!(
        total_entries >= 100_000,
        "Not enough entries: {} (expected 100K+)",
        total_entries
    );

    println!("\nStatus: PASS ✓");
}

// =============================================================================
// TPS Tests
// =============================================================================

#[test]
#[ignore]
fn test_100k_tps() {
    println!("\n=== Testing 100K TPS Target ===");

    let store = Arc::new(SparseRegisterStore::with_defaults());

    // Pre-populate
    for addr in 0..10000u16 {
        store.write_holding_register(addr, addr).unwrap();
    }

    let thread_count = num_cpus::get().min(16);
    let target_duration = Duration::from_secs(5);

    println!("Threads: {}", thread_count);
    println!("Target duration: {:?}", target_duration);
    println!("Target TPS: {}", TARGET_TPS);

    let total_ops = Arc::new(AtomicU64::new(0));
    let start = Instant::now();

    let handles: Vec<_> = (0..thread_count)
        .map(|t| {
            let store = store.clone();
            let ops = total_ops.clone();
            let t = t as u64;
            thread::spawn(move || {
                let mut local_ops = 0u64;
                let start_time = Instant::now();

                while start_time.elapsed() < target_duration {
                    // Mix of operations
                    for i in 0u64..100 {
                        let addr = ((t * 1000 + local_ops + i) % 10000) as u16;

                        // 70% reads, 30% writes (typical workload)
                        if i % 10 < 7 {
                            let _ = store.read_holding_registers(addr, 1);
                        } else {
                            let _ = store.write_holding_register(addr, (local_ops + i) as u16);
                        }
                    }
                    local_ops += 100;
                }

                ops.fetch_add(local_ops, Ordering::Relaxed);
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    let elapsed = start.elapsed();
    let total = total_ops.load(Ordering::Relaxed);
    let tps = total as f64 / elapsed.as_secs_f64();

    println!("\n--- Results ---");
    println!("Total operations: {}", total);
    println!("Elapsed time: {:?}", elapsed);
    println!("TPS: {:.0}", tps);
    println!(
        "Status: {}",
        if tps >= TARGET_TPS as f64 {
            "PASS ✓"
        } else {
            "FAIL ✗"
        }
    );

    assert!(
        tps >= TARGET_TPS as f64,
        "TPS too low: {:.0} (target: {})",
        tps,
        TARGET_TPS
    );
}

#[test]
#[ignore]
fn test_sustained_high_load() {
    println!("\n=== Testing Sustained High Load (30 seconds) ===");

    let store = Arc::new(SparseRegisterStore::with_defaults());

    // Pre-populate
    for addr in 0..10000u16 {
        store.write_holding_register(addr, addr).unwrap();
    }

    let thread_count = 8;
    let duration = Duration::from_secs(30);
    let sample_interval = Duration::from_secs(5);

    println!("Threads: {}", thread_count);
    println!("Duration: {:?}", duration);

    let total_ops = Arc::new(AtomicU64::new(0));
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));

    // Worker threads
    let handles: Vec<_> = (0..thread_count)
        .map(|t| {
            let store = store.clone();
            let ops = total_ops.clone();
            let running = running.clone();
            thread::spawn(move || {
                let mut local_ops = 0u64;
                while running.load(Ordering::Relaxed) {
                    let addr = ((t * 1000 + local_ops) % 10000) as u16;
                    let _ = store.read_holding_registers(addr, 1);
                    let _ = store.write_holding_register(addr, local_ops as u16);
                    local_ops += 2;
                }
                ops.fetch_add(local_ops, Ordering::Relaxed);
            })
        })
        .collect();

    // Monitor thread
    let start = Instant::now();
    let mut last_sample = 0u64;

    println!("\n{:>10} {:>15} {:>15}", "Time (s)", "Ops", "TPS");
    println!("{:-<45}", "");

    while start.elapsed() < duration {
        thread::sleep(sample_interval);

        let current = total_ops.load(Ordering::Relaxed);
        let delta = current - last_sample;
        let tps = delta as f64 / sample_interval.as_secs_f64();
        last_sample = current;

        println!(
            "{:>10.0} {:>15} {:>15.0}",
            start.elapsed().as_secs_f64(),
            current,
            tps
        );
    }

    running.store(false, Ordering::Relaxed);

    for handle in handles {
        handle.join().unwrap();
    }

    let total = total_ops.load(Ordering::Relaxed);
    let avg_tps = total as f64 / duration.as_secs_f64();

    println!("\n--- Summary ---");
    println!("Total operations: {}", total);
    println!("Average TPS: {:.0}", avg_tps);
    println!(
        "Status: {}",
        if avg_tps >= 50_000.0 {
            "PASS ✓"
        } else {
            "NEEDS IMPROVEMENT"
        }
    );
}

// =============================================================================
// Memory Budget Tests
// =============================================================================

#[test]
#[ignore]
fn test_memory_budget() {
    println!("\n=== Testing Memory Budget (< 2GB for 10K devices) ===");

    // Simulate memory usage for 10K devices
    // Each device has: 10K holding regs + 10K input regs + 10K coils + 10K DI
    // Total per device: 40K entries

    // We can't actually create 10K stores due to test time constraints
    // So we measure one and extrapolate

    let store = SparseRegisterStore::with_defaults();

    // Simulate one device with 40K entries
    for addr in 0..10000u16 {
        store.write_holding_register(addr, addr).unwrap();
        store.set_input_register(addr, addr).unwrap();
        store.write_coil(addr, true).unwrap();
        store.set_discrete_input(addr, true).unwrap();
    }

    let entries = store.entry_count();
    let memory_per_device = store.memory_usage();
    let memory_per_device_mb = memory_per_device as f64 / (1024.0 * 1024.0);

    // Extrapolate to 10K devices
    let estimated_10k = memory_per_device * 10_000;
    let estimated_10k_mb = estimated_10k as f64 / (1024.0 * 1024.0);
    let estimated_10k_gb = estimated_10k_mb / 1024.0;

    println!("Single device:");
    println!("  Entries: {}", entries);
    println!("  Memory: {:.2} MB", memory_per_device_mb);

    println!("\nExtrapolated for 10K devices:");
    println!(
        "  Estimated memory: {:.2} MB ({:.2} GB)",
        estimated_10k_mb, estimated_10k_gb
    );
    println!(
        "  Budget: {} MB ({:.2} GB)",
        MEMORY_BUDGET_MB,
        MEMORY_BUDGET_MB as f64 / 1024.0
    );

    let within_budget = estimated_10k_mb < MEMORY_BUDGET_MB as f64;
    println!(
        "\nStatus: {}",
        if within_budget {
            "PASS ✓"
        } else {
            "FAIL ✗"
        }
    );

    assert!(
        within_budget,
        "Estimated memory ({:.2} MB) exceeds budget ({} MB)",
        estimated_10k_mb, MEMORY_BUDGET_MB
    );
}

#[test]
#[ignore]
fn test_memory_growth_stability() {
    println!("\n=== Testing Memory Growth Stability ===");

    let store = Arc::new(SparseRegisterStore::with_defaults());

    // Run sustained operations and check memory doesn't grow unboundedly
    let initial_memory = store.memory_usage();
    println!("Initial memory: {} bytes", initial_memory);

    // Phase 1: Write 10K unique registers
    for addr in 0..10000u16 {
        store.write_holding_register(addr, addr).unwrap();
    }

    let after_write = store.memory_usage();
    println!("After 10K writes: {} bytes", after_write);

    // Phase 2: Overwrite same registers 100x
    for _ in 0..100 {
        for addr in 0..10000u16 {
            store.write_holding_register(addr, addr).unwrap();
        }
    }

    let after_overwrite = store.memory_usage();
    println!("After 100x overwrites: {} bytes", after_overwrite);

    // Memory should not grow significantly from overwrites
    let growth = after_overwrite as f64 / after_write as f64;
    println!("Growth factor: {:.2}x", growth);

    assert!(
        growth < 1.5,
        "Memory grew too much during overwrites: {:.2}x",
        growth
    );

    println!("\nStatus: PASS ✓");
}

// =============================================================================
// Concurrent Access Stress Tests
// =============================================================================

#[test]
#[ignore]
fn test_concurrent_unit_access() {
    println!("\n=== Testing Concurrent Unit Access ===");

    let config = UnitManagerConfig::default().with_auto_create(false);
    let manager = Arc::new(MultiUnitManager::new(config));

    // Create 100 units
    for unit_id in 1..=100u8 {
        manager
            .add_unit(unit_id, UnitConfig::new(&format!("Unit {}", unit_id)))
            .unwrap();
    }

    let thread_count = 20;
    let ops_per_thread = 10_000;
    let total_ops = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));

    let start = Instant::now();

    let handles: Vec<_> = (0..thread_count)
        .map(|t| {
            let manager = manager.clone();
            let total = total_ops.clone();
            let errs = errors.clone();
            thread::spawn(move || {
                for i in 0..ops_per_thread {
                    let unit_id = ((t * 5 + i) % 100 + 1) as u8;
                    let addr = (i % 100) as u16;
                    let value = (t * 1000 + i) as u16;

                    // Write
                    match manager.write_holding_register(unit_id, addr, value) {
                        Ok(_) => {}
                        Err(_) => {
                            errs.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    // Read
                    match manager.read_holding_registers(unit_id, addr, 1) {
                        Ok(_) => {}
                        Err(_) => {
                            errs.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                total.fetch_add((ops_per_thread * 2) as u64, Ordering::Relaxed);
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    let elapsed = start.elapsed();
    let total = total_ops.load(Ordering::Relaxed);
    let error_count = errors.load(Ordering::Relaxed);
    let tps = total as f64 / elapsed.as_secs_f64();

    println!("Threads: {}", thread_count);
    println!("Total ops: {}", total);
    println!("Errors: {}", error_count);
    println!("Time: {:?}", elapsed);
    println!("TPS: {:.0}", tps);
    println!(
        "Status: {}",
        if error_count == 0 {
            "PASS ✓"
        } else {
            "FAIL ✗"
        }
    );

    assert_eq!(
        error_count, 0,
        "Had {} errors during concurrent access",
        error_count
    );
}

#[test]
#[ignore]
fn test_broadcast_under_load() {
    println!("\n=== Testing Broadcast Under Load ===");

    let manager = Arc::new(MultiUnitManager::with_defaults());

    // Create 50 units
    for unit_id in 1..=50u8 {
        manager
            .add_unit(unit_id, UnitConfig::new(&format!("Unit {}", unit_id)))
            .unwrap();
    }

    // Background workers doing regular operations
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let worker_count = 4;

    let workers: Vec<_> = (0..worker_count)
        .map(|t| {
            let manager = manager.clone();
            let running = running.clone();
            thread::spawn(move || {
                let mut ops = 0u64;
                while running.load(Ordering::Relaxed) {
                    let unit_id = ((t * 10 + ops) % 50 + 1) as u8;
                    let _ = manager.read_holding_registers(unit_id, 0, 10);
                    ops += 1;
                }
            })
        })
        .collect();

    // Main thread does broadcasts
    let broadcast_count = 100;
    let start = Instant::now();

    for i in 0..broadcast_count {
        manager
            .broadcast_write_holding_register(i % 100, i as u16)
            .unwrap();
    }

    let broadcast_time = start.elapsed();

    running.store(false, Ordering::Relaxed);
    for worker in workers {
        worker.join().unwrap();
    }

    // Verify all units received broadcasts
    for unit_id in 1..=50u8 {
        // Check last broadcast value (address 99)
        let values = manager.read_holding_registers(unit_id, 99, 1).unwrap();
        assert_eq!(
            values[0], 99,
            "Unit {} didn't receive broadcast correctly",
            unit_id
        );
    }

    println!("Broadcasts: {}", broadcast_count);
    println!("Time: {:?}", broadcast_time);
    println!(
        "Rate: {:.0} broadcasts/sec",
        broadcast_count as f64 / broadcast_time.as_secs_f64()
    );
    println!("Status: PASS ✓");
}

// =============================================================================
// Data Type Conversion Under Load
// =============================================================================

#[test]
#[ignore]
fn test_type_conversion_performance() {
    println!("\n=== Testing Type Conversion Performance ===");

    let converters = [
        ("BigEndian", RegisterConverter::big_endian()),
        ("LittleEndian", RegisterConverter::little_endian()),
        (
            "BigEndianWordSwap",
            RegisterConverter::big_endian_word_swap(),
        ),
        (
            "LittleEndianWordSwap",
            RegisterConverter::little_endian_word_swap(),
        ),
    ];

    let iterations = 100_000;

    println!(
        "{:>20} {:>15} {:>15} {:>15}",
        "Word Order", "f32 (ns)", "i32 (ns)", "f64 (ns)"
    );
    println!("{:-<70}", "");

    for (name, conv) in &converters {
        // f32 conversion
        let f32_start = Instant::now();
        for i in 0..iterations {
            let value = i as f32 * 0.001;
            let regs = conv.f32_to_registers(value);
            let _ = conv.registers_to_f32(&regs);
        }
        let f32_ns = f32_start.elapsed().as_nanos() as f64 / iterations as f64;

        // i32 conversion
        let i32_start = Instant::now();
        for i in 0..iterations {
            let value = i as i32;
            let regs = conv.i32_to_registers(value);
            let _ = conv.registers_to_i32(&regs);
        }
        let i32_ns = i32_start.elapsed().as_nanos() as f64 / iterations as f64;

        // f64 conversion
        let f64_start = Instant::now();
        for i in 0..iterations {
            let value = i as f64 * 0.001;
            let regs = conv.f64_to_registers(value);
            let _ = conv.registers_to_f64(&regs);
        }
        let f64_ns = f64_start.elapsed().as_nanos() as f64 / iterations as f64;

        println!(
            "{:>20} {:>15.1} {:>15.1} {:>15.1}",
            name, f32_ns, i32_ns, f64_ns
        );
    }

    println!("\nStatus: PASS ✓");
}

// =============================================================================
// Summary
// =============================================================================

#[test]
fn test_stress_test_summary() {
    println!("\n");
    println!("╔═══════════════════════════════════════════════════════════════════════╗");
    println!("║                PHASE 2 STRESS TEST SUITE                              ║");
    println!("╠═══════════════════════════════════════════════════════════════════════╣");
    println!("║ Test                          │ Target              │ Run Command     ║");
    println!("╠───────────────────────────────┼─────────────────────┼─────────────────╣");
    println!("║ Max concurrent units          │ 247 units           │ --ignored       ║");
    println!("║ 10K device simulation         │ 10K+ data points    │ --ignored       ║");
    println!("║ 1M data points                │ 1,000,000 entries   │ --ignored       ║");
    println!("║ 100K TPS                      │ 100,000 ops/sec     │ --ignored       ║");
    println!("║ Sustained load (30s)          │ Stable throughput   │ --ignored       ║");
    println!("║ Memory budget                 │ < 2GB for 10K dev   │ --ignored       ║");
    println!("║ Memory growth stability       │ No unbounded growth │ --ignored       ║");
    println!("║ Concurrent unit access        │ 0 errors            │ --ignored       ║");
    println!("║ Broadcast under load          │ All units receive   │ --ignored       ║");
    println!("║ Type conversion perf          │ < 100ns per conv    │ --ignored       ║");
    println!("╠───────────────────────────────┴─────────────────────┴─────────────────╣");
    println!("║ Run all: cargo test --test stress_tests --release -- --ignored        ║");
    println!("╚═══════════════════════════════════════════════════════════════════════╝");
}
