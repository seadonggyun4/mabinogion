//! Integration tests for Task 2.5: Advanced Features
//!
//! Tests for:
//! - Multi-Unit ID support
//! - Data type conversion with various word orders
//! - Broadcast (Unit 0) functionality

use mabi_modbus::{
    types::{RegisterConverter, RegisterDataType, TypedValue, WordOrder},
    unit::{BroadcastMode, MultiUnitManager, UnitConfig, UnitManagerConfig},
};

// =============================================================================
// Word Order Tests
// =============================================================================

#[test]
fn test_word_order_all_types() {
    for order in WordOrder::all() {
        let conv = RegisterConverter::new(*order);

        // Test i32
        let i32_val: i32 = 0x12345678;
        let i32_regs = conv.i32_to_registers(i32_val);
        let i32_result = conv.registers_to_i32(&i32_regs);
        assert_eq!(i32_result, i32_val, "i32 round-trip failed for {:?}", order);

        // Test f32
        let f32_val: f32 = 123.456;
        let f32_regs = conv.f32_to_registers(f32_val);
        let f32_result = conv.registers_to_f32(&f32_regs);
        assert!(
            (f32_result - f32_val).abs() < 0.001,
            "f32 round-trip failed for {:?}",
            order
        );

        // Test f64
        let f64_val: f64 = 123456.789012;
        let f64_regs = conv.f64_to_registers(f64_val);
        let f64_result = conv.registers_to_f64(&f64_regs);
        assert!(
            (f64_result - f64_val).abs() < 0.000001,
            "f64 round-trip failed for {:?}",
            order
        );
    }
}

#[test]
fn test_word_order_known_patterns() {
    // Test Big Endian: 0x12345678 -> [0x1234, 0x5678]
    let be = RegisterConverter::big_endian();
    let be_regs = be.i32_to_registers(0x12345678);
    assert_eq!(be_regs, [0x1234, 0x5678], "Big Endian pattern mismatch");

    // Test Big Endian Word Swap: 0x12345678 -> [0x5678, 0x1234]
    let bes = RegisterConverter::big_endian_word_swap();
    let bes_regs = bes.i32_to_registers(0x12345678);
    assert_eq!(
        bes_regs,
        [0x5678, 0x1234],
        "Big Endian Word Swap pattern mismatch"
    );
}

#[test]
fn test_word_order_interoperability() {
    // Simulate reading from a BE device and writing to a LE device
    let be_conv = RegisterConverter::big_endian();
    let le_conv = RegisterConverter::little_endian();

    let original_value: f32 = 42.5;

    // Device A (BE) writes value
    let be_registers = be_conv.f32_to_registers(original_value);

    // Raw register values are different representations of the same data
    // To transfer, we need to read as bytes and reinterpret

    // Read back using BE converter
    let be_value = be_conv.registers_to_f32(&be_registers);
    assert!((be_value - original_value).abs() < 0.001);

    // If we try to read BE data as LE, we get wrong value (expected)
    // This demonstrates why word order configuration is important
}

// =============================================================================
// Multi-Unit Manager Tests
// =============================================================================

#[test]
fn test_multi_unit_basic() {
    let manager = MultiUnitManager::with_defaults();

    manager.add_unit(1, UnitConfig::new("PLC #1")).unwrap();
    manager.add_unit(2, UnitConfig::new("VFD #1")).unwrap();
    manager.add_unit(3, UnitConfig::new("Sensor #1")).unwrap();

    assert_eq!(manager.unit_count(), 3);
    assert!(manager.has_unit(1));
    assert!(manager.has_unit(2));
    assert!(manager.has_unit(3));
    assert!(!manager.has_unit(4));
}

#[test]
fn test_multi_unit_read_write() {
    let manager = MultiUnitManager::with_defaults();

    manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();
    manager.add_unit(2, UnitConfig::new("Unit 2")).unwrap();

    // Write to unit 1
    manager.write_holding_register(1, 100, 1111).unwrap();

    // Write to unit 2
    manager.write_holding_register(2, 100, 2222).unwrap();

    // Read from unit 1
    let v1 = manager.read_holding_registers(1, 100, 1).unwrap();
    assert_eq!(v1[0], 1111);

    // Read from unit 2
    let v2 = manager.read_holding_registers(2, 100, 1).unwrap();
    assert_eq!(v2[0], 2222);

    // Units are independent
    assert_ne!(v1[0], v2[0]);
}

