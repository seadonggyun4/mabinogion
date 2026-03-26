//! ReadPropertyMultiple and WritePropertyMultiple service implementations.
//!
//! This module provides handlers for bulk property operations, enabling efficient
//! reading and writing of multiple properties in a single request.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                  PropertyMultipleService                     │
//! │                                                             │
//! │  ┌─────────────────────┐  ┌─────────────────────────────┐  │
//! │  │ ReadPropertyMultiple│  │ WritePropertyMultiple        │  │
//! │  │   Handler           │  │   Handler                    │  │
//! │  └──────────┬──────────┘  └────────────┬────────────────┘  │
//! │             │                          │                   │
//! │             ▼                          ▼                   │
//! │  ┌──────────────────────────────────────────────────────┐  │
//! │  │              Property Access Abstraction              │  │
//! │  │   (PropertyAccessResult, PropertyReference, etc.)    │  │
//! │  └──────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Features
//!
//! - **Batch Operations**: Read/write multiple properties across multiple objects
//! - **Error Isolation**: Individual property errors don't fail the entire request
//! - **Special Properties**: Support for "all", "required", and "optional" property selectors
//! - **Segmentation Ready**: Designed for large response handling

use std::sync::Arc;

use crate::apdu::encoding::{ApduDecoder, ApduEncoder};
use crate::apdu::types::{ConfirmedService, ErrorClass, ErrorCode};
use crate::object::property::{BACnetValue, PropertyError, PropertyId};
use crate::object::types::ObjectId;
use crate::object::ObjectRegistry;

use super::handler::{ConfirmedServiceHandler, ServiceContext, ServiceResult};

// ============================================================================
// Property Reference Types (Reusable Abstractions)
// ============================================================================

/// A reference to a property, possibly with an array index.
#[derive(Debug, Clone)]
pub struct PropertyReference {
    /// The property identifier.
    pub property_id: PropertyId,
    /// Optional array index for array properties.
    pub array_index: Option<u32>,
}

impl PropertyReference {
    /// Create a new property reference.
    pub fn new(property_id: PropertyId) -> Self {
        Self {
            property_id,
            array_index: None,
        }
    }

    /// Create a property reference with an array index.
    pub fn with_index(property_id: PropertyId, index: u32) -> Self {
        Self {
            property_id,
            array_index: Some(index),
        }
    }

    /// Check if this is a special "all" property reference.
    pub fn is_all(&self) -> bool {
        self.property_id == PropertyId::All
    }

    /// Check if this is a special "required" property reference.
    pub fn is_required(&self) -> bool {
        self.property_id == PropertyId::Required
    }

    /// Check if this is a special "optional" property reference.
    pub fn is_optional(&self) -> bool {
        self.property_id == PropertyId::Optional
    }
}

/// Result of accessing a property - either success with value or error.
#[derive(Debug, Clone)]
pub enum PropertyAccessResult {
    /// Successful read with the property value.
    Value(BACnetValue),
    /// Error accessing the property.
    Error {
        error_class: ErrorClass,
        error_code: ErrorCode,
    },
}

impl PropertyAccessResult {
    /// Create a successful result.
    pub fn success(value: BACnetValue) -> Self {
        Self::Value(value)
    }

    /// Create an error result.
    pub fn error(error_class: ErrorClass, error_code: ErrorCode) -> Self {
        Self::Error {
            error_class,
            error_code,
        }
    }

    /// Create from a property error.
    pub fn from_property_error(error: PropertyError) -> Self {
        let (error_class, error_code) = property_error_to_bacnet(error);
        Self::Error {
            error_class,
            error_code,
        }
    }

    /// Check if this is a successful result.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Value(_))
    }
}

/// Result for a single property in a read response.
#[derive(Debug, Clone)]
pub struct ReadAccessResultItem {
    /// The property reference.
    pub property_ref: PropertyReference,
    /// The access result.
    pub result: PropertyAccessResult,
}

/// Result for reading properties from a single object.
#[derive(Debug, Clone)]
pub struct ReadAccessResult {
    /// The object that was accessed.
    pub object_id: ObjectId,
    /// Results for each property.
    pub results: Vec<ReadAccessResultItem>,
}

