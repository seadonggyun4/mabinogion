//! BACnet alarm and event service handlers.
//!
//! Per ASHRAE 135, Clause 13 (Alarm and Event Services):
//!
//! - **AcknowledgeAlarm** (Confirmed, service 0): Acknowledge alarm transitions.
//! - **GetAlarmSummary** (Confirmed, service 3): Retrieve active alarm list.
//! - **GetEnrollmentSummary** (Confirmed, service 4): Query EventEnrollment objects.
//! - **GetEventInformation** (Confirmed, service 29): Retrieve event state details.
//! - **ConfirmedEventNotification** (Confirmed, service 2): Receive event notifications.

use crate::apdu::encoding::{ApduDecoder, ApduEncoder};
use crate::apdu::types::{ConfirmedService, ErrorClass, ErrorCode};
use crate::object::event_enrollment::{EventEnrollment, EventTransitionBits};
use crate::object::property::{BACnetValue, EventState, PropertyId};
use crate::object::traits::BACnetObject;
use crate::object::types::{ObjectId, ObjectType};

use super::handler::{ConfirmedServiceHandler, ServiceContext, ServiceResult};

// ============================================================================
// AcknowledgeAlarm handler
// ============================================================================

/// Decoded AcknowledgeAlarm request.
#[derive(Debug)]
pub struct AcknowledgeAlarmRequest {
    /// Process identifier at the acknowledging process.
    pub acknowledging_process_id: u32,
    /// The event-generating object.
    pub event_object_id: ObjectId,
    /// The event state being acknowledged.
    pub event_state_acknowledged: EventState,
    /// Timestamp of the event being acknowledged.
    pub time_stamp: u64,
    /// Source of the acknowledgment (character string).
    pub acknowledgment_source: String,
    /// Timestamp of the acknowledgment.
    pub time_of_acknowledgment: u64,
}

impl AcknowledgeAlarmRequest {
    /// Decode from APDU service data.
    pub fn decode(data: &[u8]) -> Result<Self, &'static str> {
        let mut decoder = ApduDecoder::new(data);

        // Context tag [0]: acknowledging process identifier (unsigned)
        let (_, _, len) = decoder.decode_tag_info().map_err(|_| "Failed to decode tag")?;
        let process_id = decoder.decode_unsigned(len).map_err(|_| "Failed to decode process_id")?;

        // Context tag [1]: event object identifier
        let (_, _, len) = decoder.decode_tag_info().map_err(|_| "Failed to decode tag")?;
        if len != 4 {
            return Err("Invalid object identifier length");
        }
        let event_object_id = decoder.decode_object_identifier().map_err(|_| "Failed to decode object_id")?;

        // Context tag [2]: event state acknowledged (enumerated)
        let (_, _, len) = decoder.decode_tag_info().map_err(|_| "Failed to decode tag")?;
        let state_val = decoder.decode_unsigned(len).map_err(|_| "Failed to decode state")?;
        let event_state = event_state_from_u32(state_val);

        // Context tag [3]: timestamp (unsigned for simplicity)
        let (_, _, len) = decoder.decode_tag_info().map_err(|_| "Failed to decode tag")?;
        let time_stamp = decoder.decode_unsigned(len).map_err(|_| "Failed to decode timestamp")? as u64;

        // Context tag [4]: acknowledgment source (character string)
        let (_, _, len) = decoder.decode_tag_info().map_err(|_| "Failed to decode tag")?;
        let ack_source = decoder.decode_character_string(len).map_err(|_| "Failed to decode source")?;

        // Context tag [5]: time of acknowledgment (unsigned)
        let time_of_ack = if !decoder.is_empty() {
            let (_, _, len) = decoder.decode_tag_info().map_err(|_| "Failed to decode tag")?;
            decoder.decode_unsigned(len).map_err(|_| "Failed to decode ack_time")? as u64
        } else {
            0
        };

        Ok(Self {
            acknowledging_process_id: process_id,
            event_object_id,
            event_state_acknowledged: event_state,
            time_stamp,
            acknowledgment_source: ack_source,
            time_of_acknowledgment: time_of_ack,
        })
    }
}

/// AcknowledgeAlarm service handler.
pub struct AcknowledgeAlarmHandler;

impl AcknowledgeAlarmHandler {
    pub fn new() -> Self {
        Self
    }
}

