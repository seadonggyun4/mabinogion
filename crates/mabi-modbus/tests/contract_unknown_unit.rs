mod support;

use support::{
    assert_exception_pdu, build_device, run_rtu_request, run_tcp_request, DeviceSpec,
    TransportOutcome,
};

use mabi_modbus::context::BroadcastPolicy;
use mabi_modbus::handler::ExceptionCode;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_unit_behavior_stays_transport_specific() {
    let tcp_outcome = run_tcp_request(
        vec![build_device(DeviceSpec::dense(1))],
        BroadcastPolicy::WriteAll,
        99,
        vec![0x03, 0x00, 0x00, 0x00, 0x01],
    )
    .await;

    match tcp_outcome {
        TransportOutcome::Response(pdu) => {
            assert_exception_pdu(
                &pdu,
                0x03,
                ExceptionCode::GatewayTargetDeviceFailedToRespond,
            );
        }
        TransportOutcome::NoResponse => {
            panic!("TCP unknown-unit requests must return an exception")
        }
    }

    let rtu_outcome = run_rtu_request(
        vec![build_device(DeviceSpec::dense(1))],
        BroadcastPolicy::WriteAll,
        vec![1],
        99,
        vec![0x03, 0x00, 0x00, 0x00, 0x01],
    )
    .await;

    assert_eq!(rtu_outcome, TransportOutcome::NoResponse);
}
