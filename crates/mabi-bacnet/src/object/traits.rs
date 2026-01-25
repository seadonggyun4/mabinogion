//! BACnet object traits for extensible design.
//!
//! This module defines the core traits that all BACnet objects must implement,
//! enabling polymorphic object handling and extensibility.

use std::sync::Arc;

use super::property::{BACnetValue, PropertyError, PropertyId, StatusFlags};
use super::types::{ObjectId, ObjectType};

/// Core trait for all BACnet objects.
///
/// This trait defines the fundamental interface that all BACnet objects
/// must implement, providing access to object identification, properties,
/// and basic operations.
pub trait BACnetObject: Send + Sync {
    /// Get the object identifier.
    fn object_identifier(&self) -> ObjectId;

    /// Get the object name.
    fn object_name(&self) -> &str;

    /// Get the object type.
    fn object_type(&self) -> ObjectType {
        self.object_identifier().object_type
    }

    /// Get the object description.
    fn description(&self) -> Option<&str> {
        None
    }

    /// Read a property value.
    fn read_property(&self, property_id: PropertyId) -> Result<BACnetValue, PropertyError>;

    /// Read a property with optional array index.
    fn read_property_at(
        &self,
        property_id: PropertyId,
        array_index: Option<u32>,
    ) -> Result<BACnetValue, PropertyError> {
        let value = self.read_property(property_id)?;

        if let Some(index) = array_index {
            match value {
                BACnetValue::Array(arr) => arr
                    .get(index as usize)
                    .cloned()
                    .ok_or(PropertyError::InvalidArrayIndex(index)),
                _ => Err(PropertyError::InvalidArrayIndex(index)),
            }
        } else {
            Ok(value)
        }
    }

    /// Write a property value.
    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError>;

    /// Write a property with optional array index and priority.
    fn write_property_at(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
        _array_index: Option<u32>,
        _priority: Option<u8>,
    ) -> Result<(), PropertyError> {
        // Default implementation ignores array index and priority
        self.write_property(property_id, value)
    }

    /// List all supported properties.
    fn list_properties(&self) -> Vec<PropertyId>;

    /// Check if a property is supported.
    fn has_property(&self, property_id: PropertyId) -> bool {
        self.list_properties().contains(&property_id)
    }

    /// Get the current status flags.
    fn status_flags(&self) -> StatusFlags {
        StatusFlags::default()
    }

    /// Check if the object is out of service.
    fn is_out_of_service(&self) -> bool {
        self.status_flags().out_of_service
    }

    /// Get the present value as a generic BACnetValue.
    fn present_value(&self) -> Result<BACnetValue, PropertyError> {
        self.read_property(PropertyId::PresentValue)
    }
}

/// Trait for objects that support writing (outputs and values).
pub trait WritableObject: BACnetObject {
    /// Set the present value.
    fn set_present_value(&self, value: BACnetValue) -> Result<(), PropertyError>;

    /// Set the present value with priority (for commandable objects).
    fn set_present_value_with_priority(
        &self,
        value: BACnetValue,
        _priority: u8,
    ) -> Result<(), PropertyError> {
        // Default implementation ignores priority
        self.set_present_value(value)
    }

    /// Get the relinquish default value.
    fn relinquish_default(&self) -> Option<BACnetValue> {
        None
    }

    /// Get the priority array (for commandable objects).
    fn priority_array(&self) -> Option<[Option<BACnetValue>; 16]> {
        None
    }

    /// Relinquish a priority level.
    fn relinquish(&self, _priority: u8) -> Result<(), PropertyError> {
        Ok(())
    }
}

/// Trait for objects that support COV (Change of Value).
pub trait CovSupport: BACnetObject {
    /// Get the COV increment (for analog types).
    fn cov_increment(&self) -> Option<f32> {
        None
    }

    /// Set the COV increment.
    fn set_cov_increment(&self, _increment: f32) -> Result<(), PropertyError> {
        Err(PropertyError::WriteAccessDenied(PropertyId::CovIncrement))
    }

