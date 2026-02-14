//! Standard BACnet object implementations.
//!
//! This module provides implementations for common BACnet object types:
//! - Analog Input/Output/Value
//! - Binary Input/Output/Value
//! - Multi-state Input/Output/Value

use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use super::property::{
    BACnetValue, EventState, Polarity, PropertyError, PropertyId, PropertyStore, Reliability,
    StatusFlags,
};
use super::traits::{BACnetObject, CovSupport, WritableObject};
use super::types::{ObjectId, ObjectType};

// ============================================================================
// Analog Input
// ============================================================================

/// Analog Input object (read-only analog sensor).
pub struct AnalogInput {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,
    present_value: RwLock<f32>,
    out_of_service: AtomicBool,
    // COV support
    cov_increment: RwLock<f32>,
    last_cov_value: RwLock<f32>,
    cov_changed: AtomicBool,
}

impl AnalogInput {
    /// Create a new Analog Input.
    pub fn new(instance: u32, name: impl Into<String>) -> Self {
        let id = ObjectId::new(ObjectType::AnalogInput, instance);
        let name = name.into();
        let properties = PropertyStore::new();

        // Initialize default properties
        properties.set(PropertyId::Units, BACnetValue::Enumerated(95)); // No units
        properties.set(
            PropertyId::EventState,
            BACnetValue::Enumerated(EventState::Normal as u32),
        );
        properties.set(
            PropertyId::Reliability,
            BACnetValue::Enumerated(Reliability::NoFaultDetected as u32),
        );

        Self {
            id,
            name,
            description: String::new(),
            properties,
            present_value: RwLock::new(0.0),
            out_of_service: AtomicBool::new(false),
            cov_increment: RwLock::new(0.1),
            last_cov_value: RwLock::new(0.0),
            cov_changed: AtomicBool::new(false),
        }
    }

    /// Create with description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Set engineering units.
    pub fn with_units(self, units: u16) -> Self {
        self.properties.set(PropertyId::Units, BACnetValue::Enumerated(units as u32));
        self
    }

    /// Set COV increment.
    pub fn with_cov_increment(self, increment: f32) -> Self {
        *self.cov_increment.write() = increment;
        self
    }

    /// Set the present value (for simulation).
    pub fn set_value(&self, value: f32) {
        let mut pv = self.present_value.write();
        *pv = value;
        drop(pv);

        // Check COV
        let increment = *self.cov_increment.read();
        let last = *self.last_cov_value.read();
        if (value - last).abs() >= increment {
            self.cov_changed.store(true, Ordering::Release);
        }
    }

    /// Get the present value.
    pub fn get_value(&self) -> f32 {
        *self.present_value.read()
    }
}