// ============================================================================
// ReadPropertyMultiple Request/Response
// ============================================================================

/// Specification for which properties to read from an object.
#[derive(Debug, Clone)]
pub struct ReadAccessSpecification {
    /// The object to read from.
    pub object_id: ObjectId,
    /// List of properties to read.
    pub property_references: Vec<PropertyReference>,
}

/// ReadPropertyMultiple request.
#[derive(Debug, Clone)]
pub struct ReadPropertyMultipleRequest {
    /// List of object/property specifications to read.
    pub read_access_specifications: Vec<ReadAccessSpecification>,
}

/// ReadPropertyMultiple service handler.
pub struct ReadPropertyMultipleHandler;

impl ReadPropertyMultipleHandler {
    /// Create a new handler.
    pub fn new() -> Self {
        Self
    }

    /// Decode a ReadPropertyMultiple request.
    pub fn decode_request(data: &[u8]) -> Result<ReadPropertyMultipleRequest, ServiceError> {
        let mut decoder = ApduDecoder::new(data);
        let mut specifications = Vec::new();

        while !decoder.is_empty() {
            // Context tag 0: Object Identifier
            let (tag, is_context, len) = decoder.decode_tag_info()?;
            if !is_context || tag != 0 || len != 4 {
                return Err(ServiceError::InvalidRequest);
            }
            let object_id = decoder.decode_object_identifier()?;

            // Context tag 1: List of property references (opening tag)
            if !decoder.is_opening_tag(1) {
                return Err(ServiceError::InvalidRequest);
            }
            decoder.read_u8()?; // consume opening tag

            let mut property_references = Vec::new();

            // Read property references until closing tag
            while !decoder.is_closing_tag(1) {
                // Context tag 0: Property Identifier
                let (tag, is_context, len) = decoder.decode_tag_info()?;
                if !is_context || tag != 0 {
                    return Err(ServiceError::InvalidRequest);
                }
                let prop_id_value = decoder.decode_unsigned(len)?;
                let property_id = PropertyId::from_u32(prop_id_value)
                    .ok_or(ServiceError::UnknownProperty(prop_id_value))?;

                // Optional context tag 1: Array Index
                let array_index = if !decoder.is_empty()
                    && !decoder.is_closing_tag(1)
                    && decoder.peek() == Some(0x19)
                {
                    let (_, _, len) = decoder.decode_tag_info()?;
                    Some(decoder.decode_unsigned(len)?)
                } else {
                    None
                };

                property_references.push(PropertyReference {
                    property_id,
                    array_index,
                });
            }

            decoder.read_u8()?; // consume closing tag

            specifications.push(ReadAccessSpecification {
                object_id,
                property_references,
            });
        }

        Ok(ReadPropertyMultipleRequest {
            read_access_specifications: specifications,
        })
    }

    /// Execute the read operation and return results.
    pub fn execute(
        request: &ReadPropertyMultipleRequest,
        objects: &ObjectRegistry,
    ) -> Vec<ReadAccessResult> {
        let mut results = Vec::with_capacity(request.read_access_specifications.len());

        for spec in &request.read_access_specifications {
            let access_result = Self::read_object_properties(spec, objects);
            results.push(access_result);
        }

        results
    }

    /// Read properties from a single object.
    fn read_object_properties(
        spec: &ReadAccessSpecification,
        objects: &ObjectRegistry,
    ) -> ReadAccessResult {
        let object = match objects.get(&spec.object_id) {
            Some(obj) => obj,
            None => {
                // Object not found - return error for all properties
                let results = spec
                    .property_references
                    .iter()
                    .map(|prop_ref| ReadAccessResultItem {
                        property_ref: prop_ref.clone(),
                        result: PropertyAccessResult::error(
                            ErrorClass::Object,
                            ErrorCode::UnknownObject,
                        ),
                    })
                    .collect();

                return ReadAccessResult {
                    object_id: spec.object_id,
                    results,
                };
            }
        };

        let mut results = Vec::with_capacity(spec.property_references.len());

        for prop_ref in &spec.property_references {
            let result = if prop_ref.is_all() {
                // Handle "all" property - read all properties
                Self::read_all_properties(&object, objects)
            } else if prop_ref.is_required() {
                // Handle "required" property - read required properties
                Self::read_required_properties(&object, objects)
            } else if prop_ref.is_optional() {
                // Handle "optional" property - read optional properties
                Self::read_optional_properties(&object, objects)
            } else {
                // Normal property read
                vec![Self::read_single_property(&object, prop_ref)]
            };

            results.extend(result);
        }

        ReadAccessResult {
            object_id: spec.object_id,
            results,
        }
    }