impl ConfirmedServiceHandler for AcknowledgeAlarmHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::AcknowledgeAlarm
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        let request = match AcknowledgeAlarmRequest::decode(data) {
            Ok(r) => r,
            Err(_) => return ServiceResult::invalid_tag(),
        };

        // Find the EventEnrollment or alarm-capable object
        let obj = match ctx.objects.get(&request.event_object_id) {
            Some(o) => o,
            None => return ServiceResult::unknown_object(),
        };

        // Try downcast to EventEnrollment
        if let Some(ee) = obj.as_any().downcast_ref::<EventEnrollment>() {
            let transition_name = match request.event_state_acknowledged {
                EventState::Normal => "to-normal",
                EventState::Fault => "to-fault",
                _ => "to-offnormal",
            };
            match ee.acknowledge(transition_name) {
                Ok(()) => ServiceResult::SimpleAck,
                Err(_) => ServiceResult::service_request_denied(),
            }
        } else {
            // Object exists but is not an EventEnrollment
            ServiceResult::Error {
                error_class: ErrorClass::Object,
                error_code: ErrorCode::UnsupportedObjectType,
            }
        }
    }

    fn name(&self) -> &'static str {
        "AcknowledgeAlarm"
    }

    fn min_data_length(&self) -> usize {
        8
    }
}

// ============================================================================
// GetAlarmSummary handler
// ============================================================================

/// Entry in the GetAlarmSummary response.
#[derive(Debug, Clone)]
pub struct AlarmSummaryEntry {
    pub object_id: ObjectId,
    pub alarm_state: EventState,
    pub acked_transitions: EventTransitionBits,
}

/// GetAlarmSummary service handler.
///
/// Returns a list of all objects currently in alarm state.
pub struct GetAlarmSummaryHandler;

impl GetAlarmSummaryHandler {
    pub fn new() -> Self {
        Self
    }
}

impl ConfirmedServiceHandler for GetAlarmSummaryHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::GetAlarmSummary
    }

    fn handle(&self, _data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        let mut encoder = ApduEncoder::new();
        let mut count = 0u32;

        // Iterate through all objects and find those in alarm state
        for obj in ctx.objects.iter() {
            let flags = obj.status_flags();
            if flags.in_alarm {
                let obj_id = obj.object_identifier();

                // Get event state
                let event_state = obj
                    .read_property(PropertyId::EventState)
                    .ok()
                    .and_then(|v| v.as_unsigned())
                    .map(event_state_from_u32)
                    .unwrap_or(EventState::Offnormal);

                // Get acked transitions
                let acked = obj
                    .read_property(PropertyId::AckedTransitions)
                    .ok()
                    .and_then(|v| {
                        if let BACnetValue::BitString(bits) = v {
                            Some(EventTransitionBits::from_bits(&bits))
                        } else {
                            None
                        }
                    })
                    .unwrap_or(EventTransitionBits::all());

                // Encode: ObjectIdentifier, AlarmState (enumerated), AckedTransitions (bitstring)
                encoder.encode_context_object_identifier(0, obj_id);
                encoder.encode_context_enumerated(1, event_state as u32);
                encoder.encode_context_bit_string(2, &acked.to_bits());

                count += 1;
            }
        }

        if count == 0 {
            // Empty response is valid for ComplexAck
            ServiceResult::ComplexAck(encoder.into_bytes())
        } else {
            ServiceResult::ComplexAck(encoder.into_bytes())
        }
    }

    fn name(&self) -> &'static str {
        "GetAlarmSummary"
    }
}

// ============================================================================
// GetEnrollmentSummary handler
// ============================================================================

/// GetEnrollmentSummary acknowledgment filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AcknowledgmentFilter {
    All = 0,
    Acked = 1,
    NotAcked = 2,
}

/// GetEnrollmentSummary service handler.
///
/// Returns a filtered list of EventEnrollment objects.
pub struct GetEnrollmentSummaryHandler;

impl GetEnrollmentSummaryHandler {
    pub fn new() -> Self {
        Self
    }
}