    /// Check if COV has occurred since last check.
    ///
    /// Returns true if the present value has changed enough to trigger COV notification.
    fn check_cov(&self) -> bool;

    /// Get the values to include in COV notification.
    fn cov_values(&self) -> Vec<(PropertyId, BACnetValue)> {
        let mut values = Vec::new();

        if let Ok(pv) = self.present_value() {
            values.push((PropertyId::PresentValue, pv));
        }

        values.push((
            PropertyId::StatusFlags,
            BACnetValue::BitString(self.status_flags().to_bits()),
        ));

        values
    }

    /// Reset COV tracking (called after notification is sent).
    fn reset_cov(&self);
}

/// Trait for objects with intrinsic reporting (alarms).
pub trait IntrinsicReporting: BACnetObject {
    /// Get the current event state.
    fn event_state(&self) -> super::property::EventState {
        super::property::EventState::Normal
    }

    /// Check for event state transitions.
    fn check_event_state(&self) -> Option<super::property::EventState> {
        None
    }

    /// Get the high limit (for analog alarm detection).
    fn high_limit(&self) -> Option<f32> {
        None
    }

    /// Get the low limit (for analog alarm detection).
    fn low_limit(&self) -> Option<f32> {
        None
    }

    /// Get the deadband (for analog alarm detection).
    fn deadband(&self) -> Option<f32> {
        None
    }
}

/// Builder trait for creating BACnet objects.
pub trait ObjectBuilder<T: BACnetObject> {
    /// Set the object name.
    fn name(self, name: impl Into<String>) -> Self;

    /// Set the description.
    fn description(self, desc: impl Into<String>) -> Self;

    /// Set the out of service flag.
    fn out_of_service(self, oos: bool) -> Self;

    /// Build the object.
    fn build(self) -> T;
}

/// Type alias for a boxed BACnet object.
pub type BoxedObject = Box<dyn BACnetObject>;

/// Type alias for an Arc'd BACnet object.
pub type ArcObject = Arc<dyn BACnetObject>;

#[cfg(test)]
mod tests {
    use super::*;

    // Mock object for testing
    struct MockObject {
        id: ObjectId,
        name: String,
    }

    impl BACnetObject for MockObject {
        fn object_identifier(&self) -> ObjectId {
            self.id
        }

        fn object_name(&self) -> &str {
            &self.name
        }

        fn read_property(&self, property_id: PropertyId) -> Result<BACnetValue, PropertyError> {
            match property_id {
                PropertyId::ObjectIdentifier => {
                    Ok(BACnetValue::ObjectIdentifier(self.id))
                }
                PropertyId::ObjectName => {
                    Ok(BACnetValue::CharacterString(self.name.clone()))
                }
                PropertyId::ObjectType => {
                    Ok(BACnetValue::Enumerated(self.id.object_type as u32))
                }
                PropertyId::PresentValue => {
                    Ok(BACnetValue::Real(0.0))
                }
                _ => Err(PropertyError::NotFound(property_id)),
            }
        }

        fn write_property(
            &self,
            property_id: PropertyId,
            _value: BACnetValue,
        ) -> Result<(), PropertyError> {
            Err(PropertyError::WriteAccessDenied(property_id))
        }

        fn list_properties(&self) -> Vec<PropertyId> {
            vec![
                PropertyId::ObjectIdentifier,
                PropertyId::ObjectName,
                PropertyId::ObjectType,
                PropertyId::PresentValue,
            ]
        }
    }

    #[test]
    fn test_mock_object() {
        let obj = MockObject {
            id: ObjectId::new(ObjectType::AnalogInput, 1),
            name: "Test AI".to_string(),
        };

        assert_eq!(obj.object_type(), ObjectType::AnalogInput);
        assert_eq!(obj.object_name(), "Test AI");
        assert!(obj.has_property(PropertyId::PresentValue));
        assert!(!obj.has_property(PropertyId::HighLimit));
    }
}