    /// Read a single property from an object.
    fn read_single_property(
        object: &Arc<dyn crate::object::traits::BACnetObject>,
        prop_ref: &PropertyReference,
    ) -> ReadAccessResultItem {
        let result = match object.read_property_at(prop_ref.property_id, prop_ref.array_index) {
            Ok(value) => PropertyAccessResult::success(value),
            Err(e) => PropertyAccessResult::from_property_error(e),
        };

        ReadAccessResultItem {
            property_ref: prop_ref.clone(),
            result,
        }
    }

    /// Read all properties from an object.
    fn read_all_properties(
        object: &Arc<dyn crate::object::traits::BACnetObject>,
        _objects: &ObjectRegistry,
    ) -> Vec<ReadAccessResultItem> {
        object
            .list_properties()
            .into_iter()
            .map(|prop_id| {
                let prop_ref = PropertyReference::new(prop_id);
                Self::read_single_property(object, &prop_ref)
            })
            .collect()
    }

    /// Read required properties from an object.
    fn read_required_properties(
        object: &Arc<dyn crate::object::traits::BACnetObject>,
        _objects: &ObjectRegistry,
    ) -> Vec<ReadAccessResultItem> {
        // Required properties for all objects
        let required = [
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
        ];

        required
            .iter()
            .filter(|&&prop_id| object.has_property(prop_id))
            .map(|&prop_id| {
                let prop_ref = PropertyReference::new(prop_id);
                Self::read_single_property(object, &prop_ref)
            })
            .collect()
    }

    /// Read optional properties from an object.
    fn read_optional_properties(
        object: &Arc<dyn crate::object::traits::BACnetObject>,
        _objects: &ObjectRegistry,
    ) -> Vec<ReadAccessResultItem> {
        let required = [
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
        ];

        object
            .list_properties()
            .into_iter()
            .filter(|prop_id| !required.contains(prop_id))
            .map(|prop_id| {
                let prop_ref = PropertyReference::new(prop_id);
                Self::read_single_property(object, &prop_ref)
            })
            .collect()
    }

    /// Encode the response.
    pub fn encode_response(results: &[ReadAccessResult]) -> Vec<u8> {
        let mut encoder = ApduEncoder::new();

        for result in results {
            // Context tag 0: Object Identifier
            encoder.encode_context_object_identifier(0, result.object_id);

            // Context tag 1: List of results (opening tag)
            encoder.encode_opening_tag(1);

            for item in &result.results {
                // Context tag 2: Property Identifier
                encoder.encode_context_enumerated(2, item.property_ref.property_id as u32);

                // Optional context tag 3: Array Index
                if let Some(index) = item.property_ref.array_index {
                    encoder.encode_context_unsigned(3, index);
                }

                match &item.result {
                    PropertyAccessResult::Value(value) => {
                        // Context tag 4: Property Value (opening/closing)
                        encoder.encode_opening_tag(4);
                        encoder.encode_value(value);
                        encoder.encode_closing_tag(4);
                    }
                    PropertyAccessResult::Error {
                        error_class,
                        error_code,
                    } => {
                        // Context tag 5: Property Access Error
                        encoder.encode_opening_tag(5);
                        encoder.encode_enumerated(*error_class as u32);
                        encoder.encode_enumerated(*error_code as u32);
                        encoder.encode_closing_tag(5);
                    }
                }
            }

            encoder.encode_closing_tag(1);
        }

        encoder.into_bytes()
    }
}