#[test]
fn test_multi_unit_word_order_per_unit() {
    let manager = MultiUnitManager::with_defaults();

    // Unit 1 with Big Endian
    manager
        .add_unit(1, UnitConfig::with_word_order("BE Unit", WordOrder::BigEndian))
        .unwrap();

    // Unit 2 with Big Endian Word Swap
    manager
        .add_unit(
            2,
            UnitConfig::with_word_order("BES Unit", WordOrder::BigEndianWordSwap),
        )
        .unwrap();

    let unit1 = manager.get_unit(1).unwrap();
    let unit2 = manager.get_unit(2).unwrap();

    assert_eq!(unit1.word_order(), WordOrder::BigEndian);
    assert_eq!(unit2.word_order(), WordOrder::BigEndianWordSwap);

    // Same f32 value produces different register patterns
    let value: f32 = 123.456;

    let regs1 = unit1.converter().f32_to_registers(value);
    let regs2 = unit2.converter().f32_to_registers(value);

    // Registers should differ (word swap)
    assert_ne!(regs1, regs2);

    // But both should convert back to same value
    let v1 = unit1.converter().registers_to_f32(&regs1);
    let v2 = unit2.converter().registers_to_f32(&regs2);

    assert!((v1 - value).abs() < 0.001);
    assert!((v2 - value).abs() < 0.001);
}

// =============================================================================
// Broadcast Tests
// =============================================================================

#[test]
fn test_broadcast_write_all() {
    let manager = MultiUnitManager::with_defaults();

    manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();
    manager.add_unit(2, UnitConfig::new("Unit 2")).unwrap();
    manager.add_unit(3, UnitConfig::new("Unit 3")).unwrap();

    // Broadcast write (unit_id = 0) writes to all units
    manager.write_holding_register(0, 500, 0xABCD).unwrap();

    // All units should have the value
    for unit_id in [1, 2, 3] {
        let values = manager.read_holding_registers(unit_id, 500, 1).unwrap();
        assert_eq!(
            values[0], 0xABCD,
            "Unit {} should have broadcast value",
            unit_id
        );
    }
}

#[test]
fn test_broadcast_selective() {
    let mut config = UnitManagerConfig::default();
    config.broadcast_mode = BroadcastMode::SelectiveList(vec![1, 3]); // Only units 1 and 3

    let manager = MultiUnitManager::new(config);

    manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();
    manager.add_unit(2, UnitConfig::new("Unit 2")).unwrap();
    manager.add_unit(3, UnitConfig::new("Unit 3")).unwrap();

    // Broadcast write
    manager.broadcast_write_holding_register(600, 0x1234).unwrap();

    // Units 1 and 3 should have the value
    let v1 = manager.read_holding_registers(1, 600, 1).unwrap();
    let v3 = manager.read_holding_registers(3, 600, 1).unwrap();
    assert_eq!(v1[0], 0x1234);
    assert_eq!(v3[0], 0x1234);

    // Unit 2 should NOT have the value (not in selective list)
    let v2 = manager.read_holding_registers(2, 600, 1).unwrap();
    assert_ne!(v2[0], 0x1234);
}

#[test]
fn test_broadcast_disabled_unit() {
    let manager = MultiUnitManager::with_defaults();

    manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();
    manager
        .add_unit(2, UnitConfig::new("Unit 2").with_broadcast(false))
        .unwrap();
    manager.add_unit(3, UnitConfig::new("Unit 3")).unwrap();

    // Broadcast write
    manager.broadcast_write_coil(100, true).unwrap();

    // Units 1 and 3 should have the value
    let v1 = manager.read_coils(1, 100, 1).unwrap();
    let v3 = manager.read_coils(3, 100, 1).unwrap();
    assert!(v1[0]);
    assert!(v3[0]);

    // Unit 2 should NOT have the value (broadcast disabled)
    let v2 = manager.read_coils(2, 100, 1).unwrap();
    assert!(!v2[0]); // Default is false
}

#[test]
fn test_broadcast_mode_disabled() {
    let mut config = UnitManagerConfig::default();
    config.broadcast_mode = BroadcastMode::Disabled;

    let manager = MultiUnitManager::new(config);

    manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();
    manager.add_unit(2, UnitConfig::new("Unit 2")).unwrap();

    // Initialize with known values
    manager.write_holding_register(1, 700, 0x1111).unwrap();
    manager.write_holding_register(2, 700, 0x2222).unwrap();

    // Broadcast write (should have no effect when disabled)
    manager.broadcast_write_holding_register(700, 0xFFFF).unwrap();

    // Values should remain unchanged
    let v1 = manager.read_holding_registers(1, 700, 1).unwrap();
    let v2 = manager.read_holding_registers(2, 700, 1).unwrap();
    assert_eq!(v1[0], 0x1111);
    assert_eq!(v2[0], 0x2222);
}

// =============================================================================
// Data Type Conversion Tests
// =============================================================================

#[test]
fn test_data_type_string_conversion() {
    let conv = RegisterConverter::default();

    let original = "Hello, World!";
    let registers = conv.string_to_registers(original, 8); // 16 chars max

    let result = conv.registers_to_string_trimmed(&registers);
    assert_eq!(result, original);
}

