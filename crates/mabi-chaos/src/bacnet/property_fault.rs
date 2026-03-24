//! BACnet property access fault injection.
//!
//! Simulates failures in BACnet property read/write operations:
//! - Return wrong data type for a property
//! - Return out-of-range values
//! - Return stale (cached) values instead of current
//! - Corrupt floating-point values (NaN, Inf, denormalized)
//! - Return error for specific property IDs
//! - Swap property values between objects

use std::any::Any;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::context::FaultContext;
use crate::error::ChaosResult;
use crate::fault::{
    BaseFault, Fault, FaultBehavior, FaultCategory, FaultMetadata, FaultSeverity, FaultStatistics,
};

// =============================================================================
// Property Fault Type
// =============================================================================

/// Type of property-level fault to inject.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropertyFaultType {
    /// Return a value of the wrong data type (e.g., Unsigned instead of Real
    /// for Present_Value of an AnalogInput).
    WrongDataType {
        /// BACnet application tag number to force.
        /// 1=Boolean, 2=Unsigned, 3=Signed, 4=Real, 7=CharacterString, 9=Enumerated.
        forced_tag: u8,
    },

    /// Return a value outside the valid engineering range.
    OutOfRange {
        /// Offset to add to the value (can be negative to go below range).
        offset: f64,
        /// Or use a fixed out-of-range value.
        fixed_value: Option<f64>,
    },

    /// Return stale (old) data by caching the first read value and replaying it.
    StaleValue {
        /// Number of reads to cache before refreshing.
        stale_count: u32,
    },

    /// Corrupt floating-point values.
    FloatingPointCorruption {
        /// Type of floating-point corruption.
        corruption: FloatCorruption,
    },

    /// Return BACnet error for specific property reads.
    PropertyAccessError {
        /// BACnet error class (2=Property).
        error_class: u32,
        /// BACnet error code.
        error_code: u32,
    },

    /// Return a property with the wrong property identifier in the response.
    WrongPropertyId {
        /// The wrong property ID to return.
        wrong_property: u32,
    },

    /// Return array index out of bounds for array properties.
    ArrayIndexError,

    /// Flip status flags bits (in_alarm, fault, overridden, out_of_service).
    CorruptStatusFlags {
        /// Bitmask of which flags to flip (bit 0=in_alarm, 1=fault, 2=overridden, 3=oos).
        flip_mask: u8,
    },
}

impl Default for PropertyFaultType {
    fn default() -> Self {
        Self::WrongDataType { forced_tag: 2 }
    }
}

/// Type of floating-point corruption.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FloatCorruption {
    /// IEEE 754 NaN.
    #[default]
    NaN,
    /// Positive infinity.
    PosInfinity,
    /// Negative infinity.
    NegInfinity,
    /// Denormalized (subnormal) value.
    Denormalized,
    /// Negative zero.
    NegativeZero,
}

// =============================================================================
// Property Fault Configuration
// =============================================================================

/// Configuration for property-level fault injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyFaultConfig {
    /// Type of property fault.
    pub fault_type: PropertyFaultType,

    /// BACnet property IDs to target.
    /// Empty = target all properties.
    #[serde(default)]
    pub target_properties: Vec<u32>,

    /// BACnet object type codes to target.
    /// Empty = target all object types.
    #[serde(default)]
    pub target_object_types: Vec<u16>,

    /// Only affect read operations.
    #[serde(default = "default_true")]
    pub affect_reads: bool,

    /// Only affect write operations.
    #[serde(default)]
    pub affect_writes: bool,
}

fn default_true() -> bool {
    true
}

impl Default for PropertyFaultConfig {
    fn default() -> Self {
        Self {
            fault_type: PropertyFaultType::WrongDataType { forced_tag: 2 },
            target_properties: Vec::new(),
            target_object_types: Vec::new(),
            affect_reads: true,
            affect_writes: false,
        }
    }
}

impl PropertyFaultConfig {
    pub fn wrong_data_type(forced_tag: u8) -> Self {
        Self {
            fault_type: PropertyFaultType::WrongDataType { forced_tag },
            ..Default::default()
        }
    }

    pub fn out_of_range(offset: f64) -> Self {
        Self {
            fault_type: PropertyFaultType::OutOfRange {
                offset,
                fixed_value: None,
            },
            ..Default::default()
        }
    }

    pub fn out_of_range_fixed(value: f64) -> Self {
        Self {
            fault_type: PropertyFaultType::OutOfRange {
                offset: 0.0,
                fixed_value: Some(value),
            },
            ..Default::default()
        }
    }

