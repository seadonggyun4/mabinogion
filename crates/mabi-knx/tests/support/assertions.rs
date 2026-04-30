use mabi_knx::frame::{Dib, ServiceFamily};
use mabi_knx::tunnel::ConnectStatus;
use mabi_knx::{ConnectResponse, DisconnectResponse, KnxFrame, ServiceType, TunnellingAck};

use super::TestResult;

pub fn assert_service(frame: &KnxFrame, expected: ServiceType) -> TestResult {
    if frame.service_type != expected {
        return Err(format!("expected {:?}, received {:?}", expected, frame.service_type).into());
    }
    Ok(())
}

pub fn assert_dibs_include_device_and_tunnelling(data: &[u8]) -> TestResult {
    let dibs = Dib::decode_all(data)?;
    let mut has_device = false;
    let mut has_tunnelling = false;

    for dib in dibs {
        match dib {
            Dib::DeviceInfo(info) => {
                has_device = true;
                if info.device_name != "Mabi KNX Integration" {
                    return Err(format!("unexpected device name `{}`", info.device_name).into());
                }
            }
            Dib::SupportedServiceFamilies(families) => {
                has_tunnelling = families.supports(ServiceFamily::Tunnelling);
                if !families.supports(ServiceFamily::Core) {
                    return Err("supported service families missing Core".into());
                }
            }
            Dib::Generic { .. } => {}
        }
    }

    if !has_device {
        return Err("DeviceInfo DIB missing".into());
    }
    if !has_tunnelling {
        return Err("Tunnelling service family missing".into());
    }
    Ok(())
}

pub fn decode_successful_connect(frame: &KnxFrame) -> TestResult<ConnectResponse> {
    assert_service(frame, ServiceType::ConnectResponse)?;
    let response = ConnectResponse::decode(&frame.body)?;
    if response.status != ConnectStatus::NoError {
        return Err(format!("connect failed with status {:?}", response.status).into());
    }
    if response.channel_id == 0 {
        return Err("connect response returned channel 0".into());
    }
    if response.crd.is_none() {
        return Err("connect response missing CRD".into());
    }
    Ok(response)
}

pub fn decode_ok_ack(frame: &KnxFrame, channel_id: u8, sequence: u8) -> TestResult<TunnellingAck> {
    assert_service(frame, ServiceType::TunnellingAck)?;
    let ack = TunnellingAck::decode(&frame.body)?;
    if ack.channel_id != channel_id || ack.sequence_counter != sequence || !ack.is_ok() {
        return Err(format!(
            "unexpected ACK channel={} sequence={} status={}",
            ack.channel_id, ack.sequence_counter, ack.status
        )
        .into());
    }
    Ok(ack)
}

pub fn decode_disconnect(frame: &KnxFrame, channel_id: u8) -> TestResult<DisconnectResponse> {
    assert_service(frame, ServiceType::DisconnectResponse)?;
    let response = DisconnectResponse::decode(&frame.body)?;
    if response.channel_id != channel_id || response.status != 0 {
        return Err(format!(
            "unexpected disconnect response channel={} status={}",
            response.channel_id, response.status
        )
        .into());
    }
    Ok(response)
}
