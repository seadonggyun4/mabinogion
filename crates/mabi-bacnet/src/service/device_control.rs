//! BACnet device management service handlers.
//!
//! Per ASHRAE 135:
//!
//! - **TimeSynchronization** (Unconfirmed, service 6): Synchronize device clock.
//! - **UTCTimeSynchronization** (Unconfirmed, service 9): Synchronize to UTC time.
//! - **DeviceCommunicationControl** (Confirmed, service 17): Enable/disable device communication.
//! - **ReinitializeDevice** (Confirmed, service 20): Reset or restart the device.
//!
//! ## TimeSynchronization
//!
//! The handler decodes a BACnet Date+Time from the request and computes the
//! difference from the host system clock. This offset is stored on the DeviceObject
//! so that subsequent LocalDate/LocalTime reads reflect the synchronized time.
//!
//! ## DeviceCommunicationControl
//!
//! Supports three states: Enable, Disable, DisableInitiation.
//! An optional password can be required. The handler updates the DeviceObject's
//! communication_control field.
//!
//! ## ReinitializeDevice
//!
//! Supports Coldstart and Warmstart reinitialisation states. The handler resets
//! the device system status and records the last_restore_time.

use crate::apdu::encoding::ApduDecoder;
use crate::apdu::types::{ConfirmedService, ErrorClass, ErrorCode, UnconfirmedService};
use crate::object::device::{CommunicationControlState, DeviceObject};
use crate::object::property::{BACnetDate, BACnetTime};
use crate::object::types::{ObjectId, ObjectType};

use super::handler::{
    ConfirmedServiceHandler, ServiceContext, ServiceResult, UnconfirmedServiceHandler,
};

// ── TimeSynchronization ─────────────────────────────────────────────────────

/// Decoded time synchronization request.
#[derive(Debug, Clone)]
pub struct TimeSyncRequest {
    pub date: BACnetDate,
    pub time: BACnetTime,
}

/// Decode a TimeSynchronization request.
///
/// Format: Date (application tag 10, 4 bytes) + Time (application tag 11, 4 bytes)
pub fn decode_time_sync_request(data: &[u8]) -> Result<TimeSyncRequest, &'static str> {
    if data.len() < 10 {
        return Err("TimeSynchronization data too short");
    }

    let mut decoder = ApduDecoder::new(data);

    // Date: application tag 10, length 4
    let (tag, _is_context, _len) = decoder
        .decode_tag_info()
        .map_err(|_| "Failed to decode date tag")?;
    if tag != 10 {
        return Err("Expected Date application tag (10)");
    }
    let date_bytes = decoder
        .read_bytes(4)
        .map_err(|_| "Failed to read date bytes")?;
    let date = BACnetDate {
        year: date_bytes[0], // year - 1900
        month: date_bytes[1],
        day: date_bytes[2],
        day_of_week: date_bytes[3],
    };

    // Time: application tag 11, length 4
    let (tag, _is_context, _len) = decoder
        .decode_tag_info()
        .map_err(|_| "Failed to decode time tag")?;
    if tag != 11 {
        return Err("Expected Time application tag (11)");
    }
    let time_bytes = decoder
        .read_bytes(4)
        .map_err(|_| "Failed to read time bytes")?;
    let time = BACnetTime {
        hour: time_bytes[0],
        minute: time_bytes[1],
        second: time_bytes[2],
        hundredths: time_bytes[3],
    };

    Ok(TimeSyncRequest { date, time })
}

/// Convert a BACnet Date + Time to epoch seconds (approximate).
fn bacnet_datetime_to_epoch(date: &BACnetDate, time: &BACnetTime) -> i64 {
    let year = date.year as i64 + 1900;
    let month = date.month as i64;
    let day = date.day as i64;

    // Approximate days from epoch using standard formula
    let y = if month <= 2 { year - 1 } else { year };
    let m = if month <= 2 { month + 9 } else { month - 3 };
    let days = 365 * y + y / 4 - y / 100 + y / 400 + (m * 306 + 5) / 10 + day - 1 - 719468;

    let secs =
        days * 86400 + time.hour as i64 * 3600 + time.minute as i64 * 60 + time.second as i64;

    secs
}

