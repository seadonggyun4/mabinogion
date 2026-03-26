mod support;

use support::{assert_exception_pdu, build_device, run_direct_request, DeviceSpec};

use mabi_modbus::context::BroadcastPolicy;
use mabi_modbus::handler::ExceptionCode;

#[test]
fn validation_contract_covers_quantity_address_byte_count_and_unknown_function() {
    let device = build_device(DeviceSpec::dense(1));
    let unit_context = vec![device.context().clone()];

    let max_quantity = run_direct_request(
        unit_context.clone(),
        BroadcastPolicy::WriteAll,
        1,
        vec![0x03, 0x00, 0x00, 0x00, 0x7D],
    )
    .unwrap();
    assert_eq!(max_quantity[0], 0x03);
    assert_eq!(max_quantity[1], 250);

    let zero_quantity = run_direct_request(
        unit_context.clone(),
        BroadcastPolicy::WriteAll,
        1,
        vec![0x03, 0x00, 0x00, 0x00, 0x00],
    )
    .unwrap_err();
    assert_eq!(zero_quantity, ExceptionCode::IllegalDataValue);

    let overflow_quantity = run_direct_request(
        unit_context.clone(),
        BroadcastPolicy::WriteAll,
        1,
        vec![0x03, 0x00, 0x00, 0x00, 0x7E],
    )
    .unwrap_err();
    assert_eq!(overflow_quantity, ExceptionCode::IllegalDataValue);

    let bad_byte_count = run_direct_request(
        unit_context.clone(),
        BroadcastPolicy::WriteAll,
        1,
        vec![0x10, 0x00, 0x00, 0x00, 0x02, 0x03, 0xAA, 0xAA, 0xBB],
    )
    .unwrap_err();
    assert_eq!(bad_byte_count, ExceptionCode::IllegalDataValue);

    let bad_address = run_direct_request(
        unit_context.clone(),
        BroadcastPolicy::WriteAll,
        1,
        vec![0x03, 0x00, 0xFF, 0x00, 0x02],
    )
    .unwrap_err();
    assert_eq!(bad_address, ExceptionCode::IllegalDataAddress);

    let unsupported = run_direct_request(
        unit_context,
        BroadcastPolicy::WriteAll,
        1,
        vec![0x42, 0x00, 0x00, 0x00, 0x01],
    )
    .unwrap_err();
    assert_eq!(unsupported, ExceptionCode::IllegalFunction);

    assert_exception_pdu(&[0x83, 0x03], 0x03, ExceptionCode::IllegalDataValue);
}