    pub fn stale_value(stale_count: u32) -> Self {
        Self {
            fault_type: PropertyFaultType::StaleValue { stale_count },
            ..Default::default()
        }
    }

    pub fn float_corruption(corruption: FloatCorruption) -> Self {
        Self {
            fault_type: PropertyFaultType::FloatingPointCorruption { corruption },
            ..Default::default()
        }
    }

    pub fn property_error(error_class: u32, error_code: u32) -> Self {
        Self {
            fault_type: PropertyFaultType::PropertyAccessError {
                error_class,
                error_code,
            },
            ..Default::default()
        }
    }

    pub fn corrupt_status_flags(flip_mask: u8) -> Self {
        Self {
            fault_type: PropertyFaultType::CorruptStatusFlags { flip_mask },
            ..Default::default()
        }
    }

    pub fn for_properties(mut self, props: Vec<u32>) -> Self {
        self.target_properties = props;
        self
    }

    pub fn for_object_types(mut self, types: Vec<u16>) -> Self {
        self.target_object_types = types;
        self
    }

    pub fn writes_only(mut self) -> Self {
        self.affect_reads = false;
        self.affect_writes = true;
        self
    }

    pub fn both(mut self) -> Self {
        self.affect_reads = true;
        self.affect_writes = true;
        self
    }
}

// =============================================================================
// Property Fault Implementation
// =============================================================================

/// BACnet property access fault.
///
/// Corrupts property read/write responses to test client resilience
/// to unexpected property values and error conditions.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::bacnet::PropertyFault;
///
/// // Return NaN for Present_Value reads on analog objects
/// let fault = PropertyFault::builder()
///     .id("prop-nan-001")
///     .float_corruption(FloatCorruption::NaN)
///     .for_properties(vec![85]) // PresentValue
///     .for_object_types(vec![0, 1, 2]) // AI, AO, AV
///     .probability(0.05)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct PropertyFault {
    base: BaseFault,
    config: PropertyFaultConfig,
    /// Cached stale value for StaleValue fault.
    stale_cache: Option<Vec<u8>>,
    /// Number of reads since last cache refresh.
    stale_read_count: u32,
}

impl PropertyFault {
    pub fn new(id: impl Into<String>, config: PropertyFaultConfig) -> Self {
        let id = id.into();
        let metadata = FaultMetadata::new(&id, "BACnet Property Fault", FaultCategory::Protocol)
            .with_description("Corrupts BACnet property access responses")
            .with_severity(FaultSeverity::Medium)
            .with_tag("bacnet")
            .with_tag("property");

        Self {
            base: BaseFault::new(metadata),
            config,
            stale_cache: None,
            stale_read_count: 0,
        }
    }

    pub fn builder() -> PropertyFaultBuilder {
        PropertyFaultBuilder::default()
    }

    /// Check if a property ID matches the filter.
    pub fn matches_property(&self, prop_id: u32) -> bool {
        if self.config.target_properties.is_empty() {
            return true;
        }
        self.config.target_properties.contains(&prop_id)
    }

    /// Check if an object type matches the filter.
    pub fn matches_object_type(&self, obj_type: u16) -> bool {
        if self.config.target_object_types.is_empty() {
            return true;
        }
        self.config.target_object_types.contains(&obj_type)
    }