/// Handler for BACnet TimeSynchronization (Unconfirmed Service 6).
///
/// Updates the DeviceObject's time offset so that subsequent LocalDate/LocalTime
/// reads reflect the synchronized time.
pub struct TimeSynchronizationHandler;

impl TimeSynchronizationHandler {
    pub fn new() -> Self {
        Self
    }
}

impl UnconfirmedServiceHandler for TimeSynchronizationHandler {
    fn service_choice(&self) -> UnconfirmedService {
        UnconfirmedService::TimeSynchronization
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        let request = match decode_time_sync_request(data) {
            Ok(r) => r,
            Err(_) => return ServiceResult::NoResponse,
        };

        // Compute the target epoch and current epoch
        let target_epoch = bacnet_datetime_to_epoch(&request.date, &request.time);
        let now = std::time::SystemTime::now();
        let current_epoch = now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let offset = target_epoch - current_epoch;

        // Find the Device object and apply the time offset
        let device_id = ObjectId::new(ObjectType::Device, ctx.device_instance);
        if let Some(obj) = ctx.objects.get(&device_id) {
            if let Some(device) = obj.as_any().downcast_ref::<DeviceObject>() {
                device.set_time_offset_secs(offset);
            }
        }

        ServiceResult::NoResponse
    }

    fn name(&self) -> &'static str {
        "TimeSynchronization"
    }
}

// ── UTCTimeSynchronization ──────────────────────────────────────────────────

/// Handler for BACnet UTCTimeSynchronization (Unconfirmed Service 9).
///
/// Same as TimeSynchronization but the provided time is in UTC.
pub struct UtcTimeSynchronizationHandler;

impl UtcTimeSynchronizationHandler {
    pub fn new() -> Self {
        Self
    }
}

impl UnconfirmedServiceHandler for UtcTimeSynchronizationHandler {
    fn service_choice(&self) -> UnconfirmedService {
        UnconfirmedService::UtcTimeSynchronization
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        // Same wire format as TimeSynchronization
        let request = match decode_time_sync_request(data) {
            Ok(r) => r,
            Err(_) => return ServiceResult::NoResponse,
        };

        let target_epoch = bacnet_datetime_to_epoch(&request.date, &request.time);
        let current_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let offset = target_epoch - current_epoch;

        let device_id = ObjectId::new(ObjectType::Device, ctx.device_instance);
        if let Some(obj) = ctx.objects.get(&device_id) {
            if let Some(device) = obj.as_any().downcast_ref::<DeviceObject>() {
                device.set_time_offset_secs(offset);
            }
        }

        ServiceResult::NoResponse
    }

    fn name(&self) -> &'static str {
        "UTCTimeSynchronization"
    }
}

// ── DeviceCommunicationControl ──────────────────────────────────────────────

/// Decoded DeviceCommunicationControl request.
#[derive(Debug, Clone)]
pub struct DeviceCommunicationControlRequest {
    /// Duration in minutes (0 = indefinite, None = indefinite).
    pub time_duration: Option<u16>,
    /// Enable/Disable state.
    pub enable_disable: EnableDisable,
    /// Optional password.
    pub password: Option<String>,
}

/// Enable/Disable enumeration per ASHRAE 135, 16.4.1.1.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum EnableDisable {
    Enable = 0,
    Disable = 1,
    DisableInitiation = 2,
}

impl EnableDisable {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Enable),
            1 => Some(Self::Disable),
            2 => Some(Self::DisableInitiation),
            _ => None,
        }
    }
}

