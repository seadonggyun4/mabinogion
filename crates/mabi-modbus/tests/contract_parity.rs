mod support;

use support::{
    assert_exception_pdu, build_device, run_direct_request, run_rtu_request, run_tcp_request,
    DeviceSpec, TransportOutcome,
};

use mabi_modbus::context::BroadcastPolicy;
use mabi_modbus::handler::ExceptionCode;

#[test]
fn dense_and_sparse_backends_return_the_same_values_and_exceptions() {
    let dense = build_device(DeviceSpec::dense(1));
    let sparse = build_device(DeviceSpec::sparse(1));

    dense
        .address_space()
        .write_holding_register(4, 0x0A0A)
        .unwrap();
    dense
        .address_space()
        .write_holding_register(5, 0x0B0B)
        .unwrap();
    sparse
        .address_space()
        .write_holding_register(4, 0x0A0A)
        .unwrap();
    sparse
        .address_space()
        .write_holding_register(5, 0x0B0B)
        .unwrap();

    let request = vec![0x03, 0x00, 0x04, 0x00, 0x02];
    let dense_response = run_direct_request(
        vec![dense.context().clone()],
        BroadcastPolicy::WriteAll,
        1,
        request.clone(),
    )
    .unwrap();
    let sparse_response = run_direct_request(
        vec![sparse.context().clone()],
        BroadcastPolicy::WriteAll,
        1,
        request,
    )
    .unwrap();
    assert_eq!(dense_response, sparse_response);

    let dense_error = run_direct_request(
        vec![dense.context().clone()],
        BroadcastPolicy::WriteAll,
        1,
        vec![0x03, 0x00, 0xFF, 0x00, 0x02],
    )
    .unwrap_err();
    let sparse_error = run_direct_request(
        vec![sparse.context().clone()],
        BroadcastPolicy::WriteAll,
        1,
        vec![0x03, 0x00, 0xFF, 0x00, 0x02],
    )
    .unwrap_err();
    assert_eq!(dense_error, ExceptionCode::IllegalDataAddress);
    assert_eq!(dense_error, sparse_error);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_and_rtu_match_on_non_broadcast_success_and_exception_paths() {
    let tcp_device = build_device(DeviceSpec::dense(1));
    tcp_device
        .address_space()
        .write_holding_register(7, 0xABCD)
        .unwrap();
    let tcp_success = run_tcp_request(
        vec![tcp_device],
        BroadcastPolicy::WriteAll,
        1,
        vec![0x03, 0x00, 0x07, 0x00, 0x01],
    )
    .await;

    let rtu_device = build_device(DeviceSpec::dense(1));
    rtu_device
        .address_space()
        .write_holding_register(7, 0xABCD)
        .unwrap();
    let rtu_success = run_rtu_request(
        vec![rtu_device],
        BroadcastPolicy::WriteAll,
        vec![1],
        1,
        vec![0x03, 0x00, 0x07, 0x00, 0x01],
    )
    .await;

    assert_eq!(
        tcp_success,
        TransportOutcome::Response(vec![0x03, 0x02, 0xAB, 0xCD])
    );
    assert_eq!(tcp_success, rtu_success);

    let tcp_exception = run_tcp_request(
        vec![build_device(DeviceSpec::dense(1))],
        BroadcastPolicy::WriteAll,
        1,
        vec![0x10, 0x00, 0x00, 0x00, 0x02, 0x03, 0xAA, 0xAA, 0xBB],
    )
    .await;
    let rtu_exception = run_rtu_request(
        vec![build_device(DeviceSpec::dense(1))],
        BroadcastPolicy::WriteAll,
        vec![1],
        1,
        vec![0x10, 0x00, 0x00, 0x00, 0x02, 0x03, 0xAA, 0xAA, 0xBB],
    )
    .await;

    match tcp_exception {
        TransportOutcome::Response(pdu) => {
            assert_exception_pdu(&pdu, 0x10, ExceptionCode::IllegalDataValue);
        }
        TransportOutcome::NoResponse => panic!("TCP should return an exception for invalid FC10"),
    }
    match rtu_exception {
        TransportOutcome::Response(pdu) => {
            assert_exception_pdu(&pdu, 0x10, ExceptionCode::IllegalDataValue);
        }
        TransportOutcome::NoResponse => panic!("RTU should return an exception for invalid FC10"),
    }
}
