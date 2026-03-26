//! Tests for the registers module.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;

// =============================================================================
// SparseRegisterStore Tests
// =============================================================================

mod sparse_store_tests {
    use super::*;

    #[test]
    fn test_create_with_defaults() {
        let store = SparseRegisterStore::with_defaults();
        assert_eq!(store.entry_count(), 0); // Lazy initialization
        assert!(store.memory_usage() > 0); // Base DashMap overhead
    }

    #[test]
    fn test_coil_operations() {
        let store = SparseRegisterStore::with_defaults();

        // Write single coil
        store.write_coil(0, true).unwrap();
        let values = store.read_coils(0, 1).unwrap();
        assert_eq!(values, vec![true]);

        // Write multiple coils
        store.write_coils(10, &[true, false, true]).unwrap();
        let values = store.read_coils(10, 3).unwrap();
        assert_eq!(values, vec![true, false, true]);

        // Verify sparse storage
        assert_eq!(store.entry_count_for(RegisterType::Coil), 4);
    }

    #[test]
    fn test_discrete_input_operations() {
        let store = SparseRegisterStore::with_defaults();

        // Set (simulator internal)
        store.set_discrete_input(0, true).unwrap();
        let values = store.read_discrete_inputs(0, 1).unwrap();
        assert_eq!(values, vec![true]);

        // Set multiple
        store.set_discrete_inputs(10, &[true, false]).unwrap();
        let values = store.read_discrete_inputs(10, 2).unwrap();
        assert_eq!(values, vec![true, false]);
    }

    #[test]
    fn test_holding_register_operations() {
        let store = SparseRegisterStore::with_defaults();

        // Write single
        store.write_holding_register(0, 12345).unwrap();
        let values = store.read_holding_registers(0, 1).unwrap();
        assert_eq!(values, vec![12345]);

        // Write multiple
        store.write_holding_registers(10, &[100, 200, 300]).unwrap();
        let values = store.read_holding_registers(10, 3).unwrap();
        assert_eq!(values, vec![100, 200, 300]);

        // Verify sparse storage
        assert_eq!(store.entry_count_for(RegisterType::HoldingRegister), 4);
    }

    #[test]
    fn test_input_register_operations() {
        let store = SparseRegisterStore::with_defaults();

        // Set (simulator internal)
        store.set_input_register(0, 54321).unwrap();
        let values = store.read_input_registers(0, 1).unwrap();
        assert_eq!(values, vec![54321]);

        // Set multiple
        store.set_input_registers(10, &[111, 222]).unwrap();
        let values = store.read_input_registers(10, 2).unwrap();
        assert_eq!(values, vec![111, 222]);
    }