/// Decode a DeviceCommunicationControl request.
///
/// Format:
/// [0] timeDuration (OPTIONAL, unsigned, minutes)
/// [1] enable-disable (enumerated)
/// [2] password (OPTIONAL, character-string)
pub fn decode_dcc_request(data: &[u8]) -> Result<DeviceCommunicationControlRequest, &'static str> {
    let mut decoder = ApduDecoder::new(data);
    let mut time_duration = None;
    let mut enable_disable = None;
    let mut password = None;

    while !decoder.is_empty() {
        let (tag_num, is_context, len) = decoder
            .decode_tag_info()
            .map_err(|_| "Failed to decode tag")?;

        if !is_context {
            return Err("Expected context-tagged fields");
        }

        match tag_num {
            0 => {
                // timeDuration
                let val = decoder
                    .decode_unsigned(len)
                    .map_err(|_| "Failed to decode duration")?;
                time_duration = Some(val as u16);
            }
            1 => {
                // enable-disable
                let val = decoder
                    .decode_unsigned(len)
                    .map_err(|_| "Failed to decode enable-disable")?;
                enable_disable =
                    Some(EnableDisable::from_u32(val).ok_or("Invalid enable-disable value")?);
            }
            2 => {
                // password
                let s = decoder
                    .decode_character_string(len)
                    .map_err(|_| "Failed to decode password")?;
                password = Some(s);
            }
            _ => {
                // Skip unknown tags
                let _ = decoder.read_bytes(len);
            }
        }
    }

    Ok(DeviceCommunicationControlRequest {
        time_duration,
        enable_disable: enable_disable.ok_or("Missing enable-disable field")?,
        password,
    })
}

/// Handler for BACnet DeviceCommunicationControl (Confirmed Service 17).
///
/// Changes the device's communication state. Optionally validates a password.
pub struct DeviceCommunicationControlHandler {
    password: Option<String>,
}

impl DeviceCommunicationControlHandler {
    /// Create a handler that does not require a password.
    pub fn new() -> Self {
        Self { password: None }
    }

    /// Create a handler that requires the given password.
    pub fn with_password(password: String) -> Self {
        Self {
            password: Some(password),
        }
    }
}

impl ConfirmedServiceHandler for DeviceCommunicationControlHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::DeviceCommunicationControl
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        let request = match decode_dcc_request(data) {
            Ok(r) => r,
            Err(_) => return ServiceResult::missing_required_parameter(),
        };

        // Password validation
        if let Some(ref required_pwd) = self.password {
            match &request.password {
                Some(provided) if provided == required_pwd => {}
                _ => {
                    return ServiceResult::Error {
                        error_class: ErrorClass::Security,
                        error_code: ErrorCode::PasswordFailure,
                    };
                }
            }
        }

        // Map to CommunicationControlState
        let state = match request.enable_disable {
            EnableDisable::Enable => CommunicationControlState::Enabled,
            EnableDisable::Disable => CommunicationControlState::Disabled,
            EnableDisable::DisableInitiation => CommunicationControlState::DisabledInitiation,
        };

        // Apply to device
        let device_id = ObjectId::new(ObjectType::Device, ctx.device_instance);
        if let Some(obj) = ctx.objects.get(&device_id) {
            if let Some(device) = obj.as_any().downcast_ref::<DeviceObject>() {
                device.set_communication_control(state);
            }
        }

        ServiceResult::SimpleAck
    }

    fn name(&self) -> &'static str {
        "DeviceCommunicationControl"
    }

    fn min_data_length(&self) -> usize {
        2 // At minimum: context tag + enable-disable value
    }
}

// ── ReinitializeDevice ──────────────────────────────────────────────────────

/// Reinitialisation state per ASHRAE 135, 16.5.1.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ReinitializedState {
    Coldstart = 0,
    Warmstart = 1,
    StartBackup = 2,
    EndBackup = 3,
    StartRestore = 4,
    EndRestore = 5,
    AbortRestore = 6,
    ActivateChanges = 7,
}

impl ReinitializedState {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Coldstart),
            1 => Some(Self::Warmstart),
            2 => Some(Self::StartBackup),
            3 => Some(Self::EndBackup),
            4 => Some(Self::StartRestore),
            5 => Some(Self::EndRestore),
            6 => Some(Self::AbortRestore),
            7 => Some(Self::ActivateChanges),
            _ => None,
        }
    }
}

