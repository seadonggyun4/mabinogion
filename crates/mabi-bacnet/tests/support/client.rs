use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::time::timeout;

use mabi_bacnet::object::property::{BACnetDate, BACnetTime};
use mabi_bacnet::prelude::{
    ApduDecoder, ApduEncoder, ApduType, BACnetValue, BvlcMessage, ConfirmedService, Npdu, ObjectId,
    ObjectType, PropertyId, UnconfirmedService,
};

#[derive(Debug)]
pub enum ApduFrame {
    UnconfirmedRequest {
        service_choice: u8,
        data: Vec<u8>,
    },
    SimpleAck {
        invoke_id: u8,
        service_choice: u8,
    },
    ComplexAck {
        invoke_id: u8,
        service_choice: u8,
        data: Vec<u8>,
    },
    SegmentedComplexAck {
        invoke_id: u8,
        sequence_number: u8,
        window_size: u8,
        more_follows: bool,
        service_choice: u8,
        data: Vec<u8>,
    },
    Error {
        invoke_id: u8,
        service_choice: u8,
        error_class: u32,
        error_code: u32,
    },
    Reject {
        invoke_id: u8,
        reason: u8,
    },
    Abort {
        invoke_id: u8,
        reason: u8,
    },
    SegmentAck {
        invoke_id: u8,
        sequence_number: u8,
        window_size: u8,
    },
}

#[derive(Debug)]
pub struct ReceivedPacket {
    pub source: SocketAddr,
    pub bvlc: BvlcMessage,
    pub npdu: Option<Npdu>,
    pub apdu: Option<ApduFrame>,
}

#[derive(Debug)]
pub struct SegmentedResponse {
    pub invoke_id: u8,
    pub service_choice: u8,
    pub segment_count: usize,
    pub service_data: Vec<u8>,
}

pub struct LoopbackClient {
    socket: UdpSocket,
}

impl LoopbackClient {
    pub async fn bind() -> Self {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("loopback client socket should bind");
        Self { socket }
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.socket
            .local_addr()
            .expect("loopback client should have a local address")
    }

    pub async fn send_bvlc_message(
        &self,
        dest: SocketAddr,
        message: BvlcMessage,
    ) -> std::io::Result<()> {
        self.socket
            .send_to(&message.encode(), dest)
            .await
            .map(|_| ())
    }

    pub async fn send_confirmed_request(
        &self,
        dest: SocketAddr,
        service_choice: ConfirmedService,
        invoke_id: u8,
        service_data: Vec<u8>,
        segmented_response_accepted: bool,
    ) -> std::io::Result<()> {
        let mut apdu = Vec::with_capacity(4 + service_data.len());
        let mut pdu_type = 0x00;
        if segmented_response_accepted {
            pdu_type |= 0x02;
        }
        apdu.push(pdu_type);
        apdu.push(0x05);
        apdu.push(invoke_id);
        apdu.push(service_choice as u8);
        apdu.extend_from_slice(&service_data);

        let npdu = Npdu::simple(apdu);
        let bvlc = BvlcMessage::original_unicast(npdu.encode());
        self.send_bvlc_message(dest, bvlc).await
    }

    pub async fn send_unconfirmed_request(
        &self,
        dest: SocketAddr,
        service_choice: UnconfirmedService,
        service_data: Vec<u8>,
    ) -> std::io::Result<()> {
        let mut apdu = Vec::with_capacity(2 + service_data.len());
        apdu.push(0x10);
        apdu.push(service_choice as u8);
        apdu.extend_from_slice(&service_data);

        let npdu = Npdu::no_reply(apdu);
        let bvlc = BvlcMessage::original_unicast(npdu.encode());
        self.send_bvlc_message(dest, bvlc).await
    }

    pub async fn recv_packet(&self, max_wait: Duration) -> ReceivedPacket {
        let mut buffer = vec![0u8; 4096];
        let (size, source) = timeout(max_wait, self.socket.recv_from(&mut buffer))
            .await
            .expect("receive should complete before timeout")
            .expect("receive should succeed");
        decode_packet(source, &buffer[..size])
    }

    pub async fn expect_no_packet(&self, max_wait: Duration) {
        let mut buffer = vec![0u8; 1024];
        let result = timeout(max_wait, self.socket.recv_from(&mut buffer)).await;
        assert!(result.is_err(), "unexpected packet received");
    }

    pub async fn collect_segmented_response(
        &self,
        first: ReceivedPacket,
        max_wait: Duration,
    ) -> SegmentedResponse {
        let mut segment_count = 0usize;
        let mut service_data = Vec::new();
        let mut invoke_id = 0u8;
        let mut service_choice = 0u8;
        let mut next_packet = Some(first);

        loop {
            let packet = next_packet
                .take()
                .expect("segmented packet should be present");
            match packet.apdu {
                Some(ApduFrame::SegmentedComplexAck {
                    invoke_id: packet_invoke_id,
                    service_choice: packet_service_choice,
                    data,
                    more_follows,
                    ..
                }) => {
                    if segment_count == 0 {
                        invoke_id = packet_invoke_id;
                        service_choice = packet_service_choice;
                    }
                    segment_count += 1;
                    service_data.extend_from_slice(&data);
                    if !more_follows {
                        break;
                    }
                    next_packet = Some(self.recv_packet(max_wait).await);
                }
                other => panic!("expected segmented complex ack, got {other:?}"),
            }
        }

        SegmentedResponse {
            invoke_id,
            service_choice,
            segment_count,
            service_data,
        }
    }
}