impl BACnetObject for AnalogInput {
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
            PropertyId::ObjectIdentifier => Ok(BACnetValue::ObjectIdentifier(self.id)),
            PropertyId::ObjectName => Ok(BACnetValue::CharacterString(self.name.clone())),
            PropertyId::ObjectType => {
                Ok(BACnetValue::Enumerated(ObjectType::AnalogInput as u32))
            }
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::PresentValue => Ok(BACnetValue::Real(*self.present_value.read())),
            PropertyId::StatusFlags => Ok(BACnetValue::BitString(self.status_flags().to_bits())),
            PropertyId::OutOfService => {
                Ok(BACnetValue::Boolean(self.out_of_service.load(Ordering::Acquire)))
            }
            PropertyId::CovIncrement => Ok(BACnetValue::Real(*self.cov_increment.read())),
            _ => self
                .properties
                .get(property_id)
                .ok_or(PropertyError::NotFound(property_id)),
        }
    }

    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier | PropertyId::ObjectType => {
                Err(PropertyError::ReadOnly(property_id))
            }
            PropertyId::OutOfService => {
                if let Some(v) = value.as_bool() {
                    self.out_of_service.store(v, Ordering::Release);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::CovIncrement => {
                if let Some(v) = value.as_real() {
                    *self.cov_increment.write() = v;
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            _ => {
                self.properties.set(property_id, value);
                Ok(())
            }
        }
    }

    fn list_properties(&self) -> Vec<PropertyId> {
        vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::PresentValue,
            PropertyId::StatusFlags,
            PropertyId::EventState,
            PropertyId::Reliability,
            PropertyId::OutOfService,
            PropertyId::Units,
            PropertyId::CovIncrement,
        ]
    }

    fn status_flags(&self) -> StatusFlags {
        StatusFlags {
            in_alarm: false,
            fault: false,
            overridden: false,
            out_of_service: self.out_of_service.load(Ordering::Acquire),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl CovSupport for AnalogInput {
    fn cov_increment(&self) -> Option<f32> {
        Some(*self.cov_increment.read())
    }

    fn set_cov_increment(&self, increment: f32) -> Result<(), PropertyError> {
        *self.cov_increment.write() = increment;
        Ok(())
    }

    fn check_cov(&self) -> bool {
        self.cov_changed.load(Ordering::Acquire)
    }

    fn reset_cov(&self) {
        *self.last_cov_value.write() = *self.present_value.read();
        self.cov_changed.store(false, Ordering::Release);
    }
}

// ============================================================================
// Analog Output
// ============================================================================

/// Analog Output object (writable analog control).
pub struct AnalogOutput {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,
    present_value: RwLock<f32>,
    relinquish_default: RwLock<f32>,
    priority_array: RwLock<[Option<f32>; 16]>,
    out_of_service: AtomicBool,
    cov_increment: RwLock<f32>,
    last_cov_value: RwLock<f32>,
    cov_changed: AtomicBool,
}

impl AnalogOutput {
    /// Create a new Analog Output.
    pub fn new(instance: u32, name: impl Into<String>) -> Self {
        let id = ObjectId::new(ObjectType::AnalogOutput, instance);
        let properties = PropertyStore::new();

        properties.set(PropertyId::Units, BACnetValue::Enumerated(95));
        properties.set(
            PropertyId::EventState,
            BACnetValue::Enumerated(EventState::Normal as u32),
        );

        Self {
            id,
            name: name.into(),
            description: String::new(),
            properties,
            present_value: RwLock::new(0.0),
            relinquish_default: RwLock::new(0.0),
            priority_array: RwLock::new([None; 16]),
            out_of_service: AtomicBool::new(false),
            cov_increment: RwLock::new(0.1),
            last_cov_value: RwLock::new(0.0),
            cov_changed: AtomicBool::new(false),
        }
    }

    /// Create with description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Set engineering units.
    pub fn with_units(self, units: u16) -> Self {
        self.properties.set(PropertyId::Units, BACnetValue::Enumerated(units as u32));
        self
    }

    /// Set relinquish default.
    pub fn with_relinquish_default(self, value: f32) -> Self {
        *self.relinquish_default.write() = value;
        self.update_present_value();
        self
    }

    /// Update present value from priority array.
    fn update_present_value(&self) {
        let pa = self.priority_array.read();
        let mut pv = self.present_value.write();

        // Find highest priority (lowest index) with a value
        let new_value = pa
            .iter()
            .find_map(|v| *v)
            .unwrap_or(*self.relinquish_default.read());

        if (*pv - new_value).abs() >= *self.cov_increment.read() {
            self.cov_changed.store(true, Ordering::Release);
        }

        *pv = new_value;
    }
}

impl BACnetObject for AnalogOutput {
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
            PropertyId::ObjectIdentifier => Ok(BACnetValue::ObjectIdentifier(self.id)),
            PropertyId::ObjectName => Ok(BACnetValue::CharacterString(self.name.clone())),
            PropertyId::ObjectType => {
                Ok(BACnetValue::Enumerated(ObjectType::AnalogOutput as u32))
            }
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::PresentValue => Ok(BACnetValue::Real(*self.present_value.read())),
            PropertyId::StatusFlags => Ok(BACnetValue::BitString(self.status_flags().to_bits())),
            PropertyId::OutOfService => {
                Ok(BACnetValue::Boolean(self.out_of_service.load(Ordering::Acquire)))
            }
            PropertyId::RelinquishDefault => {
                Ok(BACnetValue::Real(*self.relinquish_default.read()))
            }
            PropertyId::PriorityArray => {
                let pa = self.priority_array.read();
                let values: Vec<BACnetValue> = pa
                    .iter()
                    .map(|v| match v {
                        Some(f) => BACnetValue::Real(*f),
                        None => BACnetValue::Null,
                    })
                    .collect();
                Ok(BACnetValue::Array(values))
            }
            PropertyId::CovIncrement => Ok(BACnetValue::Real(*self.cov_increment.read())),
            _ => self
                .properties
                .get(property_id)
                .ok_or(PropertyError::NotFound(property_id)),
        }
    }

    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier | PropertyId::ObjectType => {
                Err(PropertyError::ReadOnly(property_id))
            }
            PropertyId::PresentValue => {
                if let Some(v) = value.as_real() {
                    // Default to priority 16
                    let mut pa = self.priority_array.write();
                    pa[15] = Some(v);
                    drop(pa);
                    self.update_present_value();
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::OutOfService => {
                if let Some(v) = value.as_bool() {
                    self.out_of_service.store(v, Ordering::Release);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::RelinquishDefault => {
                if let Some(v) = value.as_real() {
                    *self.relinquish_default.write() = v;
                    self.update_present_value();
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            _ => {
                self.properties.set(property_id, value);
                Ok(())
            }
        }
    }

    fn list_properties(&self) -> Vec<PropertyId> {
        vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::PresentValue,
            PropertyId::StatusFlags,
            PropertyId::EventState,
            PropertyId::OutOfService,
            PropertyId::Units,
            PropertyId::PriorityArray,
            PropertyId::RelinquishDefault,
            PropertyId::CovIncrement,
        ]
    }

    fn status_flags(&self) -> StatusFlags {
        StatusFlags {
            in_alarm: false,
            fault: false,
            overridden: false,
            out_of_service: self.out_of_service.load(Ordering::Acquire),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl WritableObject for AnalogOutput {
    fn set_present_value(&self, value: BACnetValue) -> Result<(), PropertyError> {
        self.write_property(PropertyId::PresentValue, value)
    }

    fn set_present_value_with_priority(
        &self,
        value: BACnetValue,
        priority: u8,
    ) -> Result<(), PropertyError> {
        if priority < 1 || priority > 16 {
            return Err(PropertyError::ValueOutOfRange(PropertyId::PresentValue));
        }

        if let Some(v) = value.as_real() {
            let mut pa = self.priority_array.write();
            pa[(priority - 1) as usize] = Some(v);
            drop(pa);
            self.update_present_value();
            Ok(())
        } else if value.is_null() {
            // Relinquish this priority
            let mut pa = self.priority_array.write();
            pa[(priority - 1) as usize] = None;
            drop(pa);
            self.update_present_value();
            Ok(())
        } else {
            Err(PropertyError::InvalidDataType(PropertyId::PresentValue))
        }
    }

    fn relinquish_default(&self) -> Option<BACnetValue> {
        Some(BACnetValue::Real(*self.relinquish_default.read()))
    }

    fn relinquish(&self, priority: u8) -> Result<(), PropertyError> {
        if priority < 1 || priority > 16 {
            return Err(PropertyError::ValueOutOfRange(PropertyId::PriorityArray));
        }

        let mut pa = self.priority_array.write();
        pa[(priority - 1) as usize] = None;
        drop(pa);
        self.update_present_value();
        Ok(())
    }
}

impl CovSupport for AnalogOutput {
    fn cov_increment(&self) -> Option<f32> {
        Some(*self.cov_increment.read())
    }

    fn check_cov(&self) -> bool {
        self.cov_changed.load(Ordering::Acquire)
    }

    fn reset_cov(&self) {
        *self.last_cov_value.write() = *self.present_value.read();
        self.cov_changed.store(false, Ordering::Release);
    }
}

// ============================================================================
// Analog Value
// ============================================================================

/// Analog Value object (writable analog point).
pub struct AnalogValue {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,
    present_value: RwLock<f32>,
    out_of_service: AtomicBool,
    cov_increment: RwLock<f32>,
    last_cov_value: RwLock<f32>,
    cov_changed: AtomicBool,
}

impl AnalogValue {
    /// Create a new Analog Value.
    pub fn new(instance: u32, name: impl Into<String>) -> Self {
        let id = ObjectId::new(ObjectType::AnalogValue, instance);
        let properties = PropertyStore::new();

        properties.set(PropertyId::Units, BACnetValue::Enumerated(95));
        properties.set(
            PropertyId::EventState,
            BACnetValue::Enumerated(EventState::Normal as u32),
        );

        Self {
            id,
            name: name.into(),
            description: String::new(),
            properties,
            present_value: RwLock::new(0.0),
            out_of_service: AtomicBool::new(false),
            cov_increment: RwLock::new(0.1),
            last_cov_value: RwLock::new(0.0),
            cov_changed: AtomicBool::new(false),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_units(self, units: u16) -> Self {
        self.properties.set(PropertyId::Units, BACnetValue::Enumerated(units as u32));
        self
    }

    pub fn with_initial_value(self, value: f32) -> Self {
        *self.present_value.write() = value;
        *self.last_cov_value.write() = value;
        self
    }
}

impl BACnetObject for AnalogValue {
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
            PropertyId::ObjectIdentifier => Ok(BACnetValue::ObjectIdentifier(self.id)),
            PropertyId::ObjectName => Ok(BACnetValue::CharacterString(self.name.clone())),
            PropertyId::ObjectType => {
                Ok(BACnetValue::Enumerated(ObjectType::AnalogValue as u32))
            }
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::PresentValue => Ok(BACnetValue::Real(*self.present_value.read())),
            PropertyId::StatusFlags => Ok(BACnetValue::BitString(self.status_flags().to_bits())),
            PropertyId::OutOfService => {
                Ok(BACnetValue::Boolean(self.out_of_service.load(Ordering::Acquire)))
            }
            PropertyId::CovIncrement => Ok(BACnetValue::Real(*self.cov_increment.read())),
            _ => self
                .properties
                .get(property_id)
                .ok_or(PropertyError::NotFound(property_id)),
        }
    }

    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier | PropertyId::ObjectType => {
                Err(PropertyError::ReadOnly(property_id))
            }
            PropertyId::PresentValue => {
                if let Some(v) = value.as_real() {
                    let old = *self.present_value.read();
                    *self.present_value.write() = v;
                    if (v - old).abs() >= *self.cov_increment.read() {
                        self.cov_changed.store(true, Ordering::Release);
                    }
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::OutOfService => {
                if let Some(v) = value.as_bool() {
                    self.out_of_service.store(v, Ordering::Release);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            _ => {
                self.properties.set(property_id, value);
                Ok(())
            }
        }
    }

    fn list_properties(&self) -> Vec<PropertyId> {
        vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::PresentValue,
            PropertyId::StatusFlags,
            PropertyId::EventState,
            PropertyId::OutOfService,
            PropertyId::Units,
            PropertyId::CovIncrement,
        ]
    }

    fn status_flags(&self) -> StatusFlags {
        StatusFlags {
            in_alarm: false,
            fault: false,
            overridden: false,
            out_of_service: self.out_of_service.load(Ordering::Acquire),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl WritableObject for AnalogValue {
    fn set_present_value(&self, value: BACnetValue) -> Result<(), PropertyError> {
        self.write_property(PropertyId::PresentValue, value)
    }
}

impl CovSupport for AnalogValue {
    fn cov_increment(&self) -> Option<f32> {
        Some(*self.cov_increment.read())
    }

    fn check_cov(&self) -> bool {
        self.cov_changed.load(Ordering::Acquire)
    }

    fn reset_cov(&self) {
        *self.last_cov_value.write() = *self.present_value.read();
        self.cov_changed.store(false, Ordering::Release);
    }
}

// ============================================================================
// Binary Input
// ============================================================================

/// Binary Input object (read-only binary sensor).
pub struct BinaryInput {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,
    present_value: AtomicBool,
    out_of_service: AtomicBool,
    last_cov_value: AtomicBool,
    cov_changed: AtomicBool,
}

impl BinaryInput {
    /// Create a new Binary Input.
    pub fn new(instance: u32, name: impl Into<String>) -> Self {
        let id = ObjectId::new(ObjectType::BinaryInput, instance);
        let properties = PropertyStore::new();

        properties.set(
            PropertyId::Polarity,
            BACnetValue::Enumerated(Polarity::Normal as u32),
        );
        properties.set(
            PropertyId::EventState,
            BACnetValue::Enumerated(EventState::Normal as u32),
        );
        properties.set(
            PropertyId::ActiveText,
            BACnetValue::CharacterString("Active".to_string()),
        );
        properties.set(
            PropertyId::InactiveText,
            BACnetValue::CharacterString("Inactive".to_string()),
        );

        Self {
            id,
            name: name.into(),
            description: String::new(),
            properties,
            present_value: AtomicBool::new(false),
            out_of_service: AtomicBool::new(false),
            last_cov_value: AtomicBool::new(false),
            cov_changed: AtomicBool::new(false),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Set the present value (for simulation).
    pub fn set_value(&self, value: bool) {
        let old = self.present_value.swap(value, Ordering::AcqRel);
        if old != value {
            self.cov_changed.store(true, Ordering::Release);
        }
    }

    /// Get the present value.
    pub fn get_value(&self) -> bool {
        self.present_value.load(Ordering::Acquire)
    }
}

impl BACnetObject for BinaryInput {
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
            PropertyId::ObjectIdentifier => Ok(BACnetValue::ObjectIdentifier(self.id)),
            PropertyId::ObjectName => Ok(BACnetValue::CharacterString(self.name.clone())),
            PropertyId::ObjectType => {
                Ok(BACnetValue::Enumerated(ObjectType::BinaryInput as u32))
            }
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::PresentValue => {
                // Binary PV is enumerated: 0=inactive, 1=active
                let value = if self.present_value.load(Ordering::Acquire) {
                    1
                } else {
                    0
                };
                Ok(BACnetValue::Enumerated(value))
            }
            PropertyId::StatusFlags => Ok(BACnetValue::BitString(self.status_flags().to_bits())),
            PropertyId::OutOfService => {
                Ok(BACnetValue::Boolean(self.out_of_service.load(Ordering::Acquire)))
            }
            _ => self
                .properties
                .get(property_id)
                .ok_or(PropertyError::NotFound(property_id)),
        }
    }

    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier
            | PropertyId::ObjectType
            | PropertyId::PresentValue => Err(PropertyError::ReadOnly(property_id)),
            PropertyId::OutOfService => {
                if let Some(v) = value.as_bool() {
                    self.out_of_service.store(v, Ordering::Release);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            _ => {
                self.properties.set(property_id, value);
                Ok(())
            }
        }
    }

    fn list_properties(&self) -> Vec<PropertyId> {
        vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::PresentValue,
            PropertyId::StatusFlags,
            PropertyId::EventState,
            PropertyId::OutOfService,
            PropertyId::Polarity,
            PropertyId::ActiveText,
            PropertyId::InactiveText,
        ]
    }

    fn status_flags(&self) -> StatusFlags {
        StatusFlags {
            in_alarm: false,
            fault: false,
            overridden: false,
            out_of_service: self.out_of_service.load(Ordering::Acquire),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl CovSupport for BinaryInput {
    fn check_cov(&self) -> bool {
        self.cov_changed.load(Ordering::Acquire)
    }

    fn reset_cov(&self) {
        self.last_cov_value
            .store(self.present_value.load(Ordering::Acquire), Ordering::Release);
        self.cov_changed.store(false, Ordering::Release);
    }
}

// ============================================================================
// Binary Output
// ============================================================================

/// Binary Output object (writable binary control).
pub struct BinaryOutput {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,
    present_value: AtomicBool,
    relinquish_default: AtomicBool,
    priority_array: RwLock<[Option<bool>; 16]>,
    out_of_service: AtomicBool,
    last_cov_value: AtomicBool,
    cov_changed: AtomicBool,
}

impl BinaryOutput {
    /// Create a new Binary Output.
    pub fn new(instance: u32, name: impl Into<String>) -> Self {
        let id = ObjectId::new(ObjectType::BinaryOutput, instance);
        let properties = PropertyStore::new();

        properties.set(
            PropertyId::Polarity,
            BACnetValue::Enumerated(Polarity::Normal as u32),
        );
        properties.set(
            PropertyId::EventState,
            BACnetValue::Enumerated(EventState::Normal as u32),
        );
        properties.set(
            PropertyId::ActiveText,
            BACnetValue::CharacterString("On".to_string()),
        );
        properties.set(
            PropertyId::InactiveText,
            BACnetValue::CharacterString("Off".to_string()),
        );

        Self {
            id,
            name: name.into(),
            description: String::new(),
            properties,
            present_value: AtomicBool::new(false),
            relinquish_default: AtomicBool::new(false),
            priority_array: RwLock::new([None; 16]),
            out_of_service: AtomicBool::new(false),
            last_cov_value: AtomicBool::new(false),
            cov_changed: AtomicBool::new(false),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    fn update_present_value(&self) {
        let pa = self.priority_array.read();
        let new_value = pa
            .iter()
            .find_map(|v| *v)
            .unwrap_or(self.relinquish_default.load(Ordering::Acquire));

        let old = self.present_value.swap(new_value, Ordering::AcqRel);
        if old != new_value {
            self.cov_changed.store(true, Ordering::Release);
        }
    }
}

impl BACnetObject for BinaryOutput {
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
            PropertyId::ObjectIdentifier => Ok(BACnetValue::ObjectIdentifier(self.id)),
            PropertyId::ObjectName => Ok(BACnetValue::CharacterString(self.name.clone())),
            PropertyId::ObjectType => {
                Ok(BACnetValue::Enumerated(ObjectType::BinaryOutput as u32))
            }
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::PresentValue => {
                let value = if self.present_value.load(Ordering::Acquire) {
                    1
                } else {
                    0
                };
                Ok(BACnetValue::Enumerated(value))
            }
            PropertyId::StatusFlags => Ok(BACnetValue::BitString(self.status_flags().to_bits())),
            PropertyId::OutOfService => {
                Ok(BACnetValue::Boolean(self.out_of_service.load(Ordering::Acquire)))
            }
            PropertyId::RelinquishDefault => {
                let value = if self.relinquish_default.load(Ordering::Acquire) {
                    1
                } else {
                    0
                };
                Ok(BACnetValue::Enumerated(value))
            }
            PropertyId::PriorityArray => {
                let pa = self.priority_array.read();
                let values: Vec<BACnetValue> = pa
                    .iter()
                    .map(|v| match v {
                        Some(b) => BACnetValue::Enumerated(if *b { 1 } else { 0 }),
                        None => BACnetValue::Null,
                    })
                    .collect();
                Ok(BACnetValue::Array(values))
            }
            _ => self
                .properties
                .get(property_id)
                .ok_or(PropertyError::NotFound(property_id)),
        }
    }

    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier | PropertyId::ObjectType => {
                Err(PropertyError::ReadOnly(property_id))
            }
            PropertyId::PresentValue => {
                let bool_value = match &value {
                    BACnetValue::Boolean(b) => *b,
                    BACnetValue::Enumerated(e) => *e != 0,
                    _ => return Err(PropertyError::InvalidDataType(property_id)),
                };

                let mut pa = self.priority_array.write();
                pa[15] = Some(bool_value);
                drop(pa);
                self.update_present_value();
                Ok(())
            }
            PropertyId::OutOfService => {
                if let Some(v) = value.as_bool() {
                    self.out_of_service.store(v, Ordering::Release);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            _ => {
                self.properties.set(property_id, value);
                Ok(())
            }
        }
    }

    fn list_properties(&self) -> Vec<PropertyId> {
        vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::PresentValue,
            PropertyId::StatusFlags,
            PropertyId::EventState,
            PropertyId::OutOfService,
            PropertyId::Polarity,
            PropertyId::PriorityArray,
            PropertyId::RelinquishDefault,
            PropertyId::ActiveText,
            PropertyId::InactiveText,
        ]
    }

    fn status_flags(&self) -> StatusFlags {
        StatusFlags {
            in_alarm: false,
            fault: false,
            overridden: false,
            out_of_service: self.out_of_service.load(Ordering::Acquire),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl WritableObject for BinaryOutput {
    fn set_present_value(&self, value: BACnetValue) -> Result<(), PropertyError> {
        self.write_property(PropertyId::PresentValue, value)
    }
}

impl CovSupport for BinaryOutput {
    fn check_cov(&self) -> bool {
        self.cov_changed.load(Ordering::Acquire)
    }

    fn reset_cov(&self) {
        self.last_cov_value
            .store(self.present_value.load(Ordering::Acquire), Ordering::Release);
        self.cov_changed.store(false, Ordering::Release);
    }
}

// ============================================================================
// Binary Value
// ============================================================================

/// Binary Value object (writable binary point).
pub struct BinaryValue {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,
    present_value: AtomicBool,
    out_of_service: AtomicBool,
    last_cov_value: AtomicBool,
    cov_changed: AtomicBool,
}

impl BinaryValue {
    /// Create a new Binary Value.
    pub fn new(instance: u32, name: impl Into<String>) -> Self {
        let id = ObjectId::new(ObjectType::BinaryValue, instance);
        let properties = PropertyStore::new();

        properties.set(
            PropertyId::EventState,
            BACnetValue::Enumerated(EventState::Normal as u32),
        );
        properties.set(
            PropertyId::ActiveText,
            BACnetValue::CharacterString("Active".to_string()),
        );
        properties.set(
            PropertyId::InactiveText,
            BACnetValue::CharacterString("Inactive".to_string()),
        );

        Self {
            id,
            name: name.into(),
            description: String::new(),
            properties,
            present_value: AtomicBool::new(false),
            out_of_service: AtomicBool::new(false),
            last_cov_value: AtomicBool::new(false),
            cov_changed: AtomicBool::new(false),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_initial_value(self, value: bool) -> Self {
        self.present_value.store(value, Ordering::Release);
        self.last_cov_value.store(value, Ordering::Release);
        self
    }
}

impl BACnetObject for BinaryValue {
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
            PropertyId::ObjectIdentifier => Ok(BACnetValue::ObjectIdentifier(self.id)),
            PropertyId::ObjectName => Ok(BACnetValue::CharacterString(self.name.clone())),
            PropertyId::ObjectType => {
                Ok(BACnetValue::Enumerated(ObjectType::BinaryValue as u32))
            }
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::PresentValue => {
                let value = if self.present_value.load(Ordering::Acquire) {
                    1
                } else {
                    0
                };
                Ok(BACnetValue::Enumerated(value))
            }
            PropertyId::StatusFlags => Ok(BACnetValue::BitString(self.status_flags().to_bits())),
            PropertyId::OutOfService => {
                Ok(BACnetValue::Boolean(self.out_of_service.load(Ordering::Acquire)))
            }
            _ => self
                .properties
                .get(property_id)
                .ok_or(PropertyError::NotFound(property_id)),
        }
    }

    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier | PropertyId::ObjectType => {
                Err(PropertyError::ReadOnly(property_id))
            }
            PropertyId::PresentValue => {
                let bool_value = match &value {
                    BACnetValue::Boolean(b) => *b,
                    BACnetValue::Enumerated(e) => *e != 0,
                    _ => return Err(PropertyError::InvalidDataType(property_id)),
                };

                let old = self.present_value.swap(bool_value, Ordering::AcqRel);
                if old != bool_value {
                    self.cov_changed.store(true, Ordering::Release);
                }
                Ok(())
            }
            PropertyId::OutOfService => {
                if let Some(v) = value.as_bool() {
                    self.out_of_service.store(v, Ordering::Release);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            _ => {
                self.properties.set(property_id, value);
                Ok(())
            }
        }
    }

    fn list_properties(&self) -> Vec<PropertyId> {
        vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::PresentValue,
            PropertyId::StatusFlags,
            PropertyId::EventState,
            PropertyId::OutOfService,
            PropertyId::ActiveText,
            PropertyId::InactiveText,
        ]
    }

    fn status_flags(&self) -> StatusFlags {
        StatusFlags {
            in_alarm: false,
            fault: false,
            overridden: false,
            out_of_service: self.out_of_service.load(Ordering::Acquire),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl WritableObject for BinaryValue {
    fn set_present_value(&self, value: BACnetValue) -> Result<(), PropertyError> {
        self.write_property(PropertyId::PresentValue, value)
    }
}

impl CovSupport for BinaryValue {
    fn check_cov(&self) -> bool {
        self.cov_changed.load(Ordering::Acquire)
    }

    fn reset_cov(&self) {
        self.last_cov_value
            .store(self.present_value.load(Ordering::Acquire), Ordering::Release);
        self.cov_changed.store(false, Ordering::Release);
    }
}

// ============================================================================
// Multi-state Input
// ============================================================================

/// Multi-state Input object (read-only multi-state sensor).
pub struct MultiStateInput {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,
    present_value: RwLock<u32>,
    number_of_states: u32,
    out_of_service: AtomicBool,
    last_cov_value: RwLock<u32>,
    cov_changed: AtomicBool,
}

impl MultiStateInput {
    /// Create a new Multi-state Input.
    pub fn new(instance: u32, name: impl Into<String>, number_of_states: u32) -> Self {
        let id = ObjectId::new(ObjectType::MultiStateInput, instance);
        let properties = PropertyStore::new();

        properties.set(
            PropertyId::NumberOfStates,
            BACnetValue::Unsigned(number_of_states),
        );
        properties.set(
            PropertyId::EventState,
            BACnetValue::Enumerated(EventState::Normal as u32),
        );

        Self {
            id,
            name: name.into(),
            description: String::new(),
            properties,
            present_value: RwLock::new(1), // States are 1-indexed
            number_of_states,
            out_of_service: AtomicBool::new(false),
            last_cov_value: RwLock::new(1),
            cov_changed: AtomicBool::new(false),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_state_texts(self, texts: Vec<String>) -> Self {
        let values: Vec<BACnetValue> = texts
            .into_iter()
            .map(BACnetValue::CharacterString)
            .collect();
        self.properties
            .set(PropertyId::StateText, BACnetValue::Array(values));
        self
    }

    /// Set the present value (for simulation).
    pub fn set_value(&self, value: u32) {
        if value >= 1 && value <= self.number_of_states {
            let old = *self.present_value.read();
            *self.present_value.write() = value;
            if old != value {
                self.cov_changed.store(true, Ordering::Release);
            }
        }
    }

    /// Get the present value.
    pub fn get_value(&self) -> u32 {
        *self.present_value.read()
    }
}

impl BACnetObject for MultiStateInput {
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
            PropertyId::ObjectIdentifier => Ok(BACnetValue::ObjectIdentifier(self.id)),
            PropertyId::ObjectName => Ok(BACnetValue::CharacterString(self.name.clone())),
            PropertyId::ObjectType => {
                Ok(BACnetValue::Enumerated(ObjectType::MultiStateInput as u32))
            }
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::PresentValue => Ok(BACnetValue::Unsigned(*self.present_value.read())),
            PropertyId::StatusFlags => Ok(BACnetValue::BitString(self.status_flags().to_bits())),
            PropertyId::OutOfService => {
                Ok(BACnetValue::Boolean(self.out_of_service.load(Ordering::Acquire)))
            }
            PropertyId::NumberOfStates => Ok(BACnetValue::Unsigned(self.number_of_states)),
            _ => self
                .properties
                .get(property_id)
                .ok_or(PropertyError::NotFound(property_id)),
        }
    }

    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier
            | PropertyId::ObjectType
            | PropertyId::PresentValue
            | PropertyId::NumberOfStates => Err(PropertyError::ReadOnly(property_id)),
            PropertyId::OutOfService => {
                if let Some(v) = value.as_bool() {
                    self.out_of_service.store(v, Ordering::Release);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            _ => {
                self.properties.set(property_id, value);
                Ok(())
            }
        }
    }

    fn list_properties(&self) -> Vec<PropertyId> {
        vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::PresentValue,
            PropertyId::StatusFlags,
            PropertyId::EventState,
            PropertyId::OutOfService,
            PropertyId::NumberOfStates,
            PropertyId::StateText,
        ]
    }

    fn status_flags(&self) -> StatusFlags {
        StatusFlags {
            in_alarm: false,
            fault: false,
            overridden: false,
            out_of_service: self.out_of_service.load(Ordering::Acquire),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl CovSupport for MultiStateInput {
    fn check_cov(&self) -> bool {
        self.cov_changed.load(Ordering::Acquire)
    }

    fn reset_cov(&self) {
        *self.last_cov_value.write() = *self.present_value.read();
        self.cov_changed.store(false, Ordering::Release);
    }
}

// ============================================================================
// Multi-state Output
// ============================================================================

/// Multi-state Output object (writable multi-state control).
pub struct MultiStateOutput {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,
    present_value: RwLock<u32>,
    relinquish_default: RwLock<u32>,
    priority_array: RwLock<[Option<u32>; 16]>,
    number_of_states: u32,
    out_of_service: AtomicBool,
    last_cov_value: RwLock<u32>,
    cov_changed: AtomicBool,
}

impl MultiStateOutput {
    /// Create a new Multi-state Output.
    pub fn new(instance: u32, name: impl Into<String>, number_of_states: u32) -> Self {
        let id = ObjectId::new(ObjectType::MultiStateOutput, instance);
        let properties = PropertyStore::new();

        properties.set(
            PropertyId::NumberOfStates,
            BACnetValue::Unsigned(number_of_states),
        );
        properties.set(
            PropertyId::EventState,
            BACnetValue::Enumerated(EventState::Normal as u32),
        );

        Self {
            id,
            name: name.into(),
            description: String::new(),
            properties,
            present_value: RwLock::new(1),
            relinquish_default: RwLock::new(1),
            priority_array: RwLock::new([None; 16]),
            number_of_states,
            out_of_service: AtomicBool::new(false),
            last_cov_value: RwLock::new(1),
            cov_changed: AtomicBool::new(false),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    fn update_present_value(&self) {
        let pa = self.priority_array.read();
        let new_value = pa
            .iter()
            .find_map(|v| *v)
            .unwrap_or(*self.relinquish_default.read());

        let old = *self.present_value.read();
        *self.present_value.write() = new_value;
        if old != new_value {
            self.cov_changed.store(true, Ordering::Release);
        }
    }
}

impl BACnetObject for MultiStateOutput {
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
            PropertyId::ObjectIdentifier => Ok(BACnetValue::ObjectIdentifier(self.id)),
            PropertyId::ObjectName => Ok(BACnetValue::CharacterString(self.name.clone())),
            PropertyId::ObjectType => {
                Ok(BACnetValue::Enumerated(ObjectType::MultiStateOutput as u32))
            }
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::PresentValue => Ok(BACnetValue::Unsigned(*self.present_value.read())),
            PropertyId::StatusFlags => Ok(BACnetValue::BitString(self.status_flags().to_bits())),
            PropertyId::OutOfService => {
                Ok(BACnetValue::Boolean(self.out_of_service.load(Ordering::Acquire)))
            }
            PropertyId::NumberOfStates => Ok(BACnetValue::Unsigned(self.number_of_states)),
            PropertyId::RelinquishDefault => {
                Ok(BACnetValue::Unsigned(*self.relinquish_default.read()))
            }
            PropertyId::PriorityArray => {
                let pa = self.priority_array.read();
                let values: Vec<BACnetValue> = pa
                    .iter()
                    .map(|v| match v {
                        Some(u) => BACnetValue::Unsigned(*u),
                        None => BACnetValue::Null,
                    })
                    .collect();
                Ok(BACnetValue::Array(values))
            }
            _ => self
                .properties
                .get(property_id)
                .ok_or(PropertyError::NotFound(property_id)),
        }
    }

    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier | PropertyId::ObjectType | PropertyId::NumberOfStates => {
                Err(PropertyError::ReadOnly(property_id))
            }
            PropertyId::PresentValue => {
                if let Some(v) = value.as_unsigned() {
                    if v >= 1 && v <= self.number_of_states {
                        let mut pa = self.priority_array.write();
                        pa[15] = Some(v);
                        drop(pa);
                        self.update_present_value();
                        Ok(())
                    } else {
                        Err(PropertyError::ValueOutOfRange(property_id))
                    }
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::OutOfService => {
                if let Some(v) = value.as_bool() {
                    self.out_of_service.store(v, Ordering::Release);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            _ => {
                self.properties.set(property_id, value);
                Ok(())
            }
        }
    }

    fn list_properties(&self) -> Vec<PropertyId> {
        vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::PresentValue,
            PropertyId::StatusFlags,
            PropertyId::EventState,
            PropertyId::OutOfService,
            PropertyId::NumberOfStates,
            PropertyId::StateText,
            PropertyId::PriorityArray,
            PropertyId::RelinquishDefault,
        ]
    }

    fn status_flags(&self) -> StatusFlags {
        StatusFlags {
            in_alarm: false,
            fault: false,
            overridden: false,
            out_of_service: self.out_of_service.load(Ordering::Acquire),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl WritableObject for MultiStateOutput {
    fn set_present_value(&self, value: BACnetValue) -> Result<(), PropertyError> {
        self.write_property(PropertyId::PresentValue, value)
    }
}

impl CovSupport for MultiStateOutput {
    fn check_cov(&self) -> bool {
        self.cov_changed.load(Ordering::Acquire)
    }

    fn reset_cov(&self) {
        *self.last_cov_value.write() = *self.present_value.read();
        self.cov_changed.store(false, Ordering::Release);
    }
}

// ============================================================================
// Multi-state Value
// ============================================================================

/// Multi-state Value object (writable multi-state point).
pub struct MultiStateValue {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,
    present_value: RwLock<u32>,
    number_of_states: u32,
    out_of_service: AtomicBool,
    last_cov_value: RwLock<u32>,
    cov_changed: AtomicBool,
}

impl MultiStateValue {
    /// Create a new Multi-state Value.
    pub fn new(instance: u32, name: impl Into<String>, number_of_states: u32) -> Self {
        let id = ObjectId::new(ObjectType::MultiStateValue, instance);
        let properties = PropertyStore::new();

        properties.set(
            PropertyId::NumberOfStates,
            BACnetValue::Unsigned(number_of_states),
        );
        properties.set(
            PropertyId::EventState,
            BACnetValue::Enumerated(EventState::Normal as u32),
        );

        Self {
            id,
            name: name.into(),
            description: String::new(),
            properties,
            present_value: RwLock::new(1),
            number_of_states,
            out_of_service: AtomicBool::new(false),
            last_cov_value: RwLock::new(1),
            cov_changed: AtomicBool::new(false),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_initial_value(self, value: u32) -> Self {
        if value >= 1 && value <= self.number_of_states {
            *self.present_value.write() = value;
            *self.last_cov_value.write() = value;
        }
        self
    }
}

impl BACnetObject for MultiStateValue {
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
            PropertyId::ObjectIdentifier => Ok(BACnetValue::ObjectIdentifier(self.id)),
            PropertyId::ObjectName => Ok(BACnetValue::CharacterString(self.name.clone())),
            PropertyId::ObjectType => {
                Ok(BACnetValue::Enumerated(ObjectType::MultiStateValue as u32))
            }
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::PresentValue => Ok(BACnetValue::Unsigned(*self.present_value.read())),
            PropertyId::StatusFlags => Ok(BACnetValue::BitString(self.status_flags().to_bits())),
            PropertyId::OutOfService => {
                Ok(BACnetValue::Boolean(self.out_of_service.load(Ordering::Acquire)))
            }
            PropertyId::NumberOfStates => Ok(BACnetValue::Unsigned(self.number_of_states)),
            _ => self
                .properties
                .get(property_id)
                .ok_or(PropertyError::NotFound(property_id)),
        }
    }

    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier | PropertyId::ObjectType | PropertyId::NumberOfStates => {
                Err(PropertyError::ReadOnly(property_id))
            }
            PropertyId::PresentValue => {
                if let Some(v) = value.as_unsigned() {
                    if v >= 1 && v <= self.number_of_states {
                        let old = *self.present_value.read();
                        *self.present_value.write() = v;
                        if old != v {
                            self.cov_changed.store(true, Ordering::Release);
                        }
                        Ok(())
                    } else {
                        Err(PropertyError::ValueOutOfRange(property_id))
                    }
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::OutOfService => {
                if let Some(v) = value.as_bool() {
                    self.out_of_service.store(v, Ordering::Release);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            _ => {
                self.properties.set(property_id, value);
                Ok(())
            }
        }
    }

    fn list_properties(&self) -> Vec<PropertyId> {
        vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::PresentValue,
            PropertyId::StatusFlags,
            PropertyId::EventState,
            PropertyId::OutOfService,
            PropertyId::NumberOfStates,
            PropertyId::StateText,
        ]
    }

    fn status_flags(&self) -> StatusFlags {
        StatusFlags {
            in_alarm: false,
            fault: false,
            overridden: false,
            out_of_service: self.out_of_service.load(Ordering::Acquire),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl WritableObject for MultiStateValue {
    fn set_present_value(&self, value: BACnetValue) -> Result<(), PropertyError> {
        self.write_property(PropertyId::PresentValue, value)
    }
}

impl CovSupport for MultiStateValue {
    fn check_cov(&self) -> bool {
        self.cov_changed.load(Ordering::Acquire)
    }

    fn reset_cov(&self) {
        *self.last_cov_value.write() = *self.present_value.read();
        self.cov_changed.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analog_input() {
        let ai = AnalogInput::new(1, "Zone Temperature")
            .with_description("Room temperature sensor")
            .with_units(62); // Degrees Celsius

        assert_eq!(ai.object_type(), ObjectType::AnalogInput);
        assert_eq!(ai.object_name(), "Zone Temperature");

        ai.set_value(25.5);
        assert_eq!(ai.get_value(), 25.5);

        let pv = ai.read_property(PropertyId::PresentValue).unwrap();
        assert_eq!(pv.as_real(), Some(25.5));
    }

    #[test]
    fn test_analog_output_priority() {
        let ao = AnalogOutput::new(1, "Damper Position")
            .with_relinquish_default(50.0);

        // Initial value is relinquish default
        assert_eq!(ao.read_property(PropertyId::PresentValue).unwrap().as_real(), Some(50.0));

        // Write at priority 16
        ao.set_present_value(BACnetValue::Real(75.0)).unwrap();
        assert_eq!(ao.read_property(PropertyId::PresentValue).unwrap().as_real(), Some(75.0));

        // Write at higher priority (8)
        ao.set_present_value_with_priority(BACnetValue::Real(90.0), 8).unwrap();
        assert_eq!(ao.read_property(PropertyId::PresentValue).unwrap().as_real(), Some(90.0));

        // Relinquish priority 8, should go back to priority 16 value
        ao.relinquish(8).unwrap();
        assert_eq!(ao.read_property(PropertyId::PresentValue).unwrap().as_real(), Some(75.0));
    }

    #[test]
    fn test_binary_input() {
        let bi = BinaryInput::new(1, "Occupancy Sensor");

        bi.set_value(true);
        assert!(bi.get_value());

        let pv = bi.read_property(PropertyId::PresentValue).unwrap();
        assert_eq!(pv.as_unsigned(), Some(1)); // active = 1
    }

    #[test]
    fn test_multi_state_value() {
        let msv = MultiStateValue::new(1, "Operating Mode", 4)
            .with_initial_value(2);

        assert_eq!(msv.read_property(PropertyId::PresentValue).unwrap().as_unsigned(), Some(2));

        // Try to set invalid state
        let result = msv.write_property(PropertyId::PresentValue, BACnetValue::Unsigned(5));
        assert!(result.is_err());

        // Set valid state
        msv.write_property(PropertyId::PresentValue, BACnetValue::Unsigned(3)).unwrap();
        assert_eq!(msv.read_property(PropertyId::PresentValue).unwrap().as_unsigned(), Some(3));
    }

    #[test]
    fn test_cov_detection() {
        let ai = AnalogInput::new(1, "Temperature").with_cov_increment(1.0);

        ai.set_value(20.0);
        ai.reset_cov();

        // Small change - no COV
        ai.set_value(20.5);
        assert!(!ai.check_cov());

        // Large change - COV triggered
        ai.set_value(21.5);
        assert!(ai.check_cov());

        ai.reset_cov();
        assert!(!ai.check_cov());
    }
}