impl Default for ReadPropertyMultipleHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfirmedServiceHandler for ReadPropertyMultipleHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::ReadPropertyMultiple
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        match Self::decode_request(data) {
            Ok(request) => {
                let results = Self::execute(&request, &ctx.objects);
                let response = Self::encode_response(&results);
                ServiceResult::ComplexAck(response)
            }
            Err(e) => ServiceResult::Error {
                error_class: ErrorClass::Services,
                error_code: match e {
                    ServiceError::InvalidRequest => ErrorCode::InvalidParameterDataType,
                    ServiceError::UnknownProperty(_) => ErrorCode::UnknownProperty,
                    _ => ErrorCode::Other,
                },
            },
        }
    }

    fn name(&self) -> &'static str {
        "ReadPropertyMultiple"
    }

    fn min_data_length(&self) -> usize {
        7 // Minimum: one object-id (4) + opening tag (1) + property-id (1-3)
    }
}

// ============================================================================
// WritePropertyMultiple Request/Response
// ============================================================================

/// A value to write to a property.
#[derive(Debug, Clone)]
pub struct PropertyValueWrite {
    /// The property reference.
    pub property_ref: PropertyReference,
    /// The value to write.
    pub value: BACnetValue,
    /// Optional write priority (1-16).
    pub priority: Option<u8>,
}

/// Specification for which properties to write to an object.
#[derive(Debug, Clone)]
pub struct WriteAccessSpecification {
    /// The object to write to.
    pub object_id: ObjectId,
    /// List of property values to write.
    pub property_values: Vec<PropertyValueWrite>,
}

/// WritePropertyMultiple request.
#[derive(Debug, Clone)]
pub struct WritePropertyMultipleRequest {
    /// List of object/property write specifications.
    pub write_access_specifications: Vec<WriteAccessSpecification>,
}

/// Result of a single property write operation.
#[derive(Debug, Clone)]
pub struct WriteAccessResultItem {
    /// The property reference.
    pub property_ref: PropertyReference,
    /// Error if the write failed, None if successful.
    pub error: Option<(ErrorClass, ErrorCode)>,
}

/// Result for writing properties to a single object.
#[derive(Debug, Clone)]
pub struct WriteAccessResult {
    /// The object that was written to.
    pub object_id: ObjectId,
    /// Results for each property (only errors are recorded).
    pub errors: Vec<WriteAccessResultItem>,
}

/// WritePropertyMultiple service handler.
pub struct WritePropertyMultipleHandler;

impl WritePropertyMultipleHandler {
    /// Create a new handler.
    pub fn new() -> Self {
        Self
    }

    /// Decode a WritePropertyMultiple request.
    pub fn decode_request(data: &[u8]) -> Result<WritePropertyMultipleRequest, ServiceError> {
        let mut decoder = ApduDecoder::new(data);
        let mut specifications = Vec::new();

        while !decoder.is_empty() {
            // Context tag 0: Object Identifier
            let (tag, is_context, len) = decoder.decode_tag_info()?;
            if !is_context || tag != 0 || len != 4 {
                return Err(ServiceError::InvalidRequest);
            }
            let object_id = decoder.decode_object_identifier()?;

            // Context tag 1: List of property values (opening tag)
            if !decoder.is_opening_tag(1) {
                return Err(ServiceError::InvalidRequest);
            }
            decoder.read_u8()?; // consume opening tag

            let mut property_values = Vec::new();

            // Read property values until closing tag
            while !decoder.is_closing_tag(1) {
                // Context tag 0: Property Identifier
                let (tag, is_context, len) = decoder.decode_tag_info()?;
                if !is_context || tag != 0 {
                    return Err(ServiceError::InvalidRequest);
                }
                let prop_id_value = decoder.decode_unsigned(len)?;
                let property_id = PropertyId::from_u32(prop_id_value)
                    .ok_or(ServiceError::UnknownProperty(prop_id_value))?;

                // Optional context tag 1: Array Index
                let array_index = if !decoder.is_empty() && decoder.peek() == Some(0x19) {
                    let (_, _, len) = decoder.decode_tag_info()?;
                    Some(decoder.decode_unsigned(len)?)
                } else {
                    None
                };

                // Context tag 2: Property Value (opening tag)
                let value = decode_property_value_from_context(&mut decoder, 2)?;

                // Optional context tag 3: Priority
                let priority = if !decoder.is_empty() && decoder.peek() == Some(0x39) {
                    let (_, _, len) = decoder.decode_tag_info()?;
                    Some(decoder.decode_unsigned(len)? as u8)
                } else {
                    None
                };

                property_values.push(PropertyValueWrite {
                    property_ref: PropertyReference {
                        property_id,
                        array_index,
                    },
                    value,
                    priority,
                });
            }

            decoder.read_u8()?; // consume closing tag

            specifications.push(WriteAccessSpecification {
                object_id,
                property_values,
            });
        }

        Ok(WritePropertyMultipleRequest {
            write_access_specifications: specifications,
        })
    }

