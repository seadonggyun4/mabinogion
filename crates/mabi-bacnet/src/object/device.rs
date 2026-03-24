//! BACnet Device object implementation (Object Type 8).
//!
//! The Device object is mandatory per ASHRAE 135 Clause 12.11.
//! It represents the BACnet device itself, exposing protocol capabilities,
//! object inventory, and operational status.
//!
//! Required properties per ASHRAE 135-2020:
//! - Object_Identifier, Object_Name, Object_Type
//! - System_Status, Vendor_Name, Vendor_Identifier
//! - Model_Name, Firmware_Revision, Application_Software_Version
//! - Protocol_Version, Protocol_Revision
//! - Protocol_Services_Supported, Protocol_Object_Types_Supported
//! - Object_List, Max_APDU_Length_Accepted, Segmentation_Supported
//! - APDU_Timeout, Number_Of_APDU_Retries, Database_Revision

use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use super::property::{
    BACnetDate, BACnetTime, BACnetValue, EventState, PropertyError, PropertyId, Reliability,
    SegmentationSupport, StatusFlags,
};
use super::registry::ObjectRegistry;
use super::traits::{BACnetObject, CovSupport};
use super::types::{ObjectId, ObjectType};

/// BACnet device system status per ASHRAE 135 Clause 12.11.18.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum DeviceSystemStatus {
    Operational = 0,
    OperationalReadOnly = 1,
    DownloadRequired = 2,
    DownloadInProgress = 3,
    NonOperational = 4,
    BackupInProgress = 5,
}

impl DeviceSystemStatus {
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Operational),
            1 => Some(Self::OperationalReadOnly),
            2 => Some(Self::DownloadRequired),
            3 => Some(Self::DownloadInProgress),
            4 => Some(Self::NonOperational),
            5 => Some(Self::BackupInProgress),
            _ => None,
        }
    }
}

/// Configuration for creating a Device object.
#[derive(Debug, Clone)]
pub struct DeviceObjectConfig {
    /// Device instance number.
    pub device_instance: u32,
    /// Device name (Object_Name).
    pub device_name: String,
    /// Vendor name string.
    pub vendor_name: String,
    /// Vendor identifier (ASHRAE assigned).
    pub vendor_id: u16,
    /// Model name.
    pub model_name: String,
    /// Firmware revision string.
    pub firmware_revision: String,
    /// Application software version string.
    pub application_software_version: String,
    /// Description (optional).
    pub description: String,
    /// Location (optional).
    pub location: String,
    /// Maximum APDU length accepted.
    pub max_apdu_length: u16,
    /// Segmentation support level.
    pub segmentation_supported: SegmentationSupport,
    /// APDU timeout in milliseconds.
    pub apdu_timeout: u32,
    /// Number of APDU retries.
    pub number_of_apdu_retries: u32,
}

impl Default for DeviceObjectConfig {
    fn default() -> Self {
        Self {
            device_instance: 1234,
            device_name: "BACnet Simulator".into(),
            vendor_name: "OTSIM".into(),
            vendor_id: 999,
            model_name: "Mabinogion".into(),
            firmware_revision: "1.0.0".into(),
            application_software_version: "1.0.0".into(),
            description: String::new(),
            location: String::new(),
            max_apdu_length: 1476,
            segmentation_supported: SegmentationSupport::Both,
            apdu_timeout: 3000,
            number_of_apdu_retries: 3,
        }
    }
}