/// Decoded ReinitializeDevice request.
#[derive(Debug, Clone)]
pub struct ReinitializeDeviceRequest {
    pub reinitialized_state: ReinitializedState,
    pub password: Option<String>,
}

/// Decode a ReinitializeDevice request.
///
/// Format:
/// [0] reinitializedStateOfDevice (enumerated)
/// [1] password (OPTIONAL, character-string)
pub fn decode_reinitialize_request(data: &[u8]) -> Result<ReinitializeDeviceRequest, &'static str> {
    let mut decoder = ApduDecoder::new(data);
    let mut state = None;
    let mut password = None;

    while !decoder.is_empty() {
        let (tag_num, is_context, len) = decoder
            .decode_tag_info()
            .map_err(|_| "Failed to decode tag")?;

        if !is_context {
            return Err("Expected context-tagged fields");
        }

        match tag_num {
            0 => {
                let val = decoder
                    .decode_unsigned(len)
                    .map_err(|_| "Failed to decode state")?;
                state =
                    Some(ReinitializedState::from_u32(val).ok_or("Invalid reinitialize state")?);
            }
            1 => {
                let s = decoder
                    .decode_character_string(len)
                    .map_err(|_| "Failed to decode password")?;
                password = Some(s);
            }
            _ => {
                let _ = decoder.read_bytes(len);
            }
        }
    }

    Ok(ReinitializeDeviceRequest {
        reinitialized_state: state.ok_or("Missing reinitialized state")?,
        password,
    })
}

/// Encode a ReinitializeDevice-ACK (simple ack — no data).
/// Per ASHRAE 135, the response is just a SimpleAck.

/// Handler for BACnet ReinitializeDevice (Confirmed Service 20).
///
/// Processes Coldstart/Warmstart requests and updates device status.
/// For a simulator, this resets the device system status to Operational.
pub struct ReinitializeDeviceHandler {
    password: Option<String>,
}

impl ReinitializeDeviceHandler {
    pub fn new() -> Self {
        Self { password: None }
    }

    pub fn with_password(password: String) -> Self {
        Self {
            password: Some(password),
        }
    }
}