    /// Execute the write operation and return results.
    pub fn execute(
        request: &WritePropertyMultipleRequest,
        objects: &ObjectRegistry,
    ) -> Vec<WriteAccessResult> {
        let mut results = Vec::with_capacity(request.write_access_specifications.len());

        for spec in &request.write_access_specifications {
            let write_result = Self::write_object_properties(spec, objects);
            results.push(write_result);
        }

        results
    }

    /// Write properties to a single object.
    fn write_object_properties(
        spec: &WriteAccessSpecification,
        objects: &ObjectRegistry,
    ) -> WriteAccessResult {
        let object = match objects.get(&spec.object_id) {
            Some(obj) => obj,
            None => {
                // Object not found - return error for all properties
                let errors = spec
                    .property_values
                    .iter()
                    .map(|pv| WriteAccessResultItem {
                        property_ref: pv.property_ref.clone(),
                        error: Some((ErrorClass::Object, ErrorCode::UnknownObject)),
                    })
                    .collect();

                return WriteAccessResult {
                    object_id: spec.object_id,
                    errors,
                };
            }
        };

        let mut errors = Vec::new();

        for pv in &spec.property_values {
            let result = object.write_property_at(
                pv.property_ref.property_id,
                pv.value.clone(),
                pv.property_ref.array_index,
                pv.priority,
            );

            if let Err(e) = result {
                let (error_class, error_code) = property_error_to_bacnet(e);
                errors.push(WriteAccessResultItem {
                    property_ref: pv.property_ref.clone(),
                    error: Some((error_class, error_code)),
                });
            }
        }

        WriteAccessResult {
            object_id: spec.object_id,
            errors,
        }
    }

    /// Check if all writes were successful.
    pub fn all_successful(results: &[WriteAccessResult]) -> bool {
        results.iter().all(|r| r.errors.is_empty())
    }

    /// Encode an error response for WritePropertyMultiple.
    pub fn encode_error_response(results: &[WriteAccessResult]) -> Vec<u8> {
        let mut encoder = ApduEncoder::new();

        for result in results {
            if result.errors.is_empty() {
                continue;
            }

            // Context tag 0: Object Identifier
            encoder.encode_context_object_identifier(0, result.object_id);

            // Context tag 1: List of errors (opening tag)
            encoder.encode_opening_tag(1);

            for item in &result.errors {
                if let Some((error_class, error_code)) = &item.error {
                    // Context tag 0: Property Identifier
                    encoder.encode_context_enumerated(0, item.property_ref.property_id as u32);

                    // Optional context tag 1: Array Index
                    if let Some(index) = item.property_ref.array_index {
                        encoder.encode_context_unsigned(1, index);
                    }

                    // Context tag 2: Error
                    encoder.encode_opening_tag(2);
                    encoder.encode_enumerated(*error_class as u32);
                    encoder.encode_enumerated(*error_code as u32);
                    encoder.encode_closing_tag(2);
                }
            }

            encoder.encode_closing_tag(1);
        }

        encoder.into_bytes()
    }
}