    /// Apply property-level corruption to raw response data.
    pub fn corrupt_property_data(&mut self, data: &mut Vec<u8>) {
        if data.is_empty() {
            return;
        }

        match &self.config.fault_type {
            PropertyFaultType::WrongDataType { forced_tag } => {
                // Replace the application tag of the value portion.
                // BACnet application tag format: (tag_number << 4) | length_value_type
                if data.len() > 4 {
                    // Find the value portion (after header) and change its tag
                    for i in 4..data.len() {
                        let byte = data[i];
                        // Check if this is an application tag (bit 3 clear)
                        if byte & 0x08 == 0 {
                            data[i] = (forced_tag << 4) | (byte & 0x0F);
                            break;
                        }
                    }
                }
            }

            PropertyFaultType::OutOfRange { fixed_value, .. } => {
                // Replace the last 4 bytes of the response with out-of-range float
                if data.len() >= 8 {
                    let val = fixed_value.unwrap_or(99999.0) as f32;
                    let bytes = val.to_be_bytes();
                    let start = data.len() - 4;
                    data[start..start + 4].copy_from_slice(&bytes);
                }
            }

            PropertyFaultType::StaleValue { stale_count } => {
                if let Some(ref cached) = self.stale_cache {
                    if self.stale_read_count < *stale_count {
                        // Return cached data
                        *data = cached.clone();
                        self.stale_read_count += 1;
                    } else {
                        // Refresh cache
                        self.stale_cache = Some(data.clone());
                        self.stale_read_count = 0;
                    }
                } else {
                    // First read: cache it
                    self.stale_cache = Some(data.clone());
                    self.stale_read_count = 0;
                }
            }

            PropertyFaultType::FloatingPointCorruption { corruption } => {
                if data.len() >= 8 {
                    let bytes = match corruption {
                        FloatCorruption::NaN => f32::NAN.to_be_bytes(),
                        FloatCorruption::PosInfinity => f32::INFINITY.to_be_bytes(),
                        FloatCorruption::NegInfinity => f32::NEG_INFINITY.to_be_bytes(),
                        FloatCorruption::Denormalized => {
                            // Smallest positive subnormal
                            1u32.to_be_bytes()
                        }
                        FloatCorruption::NegativeZero => (-0.0f32).to_be_bytes(),
                    };
                    let start = data.len() - 4;
                    data[start..start + 4].copy_from_slice(&bytes);
                }
            }

            PropertyFaultType::PropertyAccessError { .. } => {
                // This is handled at the behavior level, not data level
            }

            PropertyFaultType::WrongPropertyId { wrong_property } => {
                // Context tag [1] in ReadPropertyAck holds the property identifier.
                // Replace it with the wrong one.
                for i in 0..data.len().saturating_sub(2) {
                    let byte = data[i];
                    // Look for context tag [1] (0x19 = context 1, length 1)
                    if byte == 0x19 && i + 1 < data.len() {
                        data[i + 1] = *wrong_property as u8;
                        break;
                    }
                }
            }

            PropertyFaultType::ArrayIndexError => {
                // Return array-index-out-of-range error by replacing response
                // with an Error PDU payload
                *data = vec![
                    0x02, // Error class: Property (2)
                    0x32, // Error code: InvalidArrayIndex (50)
                ];
            }

            PropertyFaultType::CorruptStatusFlags { flip_mask } => {
                // StatusFlags in BACnet is a bitstring.
                // Find bit string tag (application tag 8) and flip bits.
                for i in 0..data.len().saturating_sub(2) {
                    let byte = data[i];
                    // Application tag 8 = 0x82 (tag=8, len=2)
                    if byte == 0x82 && i + 2 < data.len() {
                        // Byte at i+1 is unused-bits count, i+2 is the flags byte
                        data[i + 2] ^= flip_mask;
                        break;
                    }
                }
            }
        }
    }

    pub fn config(&self) -> &PropertyFaultConfig {
        &self.config
    }
}

#[async_trait]
impl Fault for PropertyFault {
    fn metadata(&self) -> &FaultMetadata {
        &self.base.metadata
    }

    fn enable(&mut self) {
        self.base.enable();
    }

    fn disable(&mut self) {
        self.base.disable();
    }

    async fn should_activate(&self, ctx: &FaultContext) -> ChaosResult<bool> {
        if !self.base.matches_target(ctx.target.identifier()) {
            return Ok(false);
        }

        // Check property filter
        if let Some(prop_str) = ctx.get_metadata("bacnet_property_id") {
            if let Ok(prop_id) = prop_str.parse::<u32>() {
                if !self.matches_property(prop_id) {
                    return Ok(false);
                }
            }
        }

        // Check object type filter
        if let Some(obj_str) = ctx.get_metadata("bacnet_object_type") {
            if let Ok(obj_type) = obj_str.parse::<u16>() {
                if !self.matches_object_type(obj_type) {
                    return Ok(false);
                }
            }
        }

        // Check read/write filter
        if self.config.affect_reads && !self.config.affect_writes && ctx.is_write() {
            return Ok(false);
        }
        if self.config.affect_writes && !self.config.affect_reads && ctx.is_read() {
            return Ok(false);
        }

        Ok(true)
    }