pub fn encode_who_is_all() -> Vec<u8> {
    Vec::new()
}

pub fn encode_read_property_request(object_id: ObjectId, property_id: PropertyId) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_context_object_identifier(0, object_id);
    encoder.encode_context_enumerated(1, property_id as u32);
    encoder.into_bytes()
}

pub fn encode_read_property_request_with_array_index(
    object_id: ObjectId,
    property_id: PropertyId,
    array_index: u32,
) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_context_object_identifier(0, object_id);
    encoder.encode_context_enumerated(1, property_id as u32);
    encoder.encode_context_unsigned(2, array_index);
    encoder.into_bytes()
}

pub fn encode_write_property_request(
    object_id: ObjectId,
    property_id: PropertyId,
    value: &BACnetValue,
) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_context_object_identifier(0, object_id);
    encoder.encode_context_enumerated(1, property_id as u32);
    encoder.encode_opening_tag(3);
    encoder.encode_value(value);
    encoder.encode_closing_tag(3);
    encoder.into_bytes()
}

pub fn encode_read_property_multiple_request(
    object_specs: &[(ObjectId, Vec<PropertyId>)],
) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    for (object_id, property_ids) in object_specs {
        encoder.encode_context_object_identifier(0, *object_id);
        encoder.encode_opening_tag(1);
        for property_id in property_ids {
            encoder.encode_context_enumerated(0, *property_id as u32);
        }
        encoder.encode_closing_tag(1);
    }
    encoder.into_bytes()
}

pub fn encode_write_property_multiple_request(
    object_id: ObjectId,
    writes: &[(PropertyId, BACnetValue)],
) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_context_object_identifier(0, object_id);
    encoder.encode_opening_tag(1);
    for (property_id, value) in writes {
        encoder.encode_context_enumerated(0, *property_id as u32);
        encoder.encode_opening_tag(2);
        encoder.encode_value(value);
        encoder.encode_closing_tag(2);
    }
    encoder.encode_closing_tag(1);
    encoder.into_bytes()
}

pub fn encode_subscribe_cov_request(
    process_id: u32,
    object_id: ObjectId,
    confirmed: Option<bool>,
    lifetime_secs: Option<u32>,
) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_context_unsigned(0, process_id);
    encoder.encode_context_object_identifier(1, object_id);
    if let Some(confirmed) = confirmed {
        encoder.encode_context_unsigned(2, if confirmed { 1 } else { 0 });
    }
    if let Some(lifetime_secs) = lifetime_secs {
        encoder.encode_context_unsigned(3, lifetime_secs);
    }
    encoder.into_bytes()
}

pub fn encode_create_object_request(object_type: ObjectType) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_opening_tag(0);
    encoder.encode_context_enumerated(0, object_type as u32);
    encoder.encode_closing_tag(0);
    encoder.into_bytes()
}

pub fn encode_delete_object_request(object_id: ObjectId) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_object_identifier(object_id);
    encoder.into_bytes()
}

pub fn encode_dcc_request(enable_disable: u32) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_context_enumerated(1, enable_disable);
    encoder.into_bytes()
}

pub fn encode_reinitialize_request(reinit_state: u32) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_context_enumerated(0, reinit_state);
    encoder.into_bytes()
}

pub fn encode_time_sync_request(date: BACnetDate, time: BACnetTime) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.put_u8((10 << 4) | 4);
    encoder.put_bytes(&[date.year, date.month, date.day, date.day_of_week]);
    encoder.put_u8((11 << 4) | 4);
    encoder.put_bytes(&[time.hour, time.minute, time.second, time.hundredths]);
    encoder.into_bytes()
}

pub fn encode_atomic_write_file_stream_request(instance: u32, start: i32, data: &[u8]) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_object_identifier(ObjectId::new(ObjectType::File, instance));
    encoder.encode_opening_tag(0);
    encoder.encode_signed(start);
    encoder.encode_octet_string(data);
    encoder.encode_closing_tag(0);
    encoder.into_bytes()
}

pub fn encode_atomic_read_file_stream_request(instance: u32, start: i32, count: u32) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_object_identifier(ObjectId::new(ObjectType::File, instance));
    encoder.encode_opening_tag(0);
    encoder.encode_signed(start);
    encoder.encode_unsigned(count);
    encoder.encode_closing_tag(0);
    encoder.into_bytes()
}

pub fn encode_read_range_by_position_request(
    object_id: ObjectId,
    property_id: PropertyId,
    reference_index: u32,
    count: i32,
) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_context_object_identifier(0, object_id);
    encoder.encode_context_enumerated(1, property_id as u32);
    encoder.encode_opening_tag(3);
    encoder.encode_unsigned(reference_index);
    encoder.encode_signed(count);
    encoder.encode_closing_tag(3);
    encoder.into_bytes()
}

