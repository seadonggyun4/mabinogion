mod support;

use support::{
    build_device, run_direct_request, run_rtu_request, run_tcp_request, DeviceSpec,
    TransportOutcome,
};

use mabi_modbus::context::BroadcastPolicy;
use mabi_modbus::handler::ExceptionCode;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn broadcast_write_fans_out_without_any_wire_response() {
    let tcp_device_1 = build_device(DeviceSpec::dense(1));
    let tcp_device_2 = build_device(DeviceSpec::dense(2).with_broadcast(false));
    let tcp_space_1 = tcp_device_1.address_space();
    let tcp_space_2 = tcp_device_2.address_space();

    let tcp_outcome = run_tcp_request(
        vec![tcp_device_1, tcp_device_2],
        BroadcastPolicy::WriteAll,
        0,
        vec![0x06, 0x00, 0x05, 0x12, 0x34],
    )
    .await;
    assert_eq!(tcp_outcome, TransportOutcome::NoResponse);
    assert_eq!(
        tcp_space_1.read_holding_registers(5, 1).unwrap(),
        vec![0x1234]
    );
    assert_eq!(
        tcp_space_2.read_holding_registers(5, 1).unwrap(),
        vec![0x0000]
    );

    let rtu_device_1 = build_device(DeviceSpec::dense(1));
    let rtu_device_2 = build_device(DeviceSpec::dense(2));
    let rtu_space_1 = rtu_device_1.address_space();
    let rtu_space_2 = rtu_device_2.address_space();

    let rtu_outcome = run_rtu_request(
        vec![rtu_device_1, rtu_device_2],
        BroadcastPolicy::WriteAll,
        vec![1, 2],
        0,
        vec![0x06, 0x00, 0x05, 0x56, 0x78],
    )
    .await;
    assert_eq!(rtu_outcome, TransportOutcome::NoResponse);
    assert_eq!(
        rtu_space_1.read_holding_registers(5, 1).unwrap(),
        vec![0x5678]
    );
    assert_eq!(
        rtu_space_2.read_holding_registers(5, 1).unwrap(),
        vec![0x5678]
    );
}

#[test]
fn broadcast_policy_filters_targets_and_preserves_unit_opt_out() {
    let device_1 = build_device(DeviceSpec::dense(1));
    let device_2 = build_device(DeviceSpec::dense(2).with_broadcast(false));
    let device_3 = build_device(DeviceSpec::dense(3));

    let space_1 = device_1.address_space();
    let space_2 = device_2.address_space();
    let space_3 = device_3.address_space();
    let unit_contexts = vec![
        device_1.context().clone(),
        device_2.context().clone(),
        device_3.context().clone(),
    ];

    let selective = run_direct_request(
        unit_contexts.clone(),
        BroadcastPolicy::SelectiveList(vec![2, 3]),
        0,
        vec![0x06, 0x00, 0x09, 0x33, 0x33],
    )
    .unwrap();
    assert_eq!(selective, vec![0x06, 0x00, 0x09, 0x33, 0x33]);
    assert_eq!(space_1.read_holding_registers(9, 1).unwrap(), vec![0x0000]);
    assert_eq!(space_2.read_holding_registers(9, 1).unwrap(), vec![0x0000]);
    assert_eq!(space_3.read_holding_registers(9, 1).unwrap(), vec![0x3333]);

    let echo = run_direct_request(
        unit_contexts,
        BroadcastPolicy::EchoToUnit(1),
        0,
        vec![0x06, 0x00, 0x0A, 0x44, 0x44],
    )
    .unwrap();
    assert_eq!(echo, vec![0x06, 0x00, 0x0A, 0x44, 0x44]);
    assert_eq!(space_1.read_holding_registers(10, 1).unwrap(), vec![0x4444]);
    assert_eq!(space_3.read_holding_registers(10, 1).unwrap(), vec![0x0000]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn broadcast_read_is_rejected_internally_but_suppressed_on_the_wire() {
    let direct_device = build_device(DeviceSpec::dense(1));
    let direct_space = direct_device.address_space();
    direct_space.write_holding_register(12, 0xCAFE).unwrap();

    let direct_error = run_direct_request(
        vec![direct_device.context().clone()],
        BroadcastPolicy::WriteAll,
        0,
        vec![0x03, 0x00, 0x0C, 0x00, 0x01],
    )
    .unwrap_err();
    assert_eq!(direct_error, ExceptionCode::IllegalFunction);
    assert_eq!(
        direct_space.read_holding_registers(12, 1).unwrap(),
        vec![0xCAFE]
    );

    let tcp_device = build_device(DeviceSpec::dense(1));
    let tcp_space = tcp_device.address_space();
    tcp_space.write_holding_register(12, 0xCAFE).unwrap();

    let tcp_outcome = run_tcp_request(
        vec![tcp_device],
        BroadcastPolicy::WriteAll,
        0,
        vec![0x03, 0x00, 0x0C, 0x00, 0x01],
    )
    .await;
    assert_eq!(tcp_outcome, TransportOutcome::NoResponse);
    assert_eq!(
        tcp_space.read_holding_registers(12, 1).unwrap(),
        vec![0xCAFE]
    );
}