#[test]
fn test_data_type_typed_value() {
    let conv = RegisterConverter::default();

    // Test all basic types
    let test_cases: Vec<(TypedValue<'static>, RegisterDataType)> = vec![
        (TypedValue::Bool(true), RegisterDataType::Bool),
        (TypedValue::Int16(-1234), RegisterDataType::Int16),
        (TypedValue::UInt16(65535), RegisterDataType::UInt16),
        (TypedValue::Int32(-123456), RegisterDataType::Int32),
        (TypedValue::UInt32(4000000000), RegisterDataType::UInt32),
        (TypedValue::Float32(123.456), RegisterDataType::Float32),
    ];

    for (value, data_type) in test_cases {
        let registers = conv.value_to_registers(data_type, &value);
        let result = conv.registers_to_value(data_type, &registers);

        // Compare (special handling for float)
        match (&value, &result) {
            (TypedValue::Float32(a), TypedValue::Float32(b)) => {
                assert!((*a - *b).abs() < 0.001, "Float32 mismatch");
            }
            _ => {
                // For non-float types, we need to compare differently
                // since the lifetime might differ
                let v1_f64 = value.as_f64();
                let v2_f64 = result.as_f64();
                assert_eq!(v1_f64, v2_f64, "Type {:?} mismatch", data_type);
            }
        }
    }
}

// =============================================================================
// Statistics Tests
// =============================================================================

#[test]
fn test_unit_statistics() {
    let manager = MultiUnitManager::with_defaults();

    manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();
    manager.add_unit(2, UnitConfig::new("Unit 2")).unwrap();

    // Perform operations
    for _ in 0..10 {
        manager.write_holding_register(1, 0, 100).unwrap();
    }
    for _ in 0..5 {
        manager.read_holding_registers(1, 0, 1).unwrap();
    }
    for _ in 0..3 {
        manager.write_holding_register(2, 0, 200).unwrap();
    }

    let stats = manager.unit_statistics();
    assert_eq!(stats.len(), 2);

    let unit1_stats = stats.iter().find(|s| s.unit_id == 1).unwrap();
    let unit2_stats = stats.iter().find(|s| s.unit_id == 2).unwrap();

    assert_eq!(unit1_stats.write_count, 10);
    assert_eq!(unit1_stats.read_count, 5);
    assert_eq!(unit2_stats.write_count, 3);
    assert_eq!(unit2_stats.read_count, 0);

    // Test total requests
    assert_eq!(manager.total_requests(), 18); // 10 + 5 + 3
}

#[test]
fn test_statistics_reset() {
    let manager = MultiUnitManager::with_defaults();
    manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();

    // Perform operations
    manager.write_holding_register(1, 0, 100).unwrap();
    manager.read_holding_registers(1, 0, 1).unwrap();

    assert!(manager.total_requests() > 0);

    // Reset
    manager.reset_statistics();

    assert_eq!(manager.total_requests(), 0);
    let stats = manager.unit_statistics();
    assert_eq!(stats[0].read_count, 0);
    assert_eq!(stats[0].write_count, 0);
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn test_error_unit_not_found() {
    let manager = MultiUnitManager::with_defaults();
    manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();

    let result = manager.read_holding_registers(99, 0, 1);
    assert!(result.is_err());
}

#[test]
fn test_error_unit_zero_add() {
    let manager = MultiUnitManager::with_defaults();

    let result = manager.add_unit(0, UnitConfig::new("Broadcast"));
    assert!(result.is_err());
}

#[test]
fn test_error_duplicate_unit() {
    let manager = MultiUnitManager::with_defaults();
    manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();

    let result = manager.add_unit(1, UnitConfig::new("Unit 1 Again"));
    assert!(result.is_err());
}

#[test]
fn test_error_disabled_unit() {
    let manager = MultiUnitManager::with_defaults();
    manager
        .add_unit(1, UnitConfig::new("Unit 1").with_enabled(false))
        .unwrap();

    let result = manager.read_holding_registers(1, 0, 1);
    assert!(result.is_err());
}

// =============================================================================
// Auto-Create Tests
// =============================================================================

#[test]
fn test_auto_create_enabled() {
    let config = UnitManagerConfig::default().with_auto_create(true);
    let manager = MultiUnitManager::new(config);

    // Unit doesn't exist yet
    assert!(!manager.has_unit(10));

    // Access auto-creates it
    let unit = manager.get_unit(10);
    assert!(unit.is_some());
    assert!(manager.has_unit(10));
}

#[test]
fn test_auto_create_disabled() {
    let config = UnitManagerConfig::default().with_auto_create(false);
    let manager = MultiUnitManager::new(config);

    // Unit doesn't exist
    assert!(!manager.has_unit(10));

    // Access does NOT create it
    let unit = manager.get_unit(10);
    assert!(unit.is_none());
    assert!(!manager.has_unit(10));
}
