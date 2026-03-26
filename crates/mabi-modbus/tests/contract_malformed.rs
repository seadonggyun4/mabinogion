mod support;

use support::{assert_exception_pdu, build_device, run_tcp_request, DeviceSpec, TransportOutcome};

use bytes::BytesMut;
use mabi_modbus::context::BroadcastPolicy;
use mabi_modbus::handler::ExceptionCode;
use mabi_modbus::rtu::RtuFrame;
use mabi_modbus::tcp::{MbapCodec, MbapHeader};
use tokio_util::codec::Decoder;

#[test]
fn malformed_mbap_headers_are_rejected_by_the_codec() {
    let invalid_protocol = MbapHeader::parse(&[0x00, 0x01, 0x00, 0x01, 0x00, 0x06, 0x01])
        .unwrap_err()
        .to_string();
    assert!(invalid_protocol.contains("Invalid protocol ID"));

    let invalid_length = MbapHeader::parse(&[0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01])
        .unwrap_err()
        .to_string();
    assert!(invalid_length.contains("Invalid MBAP length"));

    let mut codec = MbapCodec::new();
    let mut short_buffer = BytesMut::from(&b"\x00\x01\x00\x00\x00\x06"[..]);
    assert!(codec.decode(&mut short_buffer).unwrap().is_none());
}

#[test]
fn malformed_rtu_frames_are_rejected() {
    let valid = RtuFrame::new(1, vec![0x03, 0x00, 0x00, 0x00, 0x01]).encode();
    let mut corrupted = valid.to_vec();
    let last = corrupted.len() - 1;
    corrupted[last] ^= 0xFF;

    assert!(RtuFrame::decode(&corrupted).is_err());
    assert!(RtuFrame::decode(&[0x01, 0x03, 0x00]).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn short_tcp_pdu_is_reported_as_an_exception_response() {
    let outcome = run_tcp_request(
        vec![build_device(DeviceSpec::dense(1))],
        BroadcastPolicy::WriteAll,
        1,
        vec![0x03],
    )
    .await;

    match outcome {
        TransportOutcome::Response(pdu) => {
            assert_exception_pdu(&pdu, 0x03, ExceptionCode::IllegalDataValue);
        }
        TransportOutcome::NoResponse => panic!("short unicast TCP PDU should produce an exception"),
    }
}