    #[test]
    fn test_invalid_address() {
        let config = RegisterStoreConfig::new(
            AddressRange::new(0, 99),
            AddressRange::new(0, 99),
            AddressRange::new(0, 99),
            AddressRange::new(0, 99),
        );
        let store = SparseRegisterStore::new(config);

        // Address out of range
        let result = store.read_coils(100, 1);
        assert!(result.is_err());

        // Quantity causing overflow
        let result = store.read_coils(99, 2);
        assert!(result.is_err());

        // Zero quantity
        let result = store.read_coils(0, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_float_operations() {
        let store = SparseRegisterStore::with_defaults();

        // Write f32
        let f32_value: f32 = 3.14159;
        store.write_f32(0, f32_value).unwrap();
        let read_value = store.read_f32(0).unwrap();
        assert!((read_value - f32_value).abs() < 0.0001);

        // Write f64
        let f64_value: f64 = 2.718281828459045;
        store.write_f64(10, f64_value).unwrap();
        let read_value = store.read_f64(10).unwrap();
        assert!((read_value - f64_value).abs() < 0.000001);
    }

    #[test]
    fn test_reset() {
        let store = SparseRegisterStore::with_defaults();

        // Write some data
        store.write_holding_register(0, 12345).unwrap();
        store.write_coil(0, true).unwrap();
        assert!(store.entry_count() > 0);

        // Reset
        store.reset();
        assert_eq!(store.entry_count(), 0);

        // Values should return to default (0/false for lazy init)
        let values = store.read_holding_registers(0, 1).unwrap();
        assert_eq!(values, vec![0]);
    }

    #[test]
    fn test_snapshot_and_restore() {
        let config = RegisterStoreConfig::minimal();

        // Create and populate store
        let store1 = SparseRegisterStore::new(config.clone());
        store1.write_holding_register(0, 100).unwrap();
        store1.write_holding_register(10, 200).unwrap();
        store1.write_coil(5, true).unwrap();

        // Create snapshot
        let snapshot = store1.snapshot();

        // Create new store from snapshot
        let config2 = config.with_initialization(InitializationMode::Snapshot(snapshot.clone()));
        let store2 = SparseRegisterStore::new(config2);

        // Verify data
        assert_eq!(store2.read_holding_registers(0, 1).unwrap(), vec![100]);
        assert_eq!(store2.read_holding_registers(10, 1).unwrap(), vec![200]);
        assert_eq!(store2.read_coils(5, 1).unwrap(), vec![true]);
    }

    #[test]
    fn test_exists_and_remove() {
        let store = SparseRegisterStore::with_defaults();

        // Initially doesn't exist
        assert!(!store.exists(RegisterType::HoldingRegister, 0));

        // Write creates entry
        store.write_holding_register(0, 100).unwrap();
        assert!(store.exists(RegisterType::HoldingRegister, 0));

        // Remove
        assert!(store.remove(RegisterType::HoldingRegister, 0));
        assert!(!store.exists(RegisterType::HoldingRegister, 0));

        // Remove non-existent
        assert!(!store.remove(RegisterType::HoldingRegister, 999));
    }

    #[test]
    fn test_memory_usage() {
        let store = SparseRegisterStore::with_defaults();

        let initial_memory = store.memory_usage();
        assert!(initial_memory > 0); // Base overhead

        // Add entries
        for i in 0..1000u16 {
            store.write_holding_register(i, i).unwrap();
        }

        let after_memory = store.memory_usage();
        assert!(after_memory > initial_memory);

        // Memory should grow proportionally
        let entry_overhead = (after_memory - initial_memory) / 1000;
        assert!(entry_overhead >= 20 && entry_overhead <= 50); // Reasonable per-entry overhead
    }

    #[test]
    fn test_default_values() {
        let config = RegisterStoreConfig::new(
            AddressRange::new(0, 99),
            AddressRange::new(0, 99),
            AddressRange::new(0, 99),
            AddressRange::new(0, 99),
        );

        // Customize default values
        let mut config = config;
        config.holding_registers.default_value = DefaultValue::Value(42);
        config.coils.default_value = DefaultValue::Value(1); // true

        let store = SparseRegisterStore::new(config);

        // Uninitialized registers should return default values
        let values = store.read_holding_registers(50, 1).unwrap();
        assert_eq!(values, vec![42]);

        let coils = store.read_coils(50, 1).unwrap();
        assert_eq!(coils, vec![true]);
    }

    #[test]
    fn test_eager_initialization() {
        let mut config = RegisterStoreConfig::new(
            AddressRange::new(0, 9),
            AddressRange::new(0, 9),
            AddressRange::new(0, 9),
            AddressRange::new(0, 9),
        );
        config.initialization = InitializationMode::Eager;

        let store = SparseRegisterStore::new(config);

        // All registers should be pre-populated
        assert_eq!(store.entry_count_for(RegisterType::Coil), 10);
        assert_eq!(store.entry_count_for(RegisterType::DiscreteInput), 10);
        assert_eq!(store.entry_count_for(RegisterType::HoldingRegister), 10);
        assert_eq!(store.entry_count_for(RegisterType::InputRegister), 10);
    }
}

// =============================================================================
// Callback Tests
// =============================================================================

mod callback_tests {
    use super::*;

    #[test]
    fn test_write_callback_notification() {
        let store = SparseRegisterStore::with_defaults();
        let counter = Arc::new(AtomicUsize::new(0));

        // Add callback
        let counter_clone = counter.clone();
        store.add_write_callback(Arc::new(WriteCallbackFn::new(
            "test_counter",
            move |_ctx| {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            },
        )));

        // Write should trigger callback
        store.write_holding_register(0, 100).unwrap();
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        // Multiple writes should trigger multiple callbacks
        store.write_holding_registers(10, &[1, 2, 3]).unwrap();
        assert_eq!(counter.load(Ordering::Relaxed), 4); // 1 + 3

        // Disable callbacks
        store.set_callbacks_enabled(false);
        store.write_holding_register(20, 200).unwrap();
        assert_eq!(counter.load(Ordering::Relaxed), 4); // Still 4

        // Re-enable
        store.set_callbacks_enabled(true);
        store.write_holding_register(30, 300).unwrap();
        assert_eq!(counter.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn test_read_callback_notification() {
        let store = SparseRegisterStore::with_defaults();
        let counter = Arc::new(AtomicUsize::new(0));

        // Add callback
        let counter_clone = counter.clone();
        store.add_read_callback(Arc::new(ReadCallbackFn::new(
            "test_read",
            move |_ctx, _values| {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            },
        )));

        // Read should trigger callback
        store.read_holding_registers(0, 1).unwrap();
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        // Read multiple
        store.read_holding_registers(0, 10).unwrap();
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_callback_context() {
        let store = SparseRegisterStore::with_defaults();
        let captured = Arc::new(parking_lot::RwLock::new(Vec::new()));

        // Capture write context
        let captured_clone = captured.clone();
        store.add_write_callback(Arc::new(WriteCallbackFn::new("capture", move |ctx| {
            captured_clone.write().push((
                ctx.address,
                ctx.old_value.as_word(),
                ctx.new_value.as_word(),
            ));
        })));

        // First write (old value is default 0)
        store.write_holding_register(100, 50).unwrap();

        // Second write (old value is 50)
        store.write_holding_register(100, 75).unwrap();

        let events = captured.read();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], (100, 0, 50));
        assert_eq!(events[1], (100, 50, 75));
    }

    #[test]
    fn test_callback_disabled_by_config() {
        let config = RegisterStoreConfig::default().without_callbacks();
        let store = SparseRegisterStore::new(config);

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        store.add_write_callback(Arc::new(WriteCallbackFn::new("test", move |_| {
            counter_clone.fetch_add(1, Ordering::Relaxed);
        })));

        // Callbacks should be disabled
        store.write_holding_register(0, 100).unwrap();
        assert_eq!(counter.load(Ordering::Relaxed), 0);
    }
}

// =============================================================================
// Concurrency Tests
// =============================================================================

mod concurrency_tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_concurrent_reads_and_writes() {
        let store = Arc::new(SparseRegisterStore::with_defaults());
        let mut handles = vec![];

        // Spawn multiple reader threads
        for t in 0..4 {
            let store = store.clone();
            handles.push(thread::spawn(move || {
                for i in 0..1000 {
                    let addr = ((t * 1000 + i) % 1000) as u16;
                    let _ = store.read_holding_registers(addr, 1);
                }
            }));
        }

        // Spawn multiple writer threads
        for t in 0..4 {
            let store = store.clone();
            handles.push(thread::spawn(move || {
                for i in 0..1000 {
                    let addr = ((t * 1000 + i) % 1000) as u16;
                    store.write_holding_register(addr, i as u16).unwrap();
                }
            }));
        }

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        // Store should be in a consistent state
        assert!(store.entry_count_for(RegisterType::HoldingRegister) <= 1000);
    }

    #[test]
    fn test_concurrent_different_types() {
        let store = Arc::new(SparseRegisterStore::with_defaults());
        let mut handles = vec![];

        // Coil writer
        let store1 = store.clone();
        handles.push(thread::spawn(move || {
            for i in 0..1000u16 {
                store1.write_coil(i, i % 2 == 0).unwrap();
            }
        }));

        // Holding register writer
        let store2 = store.clone();
        handles.push(thread::spawn(move || {
            for i in 0..1000u16 {
                store2.write_holding_register(i, i).unwrap();
            }
        }));

        // Input register setter
        let store3 = store.clone();
        handles.push(thread::spawn(move || {
            for i in 0..1000u16 {
                store3.set_input_register(i, i * 2).unwrap();
            }
        }));

        // Reader
        let store4 = store.clone();
        handles.push(thread::spawn(move || {
            for i in 0..1000u16 {
                let _ = store4.read_coils(i % 100, 1);
                let _ = store4.read_holding_registers(i % 100, 1);
                let _ = store4.read_input_registers(i % 100, 1);
            }
        }));

        for handle in handles {
            handle.join().unwrap();
        }
    }
}

// =============================================================================
// Config Tests
// =============================================================================

mod config_tests {
    use super::*;

    #[test]
    fn test_address_range_count() {
        assert_eq!(AddressRange::new(0, 99).count(), 100);
        assert_eq!(AddressRange::new(10, 19).count(), 10);
        assert_eq!(AddressRange::new(0, 0).count(), 1);
        assert_eq!(AddressRange::new(0, 65534).count(), 65535);
    }

    #[test]
    fn test_config_presets() {
        let minimal = RegisterStoreConfig::minimal();
        assert_eq!(minimal.coils.range.count(), 100);

        let large = RegisterStoreConfig::large();
        assert_eq!(large.coils.range.count(), 65535);

        let default = RegisterStoreConfig::default();
        assert_eq!(default.coils.range.count(), 10000);
    }

    #[test]
    fn test_config_builder_pattern() {
        let config = RegisterStoreConfig::default()
            .with_initialization(InitializationMode::Eager)
            .without_callbacks();

        assert_eq!(config.initialization, InitializationMode::Eager);
        assert!(!config.callbacks_enabled);
    }
}

// =============================================================================
// RegisterType Tests
// =============================================================================

mod register_type_tests {
    use super::*;

    #[test]
    fn test_function_codes() {
        assert_eq!(RegisterType::Coil.read_function_code(), 0x01);
        assert_eq!(RegisterType::DiscreteInput.read_function_code(), 0x02);
        assert_eq!(RegisterType::HoldingRegister.read_function_code(), 0x03);
        assert_eq!(RegisterType::InputRegister.read_function_code(), 0x04);

        assert_eq!(RegisterType::Coil.write_single_function_code(), Some(0x05));
        assert_eq!(
            RegisterType::HoldingRegister.write_single_function_code(),
            Some(0x06)
        );
        assert_eq!(
            RegisterType::DiscreteInput.write_single_function_code(),
            None
        );
        assert_eq!(
            RegisterType::InputRegister.write_single_function_code(),
            None
        );
    }

    #[test]
    fn test_type_properties() {
        // Bit types
        assert!(RegisterType::Coil.is_bit_type());
        assert!(RegisterType::DiscreteInput.is_bit_type());
        assert!(!RegisterType::HoldingRegister.is_bit_type());

        // Word types
        assert!(RegisterType::HoldingRegister.is_word_type());
        assert!(RegisterType::InputRegister.is_word_type());
        assert!(!RegisterType::Coil.is_word_type());

        // Writable
        assert!(RegisterType::Coil.is_writable());
        assert!(RegisterType::HoldingRegister.is_writable());
        assert!(!RegisterType::DiscreteInput.is_writable());
        assert!(!RegisterType::InputRegister.is_writable());
    }
}