fn decode_packet(source: SocketAddr, bytes: &[u8]) -> ReceivedPacket {
    let bvlc = BvlcMessage::decode(bytes).expect("BVLC packet should decode");
    let npdu = if bvlc.header.function.has_npdu() {
        Some(Npdu::decode(&bvlc.npdu).expect("NPDU should decode"))
    } else {
        None
    };
    let apdu = npdu
        .as_ref()
        .map(|npdu| parse_apdu(npdu.apdu()))
        .transpose()
        .expect("APDU should decode");

    ReceivedPacket {
        source,
        bvlc,
        npdu,
        apdu,
    }
}

fn parse_apdu(bytes: &[u8]) -> Result<ApduFrame, String> {
    if bytes.is_empty() {
        return Err("APDU is empty".into());
    }

    let apdu_type = ApduType::from_nibble((bytes[0] >> 4) & 0x0F)
        .ok_or_else(|| "unknown APDU type".to_string())?;

    match apdu_type {
        ApduType::UnconfirmedRequest => Ok(ApduFrame::UnconfirmedRequest {
            service_choice: *bytes
                .get(1)
                .ok_or_else(|| "missing unconfirmed service".to_string())?,
            data: bytes.get(2..).unwrap_or_default().to_vec(),
        }),
        ApduType::SimpleAck => Ok(ApduFrame::SimpleAck {
            invoke_id: *bytes
                .get(1)
                .ok_or_else(|| "missing simple ack invoke id".to_string())?,
            service_choice: *bytes
                .get(2)
                .ok_or_else(|| "missing simple ack service".to_string())?,
        }),
        ApduType::ComplexAck => {
            if (bytes[0] & 0x08) != 0 {
                Ok(ApduFrame::SegmentedComplexAck {
                    invoke_id: *bytes
                        .get(1)
                        .ok_or_else(|| "missing segmented invoke id".to_string())?,
                    sequence_number: *bytes
                        .get(2)
                        .ok_or_else(|| "missing sequence number".to_string())?,
                    window_size: *bytes
                        .get(3)
                        .ok_or_else(|| "missing window size".to_string())?,
                    service_choice: *bytes
                        .get(4)
                        .ok_or_else(|| "missing segmented service choice".to_string())?,
                    more_follows: (bytes[0] & 0x04) != 0,
                    data: bytes.get(5..).unwrap_or_default().to_vec(),
                })
            } else {
                Ok(ApduFrame::ComplexAck {
                    invoke_id: *bytes
                        .get(1)
                        .ok_or_else(|| "missing complex ack invoke id".to_string())?,
                    service_choice: *bytes
                        .get(2)
                        .ok_or_else(|| "missing complex ack service".to_string())?,
                    data: bytes.get(3..).unwrap_or_default().to_vec(),
                })
            }
        }
        ApduType::Error => {
            let mut decoder = ApduDecoder::new(bytes.get(3..).unwrap_or_default());
            let error_class = decode_enumerated(&mut decoder)?;
            let error_code = decode_enumerated(&mut decoder)?;
            Ok(ApduFrame::Error {
                invoke_id: *bytes
                    .get(1)
                    .ok_or_else(|| "missing error invoke id".to_string())?,
                service_choice: *bytes
                    .get(2)
                    .ok_or_else(|| "missing error service".to_string())?,
                error_class,
                error_code,
            })
        }
        ApduType::Reject => Ok(ApduFrame::Reject {
            invoke_id: *bytes
                .get(1)
                .ok_or_else(|| "missing reject invoke id".to_string())?,
            reason: *bytes
                .get(2)
                .ok_or_else(|| "missing reject reason".to_string())?,
        }),
        ApduType::Abort => Ok(ApduFrame::Abort {
            invoke_id: *bytes
                .get(1)
                .ok_or_else(|| "missing abort invoke id".to_string())?,
            reason: *bytes
                .get(2)
                .ok_or_else(|| "missing abort reason".to_string())?,
        }),
        ApduType::SegmentAck => Ok(ApduFrame::SegmentAck {
            invoke_id: *bytes
                .get(1)
                .ok_or_else(|| "missing segment ack invoke id".to_string())?,
            sequence_number: *bytes
                .get(2)
                .ok_or_else(|| "missing segment ack sequence".to_string())?,
            window_size: *bytes
                .get(3)
                .ok_or_else(|| "missing segment ack window".to_string())?,
        }),
        ApduType::ConfirmedRequest => {
            Err("confirmed request parsing is not used in loopback client".into())
        }
    }
}

fn decode_enumerated(decoder: &mut ApduDecoder<'_>) -> Result<u32, String> {
    let (tag, is_context, len) = decoder
        .decode_tag_info()
        .map_err(|error| error.to_string())?;
    if is_context || tag != 9 {
        return Err("expected application enumerated".into());
    }
    decoder
        .decode_unsigned(len)
        .map_err(|error| error.to_string())
}
