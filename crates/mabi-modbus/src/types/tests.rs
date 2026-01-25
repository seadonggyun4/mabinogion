//! Integration tests for the types module.

use super::*;

#[test]
fn test_all_word_orders_round_trip() {
    // Test that all word orders properly round-trip for all numeric types
    let converters: Vec<RegisterConverter> = WordOrder::all()
        .iter()
        .map(|&order| RegisterConverter::new(order))
        .collect();

    // Test values
    let i32_values = [0, 1, -1, i32::MIN, i32::MAX, 0x12345678, -12345678];
    let u32_values = [0, 1, u32::MAX, 0x12345678, 0xDEADBEEF];
    let f32_values = [0.0f32, 1.0, -1.0, 123.456, -987.654, f32::MIN, f32::MAX];
    let f64_values = [0.0f64, 1.0, -1.0, 123456.789, -987654.321, f64::MIN, f64::MAX];

    for conv in &converters {
        // i32
        for &v in &i32_values {
            let regs = conv.i32_to_registers(v);
            let result = conv.registers_to_i32(&regs);
            assert_eq!(result, v, "i32 failed for {:?}: {}", conv.word_order(), v);
        }

        // u32
        for &v in &u32_values {
            let regs = conv.u32_to_registers(v);
            let result = conv.registers_to_u32(&regs);
            assert_eq!(result, v, "u32 failed for {:?}: {}", conv.word_order(), v);
        }

        // f32
        for &v in &f32_values {
            let regs = conv.f32_to_registers(v);
            let result = conv.registers_to_f32(&regs);
            assert!(
                (result - v).abs() < f32::EPSILON * v.abs().max(1.0)
                    || (result.is_nan() && v.is_nan()),
                "f32 failed for {:?}: expected {}, got {}",
                conv.word_order(),
                v,
                result
            );
        }

        // f64
        for &v in &f64_values {
            let regs = conv.f64_to_registers(v);
            let result = conv.registers_to_f64(&regs);
            assert!(
                (result - v).abs() < f64::EPSILON * v.abs().max(1.0)
                    || (result.is_nan() && v.is_nan()),
                "f64 failed for {:?}: expected {}, got {}",
                conv.word_order(),
                v,
                result
            );
        }
    }
}

#[test]
fn test_known_byte_patterns() {
    // Verify specific byte patterns for known word orders
    // Value: 0x12345678

    let be = RegisterConverter::big_endian();
    let be_swap = RegisterConverter::big_endian_word_swap();

    let value: i32 = 0x12345678;

    // Big Endian: 0x1234, 0x5678
    let be_regs = be.i32_to_registers(value);
    assert_eq!(be_regs[0], 0x1234);
    assert_eq!(be_regs[1], 0x5678);

    // Big Endian Word Swap: 0x5678, 0x1234
    let bes_regs = be_swap.i32_to_registers(value);
    assert_eq!(bes_regs[0], 0x5678);
    assert_eq!(bes_regs[1], 0x1234);
}

#[test]
fn test_string_edge_cases() {
    let conv = RegisterConverter::default();

    // Empty string
    let empty_regs = conv.string_to_registers("", 4);
    assert_eq!(empty_regs, vec![0, 0, 0, 0]);

    // String exactly fills registers
    let exact = conv.string_to_registers("12345678", 4);
    assert_eq!(exact.len(), 4);
    let result = conv.registers_to_string(&exact);
    assert_eq!(result, "12345678");

    // String shorter than register capacity
    let short = conv.string_to_registers("Hi", 4);
    assert_eq!(short.len(), 4);
    let result = conv.registers_to_string_trimmed(&short);
    assert_eq!(result, "Hi");

    // Unicode string (UTF-8)
    let unicode = conv.string_to_registers("日本", 6);
    let result = conv.registers_to_string_trimmed(&unicode);
    assert_eq!(result, "日本");
}

#[test]
fn test_typed_value_from_impls() {
    let _: TypedValue = true.into();
    let _: TypedValue = 42i16.into();
    let _: TypedValue = 42u16.into();
    let _: TypedValue = 42i32.into();
    let _: TypedValue = 42u32.into();
    let _: TypedValue = 42.0f32.into();
    let _: TypedValue = 42i64.into();
    let _: TypedValue = 42u64.into();
    let _: TypedValue = 42.0f64.into();
    let _: TypedValue = "hello".into();
    let _: TypedValue = String::from("hello").into();
    let _: TypedValue = vec![1u8, 2, 3].into();
    let _: TypedValue = [1u8, 2, 3].as_slice().into();
}

#[test]
fn test_typed_value_as_f64() {
    assert_eq!(TypedValue::Bool(true).as_f64(), Some(1.0));
    assert_eq!(TypedValue::Bool(false).as_f64(), Some(0.0));
    assert_eq!(TypedValue::Int16(-100).as_f64(), Some(-100.0));
    assert_eq!(TypedValue::UInt32(1000).as_f64(), Some(1000.0));
    assert_eq!(TypedValue::Float32(123.5).as_f64(), Some(123.5));

    // String and Bytes return None
    assert!(TypedValue::String("test".into()).as_f64().is_none());
}

#[test]
fn test_data_type_parse_all() {
    let types = [
        ("bool", RegisterDataType::Bool),
        ("int16", RegisterDataType::Int16),
        ("i16", RegisterDataType::Int16),
        ("short", RegisterDataType::Int16),
        ("uint16", RegisterDataType::UInt16),
        ("u16", RegisterDataType::UInt16),
        ("word", RegisterDataType::UInt16),
        ("int32", RegisterDataType::Int32),
        ("i32", RegisterDataType::Int32),
        ("dint", RegisterDataType::Int32),
        ("uint32", RegisterDataType::UInt32),
        ("dword", RegisterDataType::UInt32),
        ("float32", RegisterDataType::Float32),
        ("real", RegisterDataType::Float32),
        ("float", RegisterDataType::Float32),
        ("int64", RegisterDataType::Int64),
        ("lint", RegisterDataType::Int64),
        ("uint64", RegisterDataType::UInt64),
        ("ulint", RegisterDataType::UInt64),
        ("float64", RegisterDataType::Float64),
        ("double", RegisterDataType::Float64),
        ("lreal", RegisterDataType::Float64),
    ];

    for (input, expected) in types {
        let parsed: RegisterDataType = input.parse().unwrap();
        assert_eq!(parsed, expected, "Failed for input: {}", input);
    }
}

#[test]
fn test_word_order_serde_roundtrip() {
    for order in WordOrder::all() {
        let json = serde_json::to_string(order).unwrap();
        let parsed: WordOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(&parsed, order);
    }
}

#[test]
fn test_data_type_serde_roundtrip() {
    for dt in RegisterDataType::basic_types() {
        let json = serde_json::to_string(dt).unwrap();
        let parsed: RegisterDataType = serde_json::from_str(&json).unwrap();
        assert_eq!(&parsed, dt);
    }
}