    async fn apply(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        if !self.clone().base.check_probability() {
            return Ok(FaultBehavior::Continue);
        }

        ctx.set_metadata(
            "bacnet_property_fault",
            &format!("{:?}", self.config.fault_type),
        );
        ctx.record_applied_fault(self.id(), "property_fault");

        tracing::debug!(
            fault_id = %self.id(),
            target = %ctx.target.device_id,
            fault_type = ?self.config.fault_type,
            "Applying BACnet property fault"
        );

        // For property access error, return error immediately
        if let PropertyFaultType::PropertyAccessError {
            error_class,
            error_code,
        } = &self.config.fault_type
        {
            return Ok(FaultBehavior::ReturnError {
                error_code: *error_code as i32,
                message: format!(
                    "BACnet property error: class={}, code={}",
                    error_class, error_code
                ),
            });
        }

        if let PropertyFaultType::ArrayIndexError = &self.config.fault_type {
            return Ok(FaultBehavior::ReturnError {
                error_code: 50,
                message: "BACnet: invalid-array-index".to_string(),
            });
        }

        Ok(FaultBehavior::Modify)
    }

    async fn after_operation(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        if ctx.get_metadata("bacnet_property_fault").is_none() {
            return Ok(FaultBehavior::Continue);
        }

        if let Some(ref mut response) = ctx.response_data {
            if let Some(ref mut raw) = response.raw {
                let mut this = self.clone();
                this.corrupt_property_data(raw);
            }
        }

        Ok(FaultBehavior::Continue)
    }

    fn reset(&mut self) {
        self.base.reset();
        self.stale_cache = None;
        self.stale_read_count = 0;
    }

    fn statistics(&self) -> FaultStatistics {
        self.base.statistics.clone()
    }

    fn clone_box(&self) -> Box<dyn Fault> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// =============================================================================
// Builder
// =============================================================================

#[derive(Debug, Default)]
pub struct PropertyFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: PropertyFaultConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl PropertyFaultBuilder {
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn fault_type(mut self, t: PropertyFaultType) -> Self {
        self.config.fault_type = t;
        self
    }

    pub fn wrong_data_type(mut self, forced_tag: u8) -> Self {
        self.config.fault_type = PropertyFaultType::WrongDataType { forced_tag };
        self
    }

    pub fn out_of_range(mut self, offset: f64) -> Self {
        self.config.fault_type = PropertyFaultType::OutOfRange {
            offset,
            fixed_value: None,
        };
        self
    }

    pub fn out_of_range_fixed(mut self, value: f64) -> Self {
        self.config.fault_type = PropertyFaultType::OutOfRange {
            offset: 0.0,
            fixed_value: Some(value),
        };
        self
    }

    pub fn stale_value(mut self, stale_count: u32) -> Self {
        self.config.fault_type = PropertyFaultType::StaleValue { stale_count };
        self
    }

    pub fn float_corruption(mut self, corruption: FloatCorruption) -> Self {
        self.config.fault_type = PropertyFaultType::FloatingPointCorruption { corruption };
        self
    }

    pub fn property_error(mut self, error_class: u32, error_code: u32) -> Self {
        self.config.fault_type = PropertyFaultType::PropertyAccessError {
            error_class,
            error_code,
        };
        self
    }

    pub fn corrupt_status_flags(mut self, flip_mask: u8) -> Self {
        self.config.fault_type = PropertyFaultType::CorruptStatusFlags { flip_mask };
        self
    }

    pub fn for_properties(mut self, props: Vec<u32>) -> Self {
        self.config.target_properties = props;
        self
    }

    pub fn for_object_types(mut self, types: Vec<u16>) -> Self {
        self.config.target_object_types = types;
        self
    }

    pub fn writes_only(mut self) -> Self {
        self.config.affect_reads = false;
        self.config.affect_writes = true;
        self
    }

    pub fn probability(mut self, p: f64) -> Self {
        self.probability = p.clamp(0.0, 1.0);
        self
    }

    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.targets.push(target.into());
        self
    }

    pub fn severity(mut self, severity: FaultSeverity) -> Self {
        self.severity = severity;
        self
    }

    pub fn build(self) -> PropertyFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = PropertyFault::new(&id, self.config);

        if let Some(name) = self.name {
            fault.base.metadata.name = name;
        }

        fault.base.metadata.probability = if self.probability == 0.0 {
            1.0
        } else {
            self.probability
        };
        fault.base.metadata.targets = self.targets;
        fault.base.metadata.severity = self.severity;

        fault
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_property_filter() {
        let config = PropertyFaultConfig::wrong_data_type(4).for_properties(vec![85, 111]);
        let fault = PropertyFault::new("test", config);

        assert!(fault.matches_property(85)); // PresentValue
        assert!(fault.matches_property(111)); // StatusFlags
        assert!(!fault.matches_property(77)); // ObjectName
    }