/// BACnet Device object (Object Type 8).
///
/// Mandatory per ASHRAE 135. Exactly one Device object must exist per BACnet device.
/// It provides protocol capability negotiation (Max_APDU_Length, Segmentation_Supported),
/// device inventory (Object_List), and operational status (System_Status).
///
/// The Device object holds an `Arc<ObjectRegistry>` to dynamically generate the
/// Object_List property from the live registry contents. Database_Revision is
/// automatically incremented whenever the registry mutates (objects added/removed).
pub struct DeviceObject {
    id: ObjectId,
    name: String,
    description: String,
    location: String,
    vendor_name: String,
    vendor_id: u16,
    model_name: String,
    firmware_revision: String,
    application_software_version: String,
    max_apdu_length: u16,
    segmentation_supported: SegmentationSupport,
    apdu_timeout: u32,
    number_of_apdu_retries: u32,
    system_status: RwLock<DeviceSystemStatus>,
    out_of_service: AtomicBool,
    /// Database revision counter. Incremented on each object add/remove.
    database_revision: AtomicU32,
    /// Reference to the object registry for dynamic Object_List generation.
    registry: Arc<ObjectRegistry>,
    /// Bit-packed services supported (40 bits per ASHRAE 135).
    services_supported: RwLock<Vec<bool>>,
    /// Bit-packed object types supported (60 bits per ASHRAE 135).
    object_types_supported: RwLock<Vec<bool>>,
    /// UTC offset in minutes.
    utc_offset: RwLock<i16>,
    /// Communication control state.
    communication_control: RwLock<CommunicationControlState>,
    #[allow(dead_code)]
    /// Last restore time.
    last_restore_time: RwLock<Option<BACnetDateTime>>,
    /// Time offset applied by TimeSynchronization (seconds from system clock).
    time_offset_secs: RwLock<i64>,
    /// COV tracking.
    cov_changed: AtomicBool,
    last_cov_status: RwLock<DeviceSystemStatus>,
}

/// Combined date+time for BACnet timestamps.
#[derive(Debug, Clone, Copy, Default)]
pub struct BACnetDateTime {
    pub date: BACnetDate,
    pub time: BACnetTime,
}

/// Communication control state per DeviceCommunicationControl service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommunicationControlState {
    /// Normal operation.
    Enabled,
    /// All communication disabled except I-Am and DCC.
    Disabled,
    /// Device will not initiate messages (responds only).
    DisabledInitiation,
}

impl Default for CommunicationControlState {
    fn default() -> Self {
        Self::Enabled
    }
}

impl DeviceObject {
    /// Create a new Device object from configuration.
    pub fn new(config: DeviceObjectConfig, registry: Arc<ObjectRegistry>) -> Self {
        let id = ObjectId::new(ObjectType::Device, config.device_instance);

        // Initialize services_supported bitstring (40 services per ASHRAE 135).
        // Indices correspond to ConfirmedService + UnconfirmedService enum values.
        let services_supported = vec![false; 41];

        // Initialize object_types_supported bitstring (60 object types).
        let object_types_supported = vec![false; 60];

        Self {
            id,
            name: config.device_name,
            description: config.description,
            location: config.location,
            vendor_name: config.vendor_name,
            vendor_id: config.vendor_id,
            model_name: config.model_name,
            firmware_revision: config.firmware_revision,
            application_software_version: config.application_software_version,
            max_apdu_length: config.max_apdu_length,
            segmentation_supported: config.segmentation_supported,
            apdu_timeout: config.apdu_timeout,
            number_of_apdu_retries: config.number_of_apdu_retries,
            system_status: RwLock::new(DeviceSystemStatus::Operational),
            out_of_service: AtomicBool::new(false),
            database_revision: AtomicU32::new(1),
            registry,
            services_supported: RwLock::new(services_supported),
            object_types_supported: RwLock::new(object_types_supported),
            utc_offset: RwLock::new(0),
            communication_control: RwLock::new(CommunicationControlState::default()),
            last_restore_time: RwLock::new(None),
            time_offset_secs: RwLock::new(0),
            cov_changed: AtomicBool::new(false),
            last_cov_status: RwLock::new(DeviceSystemStatus::Operational),
        }
    }

    /// Convenience constructor with minimal configuration.
    pub fn with_defaults(device_instance: u32, registry: Arc<ObjectRegistry>) -> Self {
        Self::new(
            DeviceObjectConfig {
                device_instance,
                ..Default::default()
            },
            registry,
        )
    }