impl ConfirmedServiceHandler for ReinitializeDeviceHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::ReinitializeDevice
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        let request = match decode_reinitialize_request(data) {
            Ok(r) => r,
            Err(_) => return ServiceResult::missing_required_parameter(),
        };

        // Password validation
        if let Some(ref required_pwd) = self.password {
            match &request.password {
                Some(provided) if provided == required_pwd => {}
                _ => {
                    return ServiceResult::Error {
                        error_class: ErrorClass::Security,
                        error_code: ErrorCode::PasswordFailure,
                    };
                }
            }
        }

        // Only support Coldstart and Warmstart for the simulator
        match request.reinitialized_state {
            ReinitializedState::Coldstart | ReinitializedState::Warmstart => {
                // Reset device to operational state
                let device_id = ObjectId::new(ObjectType::Device, ctx.device_instance);
                if let Some(obj) = ctx.objects.get(&device_id) {
                    if let Some(device) = obj.as_any().downcast_ref::<DeviceObject>() {
                        device.set_communication_control(CommunicationControlState::Enabled);
                        device.set_time_offset_secs(0);
                    }
                }
                ServiceResult::SimpleAck
            }
            _ => {
                // Backup/restore not supported in simulator
                ServiceResult::Error {
                    error_class: ErrorClass::Services,
                    error_code: ErrorCode::ServiceRequestDenied,
                }
            }
        }
    }

    fn name(&self) -> &'static str {
        "ReinitializeDevice"
    }

    fn min_data_length(&self) -> usize {
        2
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apdu::encoding::ApduEncoder;
    use crate::object::device::DeviceObjectConfig;
    use crate::object::property::SegmentationSupport;
    use crate::object::registry::ObjectRegistry;
    use std::sync::Arc;

    fn setup_device_registry() -> (Arc<ObjectRegistry>, u32) {
        let registry = Arc::new(ObjectRegistry::new());
        let config = DeviceObjectConfig {
            device_instance: 1234,
            device_name: "TestDevice".to_string(),
            vendor_name: "Test".to_string(),
            vendor_id: 999,
            model_name: "Sim".to_string(),
            firmware_revision: "1.0".to_string(),
            application_software_version: "1.0".to_string(),
            max_apdu_length: 1476,
            segmentation_supported: SegmentationSupport::Both,
            apdu_timeout: 3000,
            number_of_apdu_retries: 3,
            description: String::new(),
            location: String::new(),
        };
        let device = DeviceObject::new(config, registry.clone());
        registry.register(Arc::new(device));
        (registry, 1234)
    }

    fn make_ctx(registry: &Arc<ObjectRegistry>, device_instance: u32) -> ServiceContext {
        ServiceContext {
            objects: registry.clone(),
            device_instance,
            invoke_id: Some(1),
            max_apdu_length: 1476,
            source_address: None,
        }
    }

    #[test]
    fn test_handler_creation() {
        let ts = TimeSynchronizationHandler::new();
        assert_eq!(ts.service_choice(), UnconfirmedService::TimeSynchronization);
        assert_eq!(ts.name(), "TimeSynchronization");

        let utc = UtcTimeSynchronizationHandler::new();
        assert_eq!(
            utc.service_choice(),
            UnconfirmedService::UtcTimeSynchronization
        );
        assert_eq!(utc.name(), "UTCTimeSynchronization");

        let dcc = DeviceCommunicationControlHandler::new();
        assert_eq!(
            dcc.service_choice(),
            ConfirmedService::DeviceCommunicationControl
        );
        assert_eq!(dcc.name(), "DeviceCommunicationControl");

        let reinit = ReinitializeDeviceHandler::new();
        assert_eq!(
            reinit.service_choice(),
            ConfirmedService::ReinitializeDevice
        );
        assert_eq!(reinit.name(), "ReinitializeDevice");
    }

    #[test]
    fn test_decode_time_sync() {
        // Date: 2024-01-15, Monday (year-1900=124, month=1, day=15, dow=1)
        // Time: 10:30:00.00
        let data = [
            0xA4, // application tag 10, length 4
            124, 1, 15, 1,    // date
            0xB4, // application tag 11, length 4
            10, 30, 0, 0, // time
        ];

        let request = decode_time_sync_request(&data).unwrap();
        assert_eq!(request.date.year, 124);
        assert_eq!(request.date.month, 1);
        assert_eq!(request.date.day, 15);
        assert_eq!(request.time.hour, 10);
        assert_eq!(request.time.minute, 30);
    }

    #[test]
    fn test_time_sync_handler() {
        let handler = TimeSynchronizationHandler::new();
        let (registry, dev_inst) = setup_device_registry();
        let ctx = make_ctx(&registry, dev_inst);

        // Send a time sync for a known time
        let data = [
            0xA4, 124, 1, 15, 1, // Date: 2024-01-15
            0xB4, 10, 30, 0, 0, // Time: 10:30:00
        ];

        let result = handler.handle(&data, &ctx);
        assert!(matches!(result, ServiceResult::NoResponse));

        // Verify offset was set (non-zero since system time differs from 2024-01-15 10:30)
        let device_id = ObjectId::new(ObjectType::Device, dev_inst);
        let obj = registry.get(&device_id).unwrap();
        let device = obj.as_any().downcast_ref::<DeviceObject>().unwrap();
        // Offset should be non-zero (negative, since 2024-01-15 is in the past)
        assert_ne!(device.time_offset_secs(), 0);
    }

    #[test]
    fn test_decode_dcc_enable() {
        // [1] enable-disable = Enable (0)
        let data = [
            0x19, 0x00, // context tag [1], len=1, value=0 (Enable)
        ];

        let request = decode_dcc_request(&data).unwrap();
        assert_eq!(request.enable_disable, EnableDisable::Enable);
        assert!(request.time_duration.is_none());
        assert!(request.password.is_none());
    }

    #[test]
    fn test_decode_dcc_disable_with_duration() {
        // [0] timeDuration = 60 minutes
        // [1] enable-disable = Disable (1)
        let data = [
            0x09, 60, // context tag [0], len=1, value=60
            0x19, 0x01, // context tag [1], len=1, value=1 (Disable)
        ];

        let request = decode_dcc_request(&data).unwrap();
        assert_eq!(request.time_duration, Some(60));
        assert_eq!(request.enable_disable, EnableDisable::Disable);
    }

    #[test]
    fn test_dcc_handler_enable() {
        let handler = DeviceCommunicationControlHandler::new();
        let (registry, dev_inst) = setup_device_registry();
        let ctx = make_ctx(&registry, dev_inst);

        // First disable
        let disable_data = [0x19, 0x01]; // Disable
        let result = handler.handle(&disable_data, &ctx);
        assert!(matches!(result, ServiceResult::SimpleAck));

        let device_id = ObjectId::new(ObjectType::Device, dev_inst);
        let obj = registry.get(&device_id).unwrap();
        let device = obj.as_any().downcast_ref::<DeviceObject>().unwrap();
        assert_eq!(
            device.communication_control(),
            CommunicationControlState::Disabled
        );

        // Then enable
        let enable_data = [0x19, 0x00]; // Enable
        let result = handler.handle(&enable_data, &ctx);
        assert!(matches!(result, ServiceResult::SimpleAck));
        assert_eq!(
            device.communication_control(),
            CommunicationControlState::Enabled
        );
    }

    #[test]
    fn test_dcc_handler_with_password() {
        let handler = DeviceCommunicationControlHandler::with_password("secret123".to_string());
        let (registry, dev_inst) = setup_device_registry();
        let ctx = make_ctx(&registry, dev_inst);

        // No password — should fail
        let data = [0x19, 0x01]; // Disable, no password
        let result = handler.handle(&data, &ctx);
        assert!(matches!(
            result,
            ServiceResult::Error {
                error_code: ErrorCode::PasswordFailure,
                ..
            }
        ));

        // Wrong password — encode context tag [1] enable=Disable, then context tag [2] password
        let mut encoder = ApduEncoder::new();
        encoder.encode_context_enumerated(1, 1); // Disable
        encoder.encode_context_character_string(2, "wrong");
        let data = encoder.into_bytes();
        let result = handler.handle(&data, &ctx);
        assert!(matches!(
            result,
            ServiceResult::Error {
                error_code: ErrorCode::PasswordFailure,
                ..
            }
        ));

        // Correct password
        let mut encoder = ApduEncoder::new();
        encoder.encode_context_enumerated(1, 1); // Disable
        encoder.encode_context_character_string(2, "secret123");
        let data = encoder.into_bytes();
        let result = handler.handle(&data, &ctx);
        assert!(matches!(result, ServiceResult::SimpleAck));
    }

    #[test]
    fn test_dcc_disable_initiation() {
        let handler = DeviceCommunicationControlHandler::new();
        let (registry, dev_inst) = setup_device_registry();
        let ctx = make_ctx(&registry, dev_inst);

        let data = [0x19, 0x02]; // DisableInitiation
        let result = handler.handle(&data, &ctx);
        assert!(matches!(result, ServiceResult::SimpleAck));

        let device_id = ObjectId::new(ObjectType::Device, dev_inst);
        let obj = registry.get(&device_id).unwrap();
        let device = obj.as_any().downcast_ref::<DeviceObject>().unwrap();
        assert_eq!(
            device.communication_control(),
            CommunicationControlState::DisabledInitiation
        );
    }

    #[test]
    fn test_decode_reinitialize_coldstart() {
        let data = [0x09, 0x00]; // context tag [0], value=0 (Coldstart)
        let request = decode_reinitialize_request(&data).unwrap();
        assert_eq!(request.reinitialized_state, ReinitializedState::Coldstart);
        assert!(request.password.is_none());
    }

    #[test]
    fn test_decode_reinitialize_warmstart_with_password() {
        let mut encoder = ApduEncoder::new();
        encoder.encode_context_enumerated(0, 1); // Warmstart
        encoder.encode_context_character_string(1, "admin");
        let data = encoder.into_bytes();

        let request = decode_reinitialize_request(&data).unwrap();
        assert_eq!(request.reinitialized_state, ReinitializedState::Warmstart);
        assert_eq!(request.password.as_deref(), Some("admin"));
    }

    #[test]
    fn test_reinitialize_handler_coldstart() {
        let handler = ReinitializeDeviceHandler::new();
        let (registry, dev_inst) = setup_device_registry();
        let ctx = make_ctx(&registry, dev_inst);

        // First disable communication
        let device_id = ObjectId::new(ObjectType::Device, dev_inst);
        let obj = registry.get(&device_id).unwrap();
        let device = obj.as_any().downcast_ref::<DeviceObject>().unwrap();
        device.set_communication_control(CommunicationControlState::Disabled);
        device.set_time_offset_secs(3600);

        // Coldstart
        let data = [0x09, 0x00];
        let result = handler.handle(&data, &ctx);
        assert!(matches!(result, ServiceResult::SimpleAck));

        // Verify reset
        assert_eq!(
            device.communication_control(),
            CommunicationControlState::Enabled
        );
        assert_eq!(device.time_offset_secs(), 0);
    }

    #[test]
    fn test_reinitialize_handler_backup_denied() {
        let handler = ReinitializeDeviceHandler::new();
        let (registry, dev_inst) = setup_device_registry();
        let ctx = make_ctx(&registry, dev_inst);

        // StartBackup (not supported)
        let data = [0x09, 0x02];
        let result = handler.handle(&data, &ctx);
        assert!(matches!(
            result,
            ServiceResult::Error {
                error_code: ErrorCode::ServiceRequestDenied,
                ..
            }
        ));
    }

    #[test]
    fn test_reinitialize_handler_password_required() {
        let handler = ReinitializeDeviceHandler::with_password("admin".to_string());
        let (registry, dev_inst) = setup_device_registry();
        let ctx = make_ctx(&registry, dev_inst);

        // No password
        let data = [0x09, 0x00];
        let result = handler.handle(&data, &ctx);
        assert!(matches!(
            result,
            ServiceResult::Error {
                error_code: ErrorCode::PasswordFailure,
                ..
            }
        ));

        // Correct password
        let mut encoder = ApduEncoder::new();
        encoder.encode_context_enumerated(0, 0); // Coldstart
        encoder.encode_context_character_string(1, "admin");
        let data = encoder.into_bytes();
        let result = handler.handle(&data, &ctx);
        assert!(matches!(result, ServiceResult::SimpleAck));
    }

    #[test]
    fn test_bacnet_datetime_to_epoch() {
        // 2024-01-01 00:00:00 UTC should be 1704067200
        let date = BACnetDate {
            year: 124,
            month: 1,
            day: 1,
            day_of_week: 1,
        };
        let time = BACnetTime {
            hour: 0,
            minute: 0,
            second: 0,
            hundredths: 0,
        };
        let epoch = bacnet_datetime_to_epoch(&date, &time);
        assert_eq!(epoch, 1704067200);
    }

    #[test]
    fn test_enable_disable_from_u32() {
        assert_eq!(EnableDisable::from_u32(0), Some(EnableDisable::Enable));
        assert_eq!(EnableDisable::from_u32(1), Some(EnableDisable::Disable));
        assert_eq!(
            EnableDisable::from_u32(2),
            Some(EnableDisable::DisableInitiation)
        );
        assert_eq!(EnableDisable::from_u32(3), None);
    }

    #[test]
    fn test_reinitialized_state_from_u32() {
        assert_eq!(
            ReinitializedState::from_u32(0),
            Some(ReinitializedState::Coldstart)
        );
        assert_eq!(
            ReinitializedState::from_u32(1),
            Some(ReinitializedState::Warmstart)
        );
        assert_eq!(
            ReinitializedState::from_u32(7),
            Some(ReinitializedState::ActivateChanges)
        );
        assert_eq!(ReinitializedState::from_u32(8), None);
    }
}