    #[test]
    fn test_object_type_filter() {
        let config = PropertyFaultConfig::wrong_data_type(4).for_object_types(vec![0, 1]);
        let fault = PropertyFault::new("test", config);

        assert!(fault.matches_object_type(0)); // AnalogInput
        assert!(fault.matches_object_type(1)); // AnalogOutput
        assert!(!fault.matches_object_type(3)); // BinaryInput
    }

    #[test]
    fn test_wrong_data_type_corruption() {
        let config = PropertyFaultConfig::wrong_data_type(2); // Force Unsigned tag
        let mut fault = PropertyFault::new("test", config);

        // Simulate a response with an application-tagged Real value at offset 4
        // Application tag 4 (Real), length 4 = 0x44
        let mut data = vec![
            0x0E, 0x09, 0x55, 0x1E, 0x44, 0x42, 0x34, 0x00, 0x00, 0x1F, 0x0F,
        ];
        fault.corrupt_property_data(&mut data);
        // The tag at offset 4 should now be 2 (Unsigned) instead of 4 (Real)
        assert_eq!((data[4] >> 4) & 0x0F, 2);
    }

    #[test]
    fn test_out_of_range_fixed() {
        let config = PropertyFaultConfig::out_of_range_fixed(99999.0);
        let mut fault = PropertyFault::new("test", config);

        let mut data = vec![0x00; 12];
        fault.corrupt_property_data(&mut data);
        let bytes = [data[8], data[9], data[10], data[11]];
        let val = f32::from_be_bytes(bytes);
        assert!((val - 99999.0).abs() < 1.0);
    }

    #[test]
    fn test_float_nan_corruption() {
        let config = PropertyFaultConfig::float_corruption(FloatCorruption::NaN);
        let mut fault = PropertyFault::new("test", config);

        let mut data = vec![0x00; 12];
        fault.corrupt_property_data(&mut data);
        let bytes = [data[8], data[9], data[10], data[11]];
        let val = f32::from_be_bytes(bytes);
        assert!(val.is_nan());
    }

    #[test]
    fn test_float_infinity_corruption() {
        let config = PropertyFaultConfig::float_corruption(FloatCorruption::PosInfinity);
        let mut fault = PropertyFault::new("test", config);

        let mut data = vec![0x00; 12];
        fault.corrupt_property_data(&mut data);
        let bytes = [data[8], data[9], data[10], data[11]];
        let val = f32::from_be_bytes(bytes);
        assert!(val.is_infinite() && val.is_sign_positive());
    }

    #[test]
    fn test_stale_value() {
        let config = PropertyFaultConfig::stale_value(3);
        let mut fault = PropertyFault::new("test", config);

        // First read: caches it
        let mut data1 = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        fault.corrupt_property_data(&mut data1);
        assert!(fault.stale_cache.is_some());

        // Second read: returns cached
        let mut data2 = vec![0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8];
        fault.corrupt_property_data(&mut data2);
        assert_eq!(data2, vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
    }

    #[test]
    fn test_array_index_error() {
        let config = PropertyFaultConfig {
            fault_type: PropertyFaultType::ArrayIndexError,
            ..Default::default()
        };
        let mut fault = PropertyFault::new("test", config);

        let mut data = vec![0x00; 10];
        fault.corrupt_property_data(&mut data);
        // Should be replaced with error payload
        assert_eq!(data, vec![0x02, 0x32]);
    }

    #[test]
    fn test_corrupt_status_flags() {
        let config = PropertyFaultConfig::corrupt_status_flags(0x05); // Flip in_alarm + overridden
        let mut fault = PropertyFault::new("test", config);

        // Simulate data with BitString tag (0x82 = app tag 8, length 2)
        let mut data = vec![0x00, 0x00, 0x00, 0x82, 0x04, 0x00]; // flags = 0x00
        fault.corrupt_property_data(&mut data);
        assert_eq!(data[5], 0x05); // in_alarm + overridden flipped
    }

    #[test]
    fn test_builder() {
        let fault = PropertyFault::builder()
            .id("prop-001")
            .float_corruption(FloatCorruption::NaN)
            .for_properties(vec![85])
            .probability(0.1)
            .target("bacnet-*")
            .build();

        assert_eq!(fault.id(), "prop-001");
        assert!(fault.base.metadata.tags.contains(&"property".to_string()));
    }
}