    /// Get the device instance number.
    pub fn device_instance(&self) -> u32 {
        self.id.instance
    }

    /// Get the current system status.
    pub fn system_status(&self) -> DeviceSystemStatus {
        *self.system_status.read()
    }

    /// Set the system status.
    pub fn set_system_status(&self, status: DeviceSystemStatus) {
        let old = *self.system_status.read();
        *self.system_status.write() = status;
        if old != status {
            self.cov_changed.store(true, Ordering::Release);
        }
    }

    /// Get the communication control state.
    pub fn communication_control(&self) -> CommunicationControlState {
        *self.communication_control.read()
    }

    /// Get the time offset in seconds (applied by TimeSynchronization).
    pub fn time_offset_secs(&self) -> i64 {
        *self.time_offset_secs.read()
    }

    /// Set the time offset in seconds (called by TimeSynchronization handler).
    pub fn set_time_offset_secs(&self, offset: i64) {
        *self.time_offset_secs.write() = offset;
    }

    /// Set the communication control state.
    pub fn set_communication_control(&self, state: CommunicationControlState) {
        *self.communication_control.write() = state;
    }

    /// Increment the database revision (called when objects are added/removed).
    pub fn increment_database_revision(&self) -> u32 {
        self.database_revision.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Get the current database revision.
    pub fn database_revision(&self) -> u32 {
        self.database_revision.load(Ordering::Relaxed)
    }

    /// Update the services supported bitstring from a ServiceRegistry.
    ///
    /// This should be called after all service handlers are registered.
    /// Confirmed service indices (0-40) and unconfirmed service indices
    /// follow the ASHRAE 135 Table 12-13 numbering.
    pub fn update_services_supported(&self, confirmed: &[u8], unconfirmed: &[u8]) {
        let mut bits = self.services_supported.write();

        // Reset all
        for bit in bits.iter_mut() {
            *bit = false;
        }

        // Set confirmed services
        // ASHRAE 135 Table 12-13 service choice → bit index mapping:
        // The bit index matches the service choice number.
        for &service_choice in confirmed {
            let idx = service_choice as usize;
            if idx < bits.len() {
                bits[idx] = true;
            }
        }

        // Unconfirmed services are offset by the confirmed count in the bitstring.
        // Per ASHRAE 135, unconfirmed services occupy bits after confirmed ones.
        // However, the standard uses a combined bitstring where:
        // - Bits 0..n are confirmed services
        // - Bits n.. are unconfirmed services
        // For simplicity, we use the standard mapping where I-Am=bit 26 area, etc.
        // The exact mapping per ASHRAE 135 Clause 21 Table 21-1:
        for &service_choice in unconfirmed {
            // Unconfirmed services start after confirmed services in the bitstring.
            // Standard bit positions for common unconfirmed services:
            // I-Am: bit position depends on protocol revision.
            // We handle this by direct index if within range.
            let idx = service_choice as usize;
            if idx < bits.len() {
                bits[idx] = true;
            }
        }
    }

    /// Update the object types supported bitstring from the registry.
    ///
    /// Scans the registry and sets the bit for each object type that has
    /// at least one instance.
    pub fn update_object_types_supported(&self) {
        let mut bits = self.object_types_supported.write();

        // Reset all
        for bit in bits.iter_mut() {
            *bit = false;
        }

        // Device type is always supported
        let device_idx = ObjectType::Device as usize;
        if device_idx < bits.len() {
            bits[device_idx] = true;
        }

        // Scan registry for existing object types
        for object in self.registry.iter() {
            let type_idx = object.object_type() as usize;
            if type_idx < bits.len() {
                bits[type_idx] = true;
            }
        }
    }

    /// Build the Object_List property value.
    ///
    /// Dynamically generates the list from the live registry contents.
    /// The Device object itself is included as the first entry.
    fn build_object_list(&self) -> BACnetValue {
        let mut ids: Vec<BACnetValue> = Vec::new();

        // Device object is always first
        ids.push(BACnetValue::ObjectIdentifier(self.id));

        // All other objects from registry
        let mut registry_ids: Vec<ObjectId> = self.registry.object_ids();
        // Sort for deterministic ordering
        registry_ids.sort_by(|a, b| {
            (a.object_type as u16, a.instance).cmp(&(b.object_type as u16, b.instance))
        });

        for oid in registry_ids {
            // Skip if it's the device object itself (already added)
            if oid == self.id {
                continue;
            }
            ids.push(BACnetValue::ObjectIdentifier(oid));
        }

        BACnetValue::Array(ids)
    }

    /// Build the Active_COV_Subscriptions property value.
    ///
    /// Returns an empty list. The actual COV subscription data is managed
    /// by CovManager and would need to be injected if live data is needed.
    /// For the simulator, an empty list is acceptable as a baseline.
    fn build_active_cov_subscriptions(&self) -> BACnetValue {
        BACnetValue::Array(Vec::new())
    }
}

impl BACnetObject for DeviceObject {
    fn object_identifier(&self) -> ObjectId {
        self.id
    }