impl ConfirmedServiceHandler for GetEnrollmentSummaryHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::GetEnrollmentSummary
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        // Decode minimal filter from request
        let ack_filter = if !data.is_empty() {
            let mut decoder = ApduDecoder::new(data);
            if let Ok((_, _, len)) = decoder.decode_tag_info() {
                let val = decoder.decode_unsigned(len).unwrap_or(0);
                match val {
                    1 => AcknowledgmentFilter::Acked,
                    2 => AcknowledgmentFilter::NotAcked,
                    _ => AcknowledgmentFilter::All,
                }
            } else {
                AcknowledgmentFilter::All
            }
        } else {
            AcknowledgmentFilter::All
        };

        let mut encoder = ApduEncoder::new();

        // Iterate through EventEnrollment objects
        for obj in ctx.objects.iter() {
            if obj.object_identifier().object_type != ObjectType::EventEnrollment {
                continue;
            }

            if let Some(ee) = obj.as_any().downcast_ref::<EventEnrollment>() {
                let acked = ee.acked_transitions();
                let all_acked = acked.to_offnormal && acked.to_fault && acked.to_normal;
                let any_unacked = !acked.to_offnormal || !acked.to_fault || !acked.to_normal;

                let include = match ack_filter {
                    AcknowledgmentFilter::All => true,
                    AcknowledgmentFilter::Acked => all_acked,
                    AcknowledgmentFilter::NotAcked => any_unacked,
                };

                if include {
                    let obj_id = ee.object_identifier();
                    let event_type = ee.event_type();
                    let event_state = ee.event_state();
                    let nc = ee.notification_class();

                    // Encode summary entry:
                    // ObjectIdentifier, EventType, EventState, Priority, NotificationClass
                    encoder.encode_context_object_identifier(0, obj_id);
                    encoder.encode_context_enumerated(1, event_type as u32);
                    encoder.encode_context_enumerated(2, event_state as u32);
                    encoder.encode_context_unsigned(3, 0); // priority placeholder
                    encoder.encode_context_unsigned(4, nc);
                }
            }
        }

        ServiceResult::ComplexAck(encoder.into_bytes())
    }

    fn name(&self) -> &'static str {
        "GetEnrollmentSummary"
    }
}

// ============================================================================
// GetEventInformation handler
// ============================================================================

/// GetEventInformation service handler.
///
/// Returns detailed event state information for objects in non-normal states.
pub struct GetEventInformationHandler;

impl GetEventInformationHandler {
    pub fn new() -> Self {
        Self
    }
}