impl Default for WritePropertyMultipleHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfirmedServiceHandler for WritePropertyMultipleHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::WritePropertyMultiple
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        match Self::decode_request(data) {
            Ok(request) => {
                let results = Self::execute(&request, &ctx.objects);

                if Self::all_successful(&results) {
                    ServiceResult::SimpleAck
                } else {
                    // Return error with first failure
                    let first_error = results
                        .iter()
                        .flat_map(|r| r.errors.iter())
                        .find_map(|e| e.error)
                        .unwrap_or((ErrorClass::Property, ErrorCode::WriteAccessDenied));

                    ServiceResult::Error {
                        error_class: first_error.0,
                        error_code: first_error.1,
                    }
                }
            }
            Err(e) => ServiceResult::Error {
                error_class: ErrorClass::Services,
                error_code: match e {
                    ServiceError::InvalidRequest => ErrorCode::InvalidParameterDataType,
                    ServiceError::UnknownProperty(_) => ErrorCode::UnknownProperty,
                    _ => ErrorCode::Other,
                },
            },
        }
    }

    fn name(&self) -> &'static str {
        "WritePropertyMultiple"
    }

    fn min_data_length(&self) -> usize {
        10 // Minimum: one object-id (4) + opening tag (1) + property-id (1-3) + value
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Decode a property value from a context-tagged section.
fn decode_property_value_from_context(
    decoder: &mut ApduDecoder,
    expected_tag: u8,
) -> Result<BACnetValue, ServiceError> {
    // Check for opening tag
    if decoder.is_opening_tag(expected_tag) {
        decoder.read_u8()?; // consume opening tag
    }

    // Decode the value
    let (tag, is_context, len) = decoder.decode_tag_info()?;

    let value = if is_context {
        // Context-tagged value - treat as unsigned for now
        BACnetValue::Unsigned(decoder.decode_unsigned(len)?)
    } else {
        // Application-tagged value
        match tag {
            0 => BACnetValue::Null,
            1 => BACnetValue::Boolean(len != 0),
            2 => BACnetValue::Unsigned(decoder.decode_unsigned(len)?),
            3 => BACnetValue::Signed(decoder.decode_signed(len)?),
            4 => BACnetValue::Real(decoder.decode_real()?),
            5 => BACnetValue::Double(decoder.decode_double()?),
            6 => {
                let bytes = decoder.read_bytes(len)?;
                BACnetValue::OctetString(bytes.to_vec())
            }
            7 => {
                let s = decoder.decode_character_string(len)?;
                BACnetValue::CharacterString(s)
            }
            8 => {
                let bits = decoder.decode_bit_string(len)?;
                BACnetValue::BitString(bits)
            }
            9 => BACnetValue::Enumerated(decoder.decode_unsigned(len)?),
            12 => BACnetValue::ObjectIdentifier(decoder.decode_object_identifier()?),
            _ => return Err(ServiceError::UnsupportedDataType(tag)),
        }
    };

    // Check for closing tag
    if !decoder.is_empty() && decoder.is_closing_tag(expected_tag) {
        decoder.read_u8()?; // consume closing tag
    }

    Ok(value)
}

/// Convert PropertyError to BACnet error codes.
fn property_error_to_bacnet(error: PropertyError) -> (ErrorClass, ErrorCode) {
    match error {
        PropertyError::NotFound(_) => (ErrorClass::Property, ErrorCode::UnknownProperty),
        PropertyError::WriteAccessDenied(_) => (ErrorClass::Property, ErrorCode::WriteAccessDenied),
        PropertyError::InvalidDataType(_) => (ErrorClass::Property, ErrorCode::InvalidDataType),
        PropertyError::ValueOutOfRange(_) => (ErrorClass::Property, ErrorCode::ValueOutOfRange),
        PropertyError::ReadOnly(_) => (ErrorClass::Property, ErrorCode::WriteAccessDenied),
        PropertyError::InvalidArrayIndex(_) => (ErrorClass::Property, ErrorCode::InvalidArrayIndex),
    }
}

/// Service errors.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("Invalid request format")]
    InvalidRequest,

    #[error("Unknown property: {0}")]
    UnknownProperty(u32),

    #[error("Unsupported data type: {0}")]
    UnsupportedDataType(u8),

    #[error("APDU error: {0}")]
    Apdu(#[from] crate::apdu::types::ApduError),
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::standard::AnalogInput;
    use crate::object::types::ObjectType;

    #[test]
    fn test_property_reference() {
        let prop_ref = PropertyReference::new(PropertyId::PresentValue);
        assert!(!prop_ref.is_all());
        assert!(!prop_ref.is_required());
        assert!(!prop_ref.is_optional());

        let all_ref = PropertyReference::new(PropertyId::All);
        assert!(all_ref.is_all());
    }

    #[test]
    fn test_property_access_result() {
        let success = PropertyAccessResult::success(BACnetValue::Real(25.0));
        assert!(success.is_success());

        let error = PropertyAccessResult::error(ErrorClass::Property, ErrorCode::UnknownProperty);
        assert!(!error.is_success());
    }

    #[test]
    fn test_read_property_multiple_handler() {
        let registry = ObjectRegistry::new();

        let ai1 = Arc::new(AnalogInput::new(1, "Temperature"));
        ai1.set_value(25.0);
        registry.register(ai1);

        let ai2 = Arc::new(AnalogInput::new(2, "Humidity"));
        ai2.set_value(50.0);
        registry.register(ai2);

        let request = ReadPropertyMultipleRequest {
            read_access_specifications: vec![
                ReadAccessSpecification {
                    object_id: ObjectId::new(ObjectType::AnalogInput, 1),
                    property_references: vec![PropertyReference::new(PropertyId::PresentValue)],
                },
                ReadAccessSpecification {
                    object_id: ObjectId::new(ObjectType::AnalogInput, 2),
                    property_references: vec![PropertyReference::new(PropertyId::PresentValue)],
                },
            ],
        };

        let results = ReadPropertyMultipleHandler::execute(&request, &registry);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].results.len(), 1);
        assert!(results[0].results[0].result.is_success());
        assert_eq!(results[1].results.len(), 1);
        assert!(results[1].results[0].result.is_success());
    }

    #[test]
    fn test_read_all_properties() {
        let registry = ObjectRegistry::new();

        let ai = Arc::new(AnalogInput::new(1, "Temperature"));
        ai.set_value(25.0);
        registry.register(ai);

        let request = ReadPropertyMultipleRequest {
            read_access_specifications: vec![ReadAccessSpecification {
                object_id: ObjectId::new(ObjectType::AnalogInput, 1),
                property_references: vec![PropertyReference::new(PropertyId::All)],
            }],
        };

        let results = ReadPropertyMultipleHandler::execute(&request, &registry);

        assert_eq!(results.len(), 1);
        // Should have multiple properties (at least ObjectIdentifier, ObjectName, ObjectType, PresentValue)
        assert!(results[0].results.len() >= 4);
    }

    #[test]
    fn test_write_property_multiple_handler() {
        let registry = ObjectRegistry::new();

        let ai = Arc::new(AnalogInput::new(1, "Temperature"));
        registry.register(ai);

        // Attempt to write to read-only property (ObjectIdentifier is always read-only)
        let request = WritePropertyMultipleRequest {
            write_access_specifications: vec![WriteAccessSpecification {
                object_id: ObjectId::new(ObjectType::AnalogInput, 1),
                property_values: vec![PropertyValueWrite {
                    property_ref: PropertyReference::new(PropertyId::ObjectIdentifier),
                    value: BACnetValue::ObjectIdentifier(ObjectId::new(
                        ObjectType::AnalogInput,
                        999,
                    )),
                    priority: None,
                }],
            }],
        };

        let results = WritePropertyMultipleHandler::execute(&request, &registry);

        assert_eq!(results.len(), 1);
        // ObjectIdentifier is read-only, so write should fail
        assert!(!WritePropertyMultipleHandler::all_successful(&results));
    }

    #[test]
    fn test_unknown_object_error() {
        let registry = ObjectRegistry::new();

        let request = ReadPropertyMultipleRequest {
            read_access_specifications: vec![ReadAccessSpecification {
                object_id: ObjectId::new(ObjectType::AnalogInput, 999),
                property_references: vec![PropertyReference::new(PropertyId::PresentValue)],
            }],
        };

        let results = ReadPropertyMultipleHandler::execute(&request, &registry);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].results.len(), 1);
        assert!(!results[0].results[0].result.is_success());

        if let PropertyAccessResult::Error {
            error_class,
            error_code,
        } = &results[0].results[0].result
        {
            assert_eq!(*error_class, ErrorClass::Object);
            assert_eq!(*error_code, ErrorCode::UnknownObject);
        } else {
            panic!("Expected error result");
        }
    }
}