    fn object_name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> Option<&str> {
        if self.description.is_empty() {
            None
        } else {
            Some(&self.description)
        }
    }

    fn read_property(&self, property_id: PropertyId) -> Result<BACnetValue, PropertyError> {
        match property_id {
            // --- Required identification properties ---
            PropertyId::ObjectIdentifier => Ok(BACnetValue::ObjectIdentifier(self.id)),
            PropertyId::ObjectName => Ok(BACnetValue::CharacterString(self.name.clone())),
            PropertyId::ObjectType => Ok(BACnetValue::Enumerated(ObjectType::Device as u32)),
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),

            // --- Device status ---
            PropertyId::SystemStatus => {
                Ok(BACnetValue::Enumerated(*self.system_status.read() as u32))
            }
            PropertyId::StatusFlags => Ok(BACnetValue::BitString(self.status_flags().to_bits())),
            PropertyId::EventState => Ok(BACnetValue::Enumerated(EventState::Normal as u32)),
            PropertyId::Reliability => {
                Ok(BACnetValue::Enumerated(Reliability::NoFaultDetected as u32))
            }
            PropertyId::OutOfService => Ok(BACnetValue::Boolean(
                self.out_of_service.load(Ordering::Acquire),
            )),

            // --- Vendor information ---
            PropertyId::VendorName => Ok(BACnetValue::CharacterString(self.vendor_name.clone())),
            PropertyId::VendorIdentifier => Ok(BACnetValue::Unsigned(self.vendor_id as u32)),
            PropertyId::ModelName => Ok(BACnetValue::CharacterString(self.model_name.clone())),
            PropertyId::FirmwareRevision => {
                Ok(BACnetValue::CharacterString(self.firmware_revision.clone()))
            }
            PropertyId::ApplicationSoftwareVersion => Ok(BACnetValue::CharacterString(
                self.application_software_version.clone(),
            )),
            PropertyId::Location => Ok(BACnetValue::CharacterString(self.location.clone())),

            // --- Protocol capabilities ---
            PropertyId::ProtocolVersion => Ok(BACnetValue::Unsigned(1)), // Always 1
            PropertyId::ProtocolRevision => Ok(BACnetValue::Unsigned(22)), // ASHRAE 135-2020
            PropertyId::ProtocolServicesSupported => Ok(BACnetValue::BitString(
                self.services_supported.read().clone(),
            )),
            PropertyId::ProtocolObjectTypesSupported => Ok(BACnetValue::BitString(
                self.object_types_supported.read().clone(),
            )),

            // --- Network configuration ---
            PropertyId::MaxApduLengthAccepted => {
                Ok(BACnetValue::Unsigned(self.max_apdu_length as u32))
            }
            PropertyId::SegmentationSupported => {
                Ok(BACnetValue::Enumerated(self.segmentation_supported as u32))
            }
            PropertyId::ApduTimeout => Ok(BACnetValue::Unsigned(self.apdu_timeout)),
            PropertyId::NumberOfApduRetries => {
                Ok(BACnetValue::Unsigned(self.number_of_apdu_retries))
            }

            // --- Object management ---
            PropertyId::ObjectList => Ok(self.build_object_list()),
            PropertyId::DatabaseRevision => Ok(BACnetValue::Unsigned(
                self.database_revision.load(Ordering::Relaxed),
            )),

            // --- Time ---
            PropertyId::LocalDate => {
                let now = std::time::SystemTime::now();
                let since_epoch = now
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                let offset = *self.time_offset_secs.read();
                let adjusted_secs = since_epoch.as_secs() as i64 + offset;
                let total_days = adjusted_secs / 86400;
                let (year, month, day) = days_to_ymd(total_days + 719468);
                let dow = ((total_days.rem_euclid(7) + 4) % 7 + 1) as u8; // 1=Mon
                Ok(BACnetValue::Date(BACnetDate {
                    year: ((year - 1900) & 0xFF) as u8,
                    month: month as u8,
                    day: day as u8,
                    day_of_week: dow,
                }))
            }
            PropertyId::LocalTime => {
                let now = std::time::SystemTime::now();
                let since_epoch = now
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                let offset = *self.time_offset_secs.read();
                let adjusted_secs = since_epoch.as_secs() as i64 + offset;
                let secs_today = adjusted_secs.rem_euclid(86400) as u64;
                let millis = since_epoch.subsec_millis();
                Ok(BACnetValue::Time(BACnetTime {
                    hour: (secs_today / 3600) as u8,
                    minute: ((secs_today % 3600) / 60) as u8,
                    second: (secs_today % 60) as u8,
                    hundredths: (millis / 10) as u8,
                }))
            }
            PropertyId::UtcOffset => Ok(BACnetValue::Signed(*self.utc_offset.read() as i32)),

            // --- COV ---
            PropertyId::ActiveCovSubscriptions => Ok(self.build_active_cov_subscriptions()),

            // --- MaxSegmentsAccepted ---
            PropertyId::MaxSegmentsAccepted => Ok(BACnetValue::Unsigned(64)),

            // --- Device address binding ---
            PropertyId::DeviceAddressBinding => Ok(BACnetValue::Array(Vec::new())),

            _ => Err(PropertyError::NotFound(property_id)),
        }
    }

    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError> {
        match property_id {
            // Read-only properties
            PropertyId::ObjectIdentifier
            | PropertyId::ObjectType
            | PropertyId::VendorIdentifier
            | PropertyId::ProtocolVersion
            | PropertyId::ProtocolRevision
            | PropertyId::ProtocolServicesSupported
            | PropertyId::ProtocolObjectTypesSupported
            | PropertyId::ObjectList
            | PropertyId::DatabaseRevision
            | PropertyId::StatusFlags
            | PropertyId::EventState
            | PropertyId::SystemStatus => Err(PropertyError::ReadOnly(property_id)),

            PropertyId::OutOfService => {
                if let Some(v) = value.as_bool() {
                    self.out_of_service.store(v, Ordering::Release);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }

            PropertyId::UtcOffset => {
                if let Some(v) = value.as_unsigned() {
                    *self.utc_offset.write() = v as i16;
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }

            PropertyId::Location => {
                // Location is technically writable but we store it in a field
                Err(PropertyError::WriteAccessDenied(property_id))
            }

            _ => Err(PropertyError::NotFound(property_id)),
        }
    }

    fn list_properties(&self) -> Vec<PropertyId> {
        vec![
            // Identification (required)
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            // Status (required)
            PropertyId::SystemStatus,
            PropertyId::StatusFlags,
            PropertyId::EventState,
            PropertyId::Reliability,
            PropertyId::OutOfService,
            // Vendor (required)
            PropertyId::VendorName,
            PropertyId::VendorIdentifier,
            PropertyId::ModelName,
            PropertyId::FirmwareRevision,
            PropertyId::ApplicationSoftwareVersion,
            PropertyId::Location,
            // Protocol (required)
            PropertyId::ProtocolVersion,
            PropertyId::ProtocolRevision,
            PropertyId::ProtocolServicesSupported,
            PropertyId::ProtocolObjectTypesSupported,
            // Network (required)
            PropertyId::MaxApduLengthAccepted,
            PropertyId::SegmentationSupported,
            PropertyId::ApduTimeout,
            PropertyId::NumberOfApduRetries,
            PropertyId::MaxSegmentsAccepted,
            // Object management (required)
            PropertyId::ObjectList,
            PropertyId::DatabaseRevision,
            // Time
            PropertyId::LocalDate,
            PropertyId::LocalTime,
            PropertyId::UtcOffset,
            // COV
            PropertyId::ActiveCovSubscriptions,
            // Address binding
            PropertyId::DeviceAddressBinding,
        ]
    }

    fn status_flags(&self) -> StatusFlags {
        let status = *self.system_status.read();
        StatusFlags {
            in_alarm: false,
            fault: matches!(status, DeviceSystemStatus::NonOperational),
            overridden: false,
            out_of_service: self.out_of_service.load(Ordering::Acquire),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl CovSupport for DeviceObject {
    fn cov_increment(&self) -> Option<f32> {
        None // Device object uses event-based COV, not increment
    }

    fn check_cov(&self) -> bool {
        self.cov_changed.load(Ordering::Acquire)
    }

    fn cov_values(&self) -> Vec<(PropertyId, BACnetValue)> {
        vec![
            (
                PropertyId::SystemStatus,
                BACnetValue::Enumerated(*self.system_status.read() as u32),
            ),
            (
                PropertyId::StatusFlags,
                BACnetValue::BitString(self.status_flags().to_bits()),
            ),
        ]
    }

    fn reset_cov(&self) {
        *self.last_cov_status.write() = *self.system_status.read();
        self.cov_changed.store(false, Ordering::Release);
    }
}

/// Civil date calculation: convert days since epoch to (year, month, day).
///
/// Uses the algorithm from Howard Hinnant's `chrono`-compatible date conversion.
/// Input: days since the civil epoch (March 1, 0000).
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    let era = if days >= 0 {
        days / 146097
    } else {
        (days - 146096) / 146097
    };
    let doe = (days - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_device() -> DeviceObject {
        let registry = Arc::new(ObjectRegistry::new());
        let config = DeviceObjectConfig {
            device_instance: 1234,
            device_name: "Test Device".into(),
            vendor_name: "Test Vendor".into(),
            vendor_id: 42,
            model_name: "TestModel".into(),
            firmware_revision: "2.0".into(),
            application_software_version: "1.5".into(),
            description: "A test device".into(),
            location: "Lab".into(),
            max_apdu_length: 1476,
            segmentation_supported: SegmentationSupport::Both,
            apdu_timeout: 3000,
            number_of_apdu_retries: 3,
        };
        DeviceObject::new(config, registry)
    }

    #[test]
    fn test_device_identification() {
        let dev = make_device();
        assert_eq!(
            dev.object_identifier(),
            ObjectId::new(ObjectType::Device, 1234)
        );
        assert_eq!(dev.object_name(), "Test Device");
        assert_eq!(dev.object_type(), ObjectType::Device);
    }

    #[test]
    fn test_device_required_properties() {
        let dev = make_device();

        // All required properties must be readable
        let required = vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::SystemStatus,
            PropertyId::VendorName,
            PropertyId::VendorIdentifier,
            PropertyId::ModelName,
            PropertyId::FirmwareRevision,
            PropertyId::ApplicationSoftwareVersion,
            PropertyId::ProtocolVersion,
            PropertyId::ProtocolRevision,
            PropertyId::ProtocolServicesSupported,
            PropertyId::ProtocolObjectTypesSupported,
            PropertyId::ObjectList,
            PropertyId::MaxApduLengthAccepted,
            PropertyId::SegmentationSupported,
            PropertyId::ApduTimeout,
            PropertyId::NumberOfApduRetries,
            PropertyId::DatabaseRevision,
        ];

        for prop in required {
            let result = dev.read_property(prop);
            assert!(
                result.is_ok(),
                "Required property {:?} failed: {:?}",
                prop,
                result.err()
            );
        }
    }

    #[test]
    fn test_device_vendor_info() {
        let dev = make_device();

        assert_eq!(
            dev.read_property(PropertyId::VendorName)
                .unwrap()
                .as_string(),
            Some("Test Vendor")
        );
        assert_eq!(
            dev.read_property(PropertyId::VendorIdentifier)
                .unwrap()
                .as_unsigned(),
            Some(42)
        );
        assert_eq!(
            dev.read_property(PropertyId::ModelName)
                .unwrap()
                .as_string(),
            Some("TestModel")
        );
    }

    #[test]
    fn test_device_protocol_info() {
        let dev = make_device();

        assert_eq!(
            dev.read_property(PropertyId::ProtocolVersion)
                .unwrap()
                .as_unsigned(),
            Some(1)
        );
        assert_eq!(
            dev.read_property(PropertyId::ProtocolRevision)
                .unwrap()
                .as_unsigned(),
            Some(22)
        );
        assert_eq!(
            dev.read_property(PropertyId::MaxApduLengthAccepted)
                .unwrap()
                .as_unsigned(),
            Some(1476)
        );
        assert_eq!(
            dev.read_property(PropertyId::SegmentationSupported)
                .unwrap()
                .as_unsigned(),
            Some(SegmentationSupport::Both as u32)
        );
    }

    #[test]
    fn test_device_system_status() {
        let dev = make_device();

        assert_eq!(dev.system_status(), DeviceSystemStatus::Operational);
        assert_eq!(
            dev.read_property(PropertyId::SystemStatus)
                .unwrap()
                .as_unsigned(),
            Some(0) // Operational
        );

        dev.set_system_status(DeviceSystemStatus::NonOperational);
        assert_eq!(dev.system_status(), DeviceSystemStatus::NonOperational);
        assert!(dev.status_flags().fault); // NonOperational → fault flag set
    }

    #[test]
    fn test_device_object_list() {
        let registry = Arc::new(ObjectRegistry::new());

        // Add some objects to registry
        use super::super::standard::AnalogInput;
        registry.register(Arc::new(AnalogInput::new(0, "AI_0")));
        registry.register(Arc::new(AnalogInput::new(1, "AI_1")));

        let dev = DeviceObject::with_defaults(1234, registry);
        let obj_list = dev.read_property(PropertyId::ObjectList).unwrap();

        match obj_list {
            BACnetValue::Array(items) => {
                // Device + 2 AIs = 3
                assert_eq!(items.len(), 3);
                // First entry is the Device object itself
                assert_eq!(
                    items[0].as_object_id(),
                    Some(ObjectId::new(ObjectType::Device, 1234))
                );
            }
            _ => panic!("Object_List should be an Array"),
        }
    }

    #[test]
    fn test_device_database_revision() {
        let dev = make_device();

        let rev1 = dev.database_revision();
        let rev2 = dev.increment_database_revision();
        assert_eq!(rev2, rev1 + 1);
        assert_eq!(dev.database_revision(), rev2);
    }

    #[test]
    fn test_device_read_only_properties() {
        let dev = make_device();

        let read_only = vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectType,
            PropertyId::ProtocolVersion,
            PropertyId::ProtocolRevision,
            PropertyId::ObjectList,
            PropertyId::DatabaseRevision,
        ];

        for prop in read_only {
            let result = dev.write_property(prop, BACnetValue::Unsigned(0));
            assert!(
                matches!(result, Err(PropertyError::ReadOnly(_))),
                "Property {:?} should be read-only",
                prop
            );
        }
    }

    #[test]
    fn test_device_cov_support() {
        let dev = make_device();

        // Initially no COV
        assert!(!dev.check_cov());

        // Change system status triggers COV
        dev.set_system_status(DeviceSystemStatus::OperationalReadOnly);
        assert!(dev.check_cov());

        // Reset clears COV
        dev.reset_cov();
        assert!(!dev.check_cov());
    }

    #[test]
    fn test_device_communication_control() {
        let dev = make_device();

        assert_eq!(
            dev.communication_control(),
            CommunicationControlState::Enabled
        );
        dev.set_communication_control(CommunicationControlState::Disabled);
        assert_eq!(
            dev.communication_control(),
            CommunicationControlState::Disabled
        );
    }

    #[test]
    fn test_device_services_supported_update() {
        let dev = make_device();

        // Simulate registering ReadProperty(12) and WriteProperty(15)
        dev.update_services_supported(&[12, 15], &[8]); // 8 = Who-Is

        let bits = dev
            .read_property(PropertyId::ProtocolServicesSupported)
            .unwrap();
        match bits {
            BACnetValue::BitString(b) => {
                assert!(b[12]); // ReadProperty
                assert!(b[15]); // WriteProperty
                assert!(b[8]); // Who-Is (mapped to bit 8)
                assert!(!b[0]); // AcknowledgeAlarm not registered
            }
            _ => panic!("Expected BitString"),
        }
    }

    #[test]
    fn test_device_object_types_supported_update() {
        let registry = Arc::new(ObjectRegistry::new());
        use super::super::standard::{AnalogInput, BinaryOutput};
        registry.register(Arc::new(AnalogInput::new(0, "AI_0")));
        registry.register(Arc::new(BinaryOutput::new(0, "BO_0")));

        let dev = DeviceObject::with_defaults(1234, registry);
        dev.update_object_types_supported();

        let bits = dev
            .read_property(PropertyId::ProtocolObjectTypesSupported)
            .unwrap();
        match bits {
            BACnetValue::BitString(b) => {
                assert!(b[ObjectType::Device as usize]); // Always
                assert!(b[ObjectType::AnalogInput as usize]); // AI registered
                assert!(b[ObjectType::BinaryOutput as usize]); // BO registered
                assert!(!b[ObjectType::AnalogValue as usize]); // AV not registered
            }
            _ => panic!("Expected BitString"),
        }
    }

    #[test]
    fn test_device_list_properties() {
        let dev = make_device();
        let props = dev.list_properties();

        // Must include all required properties
        assert!(props.contains(&PropertyId::ObjectIdentifier));
        assert!(props.contains(&PropertyId::SystemStatus));
        assert!(props.contains(&PropertyId::VendorName));
        assert!(props.contains(&PropertyId::ProtocolVersion));
        assert!(props.contains(&PropertyId::ObjectList));
        assert!(props.contains(&PropertyId::DatabaseRevision));
        assert!(props.contains(&PropertyId::MaxApduLengthAccepted));
    }

    #[test]
    fn test_device_time_properties() {
        let dev = make_device();

        // LocalDate should return a Date
        let date = dev.read_property(PropertyId::LocalDate);
        assert!(date.is_ok());
        assert!(matches!(date.unwrap(), BACnetValue::Date(_)));

        // LocalTime should return a Time
        let time = dev.read_property(PropertyId::LocalTime);
        assert!(time.is_ok());
        assert!(matches!(time.unwrap(), BACnetValue::Time(_)));
    }
}