impl ConfirmedServiceHandler for GetEventInformationHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::GetEventInformation
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        // Optional: lastReceivedObjectIdentifier (context tag 0)
        let last_obj = if !data.is_empty() {
            let mut decoder = ApduDecoder::new(data);
            if let Ok((_, _, len)) = decoder.decode_tag_info() {
                if len == 4 {
                    decoder.decode_object_identifier().ok()
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let mut encoder = ApduEncoder::new();

        // Opening tag [0] for listOfEventSummaries
        encoder.encode_opening_tag(0);

        let mut found_last = last_obj.is_none();
        let mut count = 0u32;

        for obj in ctx.objects.iter() {
            let obj_id = obj.object_identifier();

            // Skip until we pass the last received object
            if !found_last {
                if Some(obj_id) == last_obj {
                    found_last = true;
                }
                continue;
            }

            // Only include objects NOT in normal state
            let event_state = obj
                .read_property(PropertyId::EventState)
                .ok()
                .and_then(|v| v.as_unsigned())
                .map(event_state_from_u32)
                .unwrap_or(EventState::Normal);

            if event_state == EventState::Normal {
                continue;
            }

            // Encode event summary
            encoder.encode_context_object_identifier(0, obj_id);
            encoder.encode_context_enumerated(1, event_state as u32);

            // AckedTransitions
            let acked = obj
                .read_property(PropertyId::AckedTransitions)
                .ok()
                .and_then(|v| {
                    if let BACnetValue::BitString(bits) = v {
                        Some(bits)
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| vec![true, true, true]);
            encoder.encode_context_bit_string(2, &acked);

            // EventTimeStamps (simplified)
            encoder.encode_opening_tag(3);
            if let Ok(BACnetValue::Array(ts)) =
                obj.read_property(PropertyId::EventTimeStamps)
            {
                for t in &ts {
                    if let Some(v) = t.as_unsigned() {
                        encoder.encode_unsigned(v);
                    } else if let BACnetValue::Unsigned64(v) = t {
                        encoder.encode_unsigned(*v as u32);
                    } else {
                        encoder.encode_unsigned(0);
                    }
                }
            }
            encoder.encode_closing_tag(3);

            // NotifyType
            let notify_type = obj
                .read_property(PropertyId::NotifyType)
                .ok()
                .and_then(|v| v.as_unsigned())
                .unwrap_or(0);
            encoder.encode_context_enumerated(4, notify_type);

            // EventEnable
            let event_enable = obj
                .read_property(PropertyId::EventEnable)
                .ok()
                .and_then(|v| {
                    if let BACnetValue::BitString(bits) = v {
                        Some(bits)
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| vec![true, true, true]);
            encoder.encode_context_bit_string(5, &event_enable);

            count += 1;
            // Limit to reasonable batch size
            if count >= 20 {
                break;
            }
        }

        // Closing tag [0]
        encoder.encode_closing_tag(0);

        // moreEvents [1] BOOLEAN
        let more_events = count >= 20;
        encoder.encode_context_unsigned(1, if more_events { 1 } else { 0 });

        ServiceResult::ComplexAck(encoder.into_bytes())
    }

    fn name(&self) -> &'static str {
        "GetEventInformation"
    }
}

// ============================================================================
// ConfirmedEventNotification handler
// ============================================================================

/// ConfirmedEventNotification service handler.
///
/// Receives event notifications from other devices (for forwarding/logging).
/// In a simulator, this primarily acknowledges receipt.
pub struct ConfirmedEventNotificationHandler;

impl ConfirmedEventNotificationHandler {
    pub fn new() -> Self {
        Self
    }
}

impl ConfirmedServiceHandler for ConfirmedEventNotificationHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::ConfirmedEventNotification
    }

    fn handle(&self, _data: &[u8], _ctx: &ServiceContext) -> ServiceResult {
        // A BACnet simulator receiving confirmed event notifications
        // should acknowledge receipt with a SimpleAck.
        // (Real implementations would log/forward the notification.)
        ServiceResult::SimpleAck
    }

    fn name(&self) -> &'static str {
        "ConfirmedEventNotification"
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn event_state_from_u32(v: u32) -> EventState {
    match v {
        0 => EventState::Normal,
        1 => EventState::Fault,
        2 => EventState::Offnormal,
        3 => EventState::HighLimit,
        4 => EventState::LowLimit,
        5 => EventState::LifeSafetyAlarm,
        _ => EventState::Offnormal,
    }
}

/// Encode an EventNotification as an unconfirmed APDU.
///
/// Per ASHRAE 135, Clause 13.8.
pub fn encode_event_notification(
    notification: &crate::object::event_enrollment::EventNotification,
) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();

    // [0] Process Identifier
    encoder.encode_context_unsigned(0, notification.process_id);
    // [1] Initiating Device
    encoder.encode_context_object_identifier(1, notification.initiating_device);
    // [2] Event Object
    encoder.encode_context_object_identifier(2, notification.event_object);
    // [3] Time Stamp (unsigned for simplicity)
    encoder.encode_context_unsigned(3, notification.time_stamp as u32);
    // [4] Notification Class
    encoder.encode_context_unsigned(4, notification.notification_class);
    // [5] Priority
    encoder.encode_context_unsigned(5, notification.priority as u32);
    // [6] Event Type
    encoder.encode_context_enumerated(6, notification.event_type as u32);
    // [7] Message Text (optional)
    if let Some(ref text) = notification.message_text {
        encoder.encode_context_character_string(7, text);
    }
    // [8] Notify Type
    encoder.encode_context_enumerated(8, notification.notify_type as u32);
    // [9] Ack Required
    encoder.encode_context_unsigned(9, if notification.ack_required { 1 } else { 0 });
    // [10] From State
    encoder.encode_context_enumerated(10, notification.from_state as u32);
    // [11] To State
    encoder.encode_context_enumerated(11, notification.to_state as u32);

    encoder.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use crate::object::event_enrollment::{EventEnrollment, EventType};
    use crate::object::registry::ObjectRegistry;

    fn make_ctx(registry: &Arc<ObjectRegistry>) -> ServiceContext {
        ServiceContext {
            objects: registry.clone(),
            device_instance: 1234,
            invoke_id: Some(1),
            max_apdu_length: 1476,
            source_address: None,
        }
    }

    #[test]
    fn test_handler_creation() {
        assert_eq!(
            AcknowledgeAlarmHandler::new().service_choice(),
            ConfirmedService::AcknowledgeAlarm
        );
        assert_eq!(
            GetAlarmSummaryHandler::new().service_choice(),
            ConfirmedService::GetAlarmSummary
        );
        assert_eq!(
            GetEnrollmentSummaryHandler::new().service_choice(),
            ConfirmedService::GetEnrollmentSummary
        );
        assert_eq!(
            GetEventInformationHandler::new().service_choice(),
            ConfirmedService::GetEventInformation
        );
        assert_eq!(
            ConfirmedEventNotificationHandler::new().service_choice(),
            ConfirmedService::ConfirmedEventNotification
        );
    }

    #[test]
    fn test_get_alarm_summary_empty() {
        let registry = Arc::new(ObjectRegistry::new());
        let ctx = make_ctx(&registry);

        let handler = GetAlarmSummaryHandler::new();
        match handler.handle(&[], &ctx) {
            ServiceResult::ComplexAck(data) => {
                assert!(data.is_empty()); // No alarms
            }
            other => panic!("Expected ComplexAck, got {:?}", other),
        }
    }

    #[test]
    fn test_get_alarm_summary_with_alarm() {
        let registry = Arc::new(ObjectRegistry::new());

        let ee = EventEnrollment::new(1, "TempAlarm", EventType::OutOfRange)
            .with_high_limit(80.0)
            .with_low_limit(20.0);
        ee.transition_to(EventState::HighLimit, 1000);
        registry.register(Arc::new(ee));

        let ctx = make_ctx(&registry);

        let handler = GetAlarmSummaryHandler::new();
        match handler.handle(&[], &ctx) {
            ServiceResult::ComplexAck(data) => {
                assert!(!data.is_empty());
            }
            other => panic!("Expected ComplexAck, got {:?}", other),
        }
    }

    #[test]
    fn test_get_enrollment_summary() {
        let registry = Arc::new(ObjectRegistry::new());

        let ee1 = EventEnrollment::new(1, "Alarm1", EventType::OutOfRange);
        let ee2 = EventEnrollment::new(2, "Alarm2", EventType::ChangeOfState);
        registry.register(Arc::new(ee1));
        registry.register(Arc::new(ee2));

        let ctx = make_ctx(&registry);

        let handler = GetEnrollmentSummaryHandler::new();
        match handler.handle(&[], &ctx) {
            ServiceResult::ComplexAck(data) => {
                assert!(!data.is_empty());
            }
            other => panic!("Expected ComplexAck, got {:?}", other),
        }
    }

    #[test]
    fn test_get_event_information_no_alarms() {
        let registry = Arc::new(ObjectRegistry::new());

        // Add a normal-state enrollment
        let ee = EventEnrollment::new(1, "NormalAlarm", EventType::OutOfRange);
        registry.register(Arc::new(ee));

        let ctx = make_ctx(&registry);

        let handler = GetEventInformationHandler::new();
        match handler.handle(&[], &ctx) {
            ServiceResult::ComplexAck(data) => {
                // Should only have opening/closing tags + moreEvents
                assert!(!data.is_empty());
            }
            other => panic!("Expected ComplexAck, got {:?}", other),
        }
    }

    #[test]
    fn test_get_event_information_with_alarm() {
        let registry = Arc::new(ObjectRegistry::new());

        let ee = EventEnrollment::new(1, "ActiveAlarm", EventType::OutOfRange);
        ee.transition_to(EventState::HighLimit, 5000);
        registry.register(Arc::new(ee));

        let ctx = make_ctx(&registry);

        let handler = GetEventInformationHandler::new();
        match handler.handle(&[], &ctx) {
            ServiceResult::ComplexAck(data) => {
                assert!(!data.is_empty());
            }
            other => panic!("Expected ComplexAck, got {:?}", other),
        }
    }

    #[test]
    fn test_confirmed_event_notification_ack() {
        let registry = Arc::new(ObjectRegistry::new());
        let ctx = make_ctx(&registry);

        let handler = ConfirmedEventNotificationHandler::new();
        match handler.handle(&[], &ctx) {
            ServiceResult::SimpleAck => {} // Expected
            other => panic!("Expected SimpleAck, got {:?}", other),
        }
    }

    #[test]
    fn test_encode_event_notification() {
        use crate::object::event_enrollment::{EventNotification, NotifyType};

        let notification = EventNotification {
            destination: "192.168.1.100:47808".parse().unwrap(),
            process_id: 1,
            initiating_device: ObjectId::new(ObjectType::Device, 1234),
            event_object: ObjectId::new(ObjectType::EventEnrollment, 1),
            time_stamp: 5000,
            notification_class: 1,
            priority: 3,
            event_type: EventType::OutOfRange,
            message_text: Some("Temperature too high".to_string()),
            notify_type: NotifyType::Alarm,
            ack_required: true,
            from_state: EventState::Normal,
            to_state: EventState::HighLimit,
            confirmed: false,
        };

        let encoded = encode_event_notification(&notification);
        assert!(!encoded.is_empty());
    }

    #[test]
    fn test_event_state_from_u32() {
        assert_eq!(event_state_from_u32(0), EventState::Normal);
        assert_eq!(event_state_from_u32(1), EventState::Fault);
        assert_eq!(event_state_from_u32(2), EventState::Offnormal);
        assert_eq!(event_state_from_u32(3), EventState::HighLimit);
        assert_eq!(event_state_from_u32(4), EventState::LowLimit);
        assert_eq!(event_state_from_u32(5), EventState::LifeSafetyAlarm);
        assert_eq!(event_state_from_u32(99), EventState::Offnormal); // fallback
    }
}
