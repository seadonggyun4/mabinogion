//! Property services (ReadProperty, WriteProperty, etc.).

use crate::apdu::encoding::{ApduDecoder, ApduEncoder};
use crate::apdu::types::{ConfirmedService, ErrorClass, ErrorCode};
use crate::object::property::{BACnetValue, PropertyError, PropertyId};
use crate::object::types::ObjectId;
use crate::object::ObjectRegistry;

use super::handler::{ConfirmedServiceHandler, ServiceContext, ServiceResult};

/// ReadProperty request.
#[derive(Debug, Clone)]
pub struct ReadPropertyRequest {
    pub object_identifier: ObjectId,
    pub property_identifier: PropertyId,
    pub property_array_index: Option<u32>,
}

/// WriteProperty request.
#[derive(Debug, Clone)]
pub struct WritePropertyRequest {
    pub object_identifier: ObjectId,
    pub property_identifier: PropertyId,
    pub property_array_index: Option<u32>,
    pub property_value: BACnetValue,
    pub priority: Option<u8>,
}

/// Property service for handling property-related requests.
pub struct PropertyService;

impl PropertyService {
    /// Decode a ReadProperty request.
    pub fn decode_read_property(data: &[u8]) -> Result<ReadPropertyRequest, ServiceError> {
        let mut decoder = ApduDecoder::new(data);

        // Context tag 0: Object Identifier
        let (tag, is_context, len) = decoder.decode_tag_info()?;
        if !is_context || tag != 0 || len != 4 {
            return Err(ServiceError::InvalidRequest);
        }
        let object_identifier = decoder.decode_object_identifier()?;

        // Context tag 1: Property Identifier
        let (tag, is_context, len) = decoder.decode_tag_info()?;
        if !is_context || tag != 1 {
            return Err(ServiceError::InvalidRequest);
        }
        let prop_id_value = decoder.decode_unsigned(len)?;
        let property_identifier = PropertyId::from_u32(prop_id_value)
            .ok_or(ServiceError::UnknownProperty(prop_id_value))?;

        // Optional context tag 2: Property Array Index
        let property_array_index = if !decoder.is_empty() {
            let (tag, is_context, len) = decoder.decode_tag_info()?;
            if is_context && tag == 2 {
                Some(decoder.decode_unsigned(len)?)
            } else {
                None
            }
        } else {
            None
        };

        Ok(ReadPropertyRequest {
            object_identifier,
            property_identifier,
            property_array_index,
        })
    }

    /// Encode a ReadProperty ACK.
    pub fn encode_read_property_ack(
        object_id: ObjectId,
        property_id: PropertyId,
        array_index: Option<u32>,
        value: &BACnetValue,
    ) -> Vec<u8> {
        let mut encoder = ApduEncoder::new();

        // Context tag 0: Object Identifier
        encoder.encode_context_object_identifier(0, object_id);

        // Context tag 1: Property Identifier
        encoder.encode_context_enumerated(1, property_id as u32);

        // Context tag 2: Property Array Index (optional)
        if let Some(index) = array_index {
            encoder.encode_context_unsigned(2, index);
        }

        // Context tag 3: Property Value (opening/closing tags)
        encoder.encode_opening_tag(3);
        encoder.encode_value(value);
        encoder.encode_closing_tag(3);

        encoder.into_bytes()
    }

    /// Decode a WriteProperty request.
    pub fn decode_write_property(data: &[u8]) -> Result<WritePropertyRequest, ServiceError> {
        let mut decoder = ApduDecoder::new(data);

        // Context tag 0: Object Identifier
        let (tag, is_context, len) = decoder.decode_tag_info()?;
        if !is_context || tag != 0 || len != 4 {
            return Err(ServiceError::InvalidRequest);
        }
        let object_identifier = decoder.decode_object_identifier()?;

        // Context tag 1: Property Identifier
        let (tag, is_context, len) = decoder.decode_tag_info()?;
        if !is_context || tag != 1 {
            return Err(ServiceError::InvalidRequest);
        }
        let prop_id_value = decoder.decode_unsigned(len)?;
        let property_identifier = PropertyId::from_u32(prop_id_value)
            .ok_or(ServiceError::UnknownProperty(prop_id_value))?;

        // Optional context tag 2: Property Array Index
        let mut property_array_index = None;
        if !decoder.is_empty() && decoder.peek() == Some(0x29) {
            // Context tag 2
            let (_, _, len) = decoder.decode_tag_info()?;
            property_array_index = Some(decoder.decode_unsigned(len)?);
        }

        // Context tag 3: Property Value (opening/closing tags)
        // For now, we'll decode a simple value
        // In a full implementation, this would need to handle constructed values
        let property_value = decode_property_value(&mut decoder)?;

        // Optional context tag 4: Priority
        let priority = if !decoder.is_empty() {
            if let Some(byte) = decoder.peek() {
                if (byte >> 4) == 4 && (byte & 0x08) != 0 {
                    let (_, _, len) = decoder.decode_tag_info()?;
                    Some(decoder.decode_unsigned(len)? as u8)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        Ok(WritePropertyRequest {
            object_identifier,
            property_identifier,
            property_array_index,
            property_value,
            priority,
        })
    }

    /// Handle a ReadProperty request.
    pub fn handle_read_property(
        request: ReadPropertyRequest,
        objects: &ObjectRegistry,
    ) -> Result<BACnetValue, (ErrorClass, ErrorCode)> {
        let object = objects
            .get(&request.object_identifier)
            .ok_or((ErrorClass::Object, ErrorCode::UnknownObject))?;

        object
            .read_property_at(request.property_identifier, request.property_array_index)
            .map_err(|e| property_error_to_bacnet(e))
    }

    /// Handle a WriteProperty request.
    pub fn handle_write_property(
        request: WritePropertyRequest,
        objects: &ObjectRegistry,
    ) -> Result<(), (ErrorClass, ErrorCode)> {
        let object = objects
            .get(&request.object_identifier)
            .ok_or((ErrorClass::Object, ErrorCode::UnknownObject))?;

        object
            .write_property_at(
                request.property_identifier,
                request.property_value,
                request.property_array_index,
                request.priority,
            )
            .map_err(|e| property_error_to_bacnet(e))
    }
}

/// ReadProperty service handler.
pub struct ReadPropertyHandler;

impl ConfirmedServiceHandler for ReadPropertyHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::ReadProperty
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        match PropertyService::decode_read_property(data) {
            Ok(request) => {
                match PropertyService::handle_read_property(request.clone(), &ctx.objects) {
                    Ok(value) => {
                        let response = PropertyService::encode_read_property_ack(
                            request.object_identifier,
                            request.property_identifier,
                            request.property_array_index,
                            &value,
                        );
                        ServiceResult::ComplexAck(response)
                    }
                    Err((error_class, error_code)) => ServiceResult::Error {
                        error_class,
                        error_code,
                    },
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
        "ReadProperty"
    }

    fn min_data_length(&self) -> usize {
        7 // Minimum: object-id (4) + property-id (1-3)
    }
}

/// WriteProperty service handler.
pub struct WritePropertyHandler;

impl ConfirmedServiceHandler for WritePropertyHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::WriteProperty
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        match PropertyService::decode_write_property(data) {
            Ok(request) => match PropertyService::handle_write_property(request, &ctx.objects) {
                Ok(()) => ServiceResult::SimpleAck,
                Err((error_class, error_code)) => ServiceResult::Error {
                    error_class,
                    error_code,
                },
            },
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
        "WriteProperty"
    }

    fn min_data_length(&self) -> usize {
        10 // Minimum: object-id (4) + property-id (1-3) + value
    }
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

/// Decode a property value from the decoder.
fn decode_property_value(decoder: &mut ApduDecoder) -> Result<BACnetValue, ServiceError> {
    // Skip opening tag 3
    if decoder.is_opening_tag(3) {
        decoder.read_u8()?;
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

    // Skip closing tag 3 if present
    if !decoder.is_empty() && decoder.is_closing_tag(3) {
        decoder.read_u8()?;
    }

    Ok(value)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::standard::AnalogInput;
    use crate::object::types::ObjectType;
    use std::sync::Arc;

    #[test]
    fn test_read_property_decode() {
        // ReadProperty request for AI:1, Present_Value
        let data = [
            0x0C, 0x00, 0x00, 0x00, 0x01, // Context 0: Object ID (AI:1)
            0x19, 0x55, // Context 1: Property ID (85 = Present_Value)
        ];

        let request = PropertyService::decode_read_property(&data).unwrap();
        assert_eq!(
            request.object_identifier.object_type,
            ObjectType::AnalogInput
        );
        assert_eq!(request.object_identifier.instance, 1);
        assert_eq!(request.property_identifier, PropertyId::PresentValue);
    }

    #[test]
    fn test_read_property_ack_encode() {
        let object_id = ObjectId::new(ObjectType::AnalogInput, 1);
        let value = BACnetValue::Real(25.5);

        let encoded = PropertyService::encode_read_property_ack(
            object_id,
            PropertyId::PresentValue,
            None,
            &value,
        );

        // Should start with context tag 0 for object ID (tag=0, context=1, len=4 → 0x0C)
        assert_eq!(encoded[0], 0x0C);
    }

    #[test]
    fn test_handle_read_property() {
        let registry = ObjectRegistry::new();
        let ai = Arc::new(AnalogInput::new(1, "Temperature"));
        ai.set_value(25.0);
        registry.register(ai);

        let request = ReadPropertyRequest {
            object_identifier: ObjectId::new(ObjectType::AnalogInput, 1),
            property_identifier: PropertyId::PresentValue,
            property_array_index: None,
        };

        let result = PropertyService::handle_read_property(request, &registry).unwrap();
        assert_eq!(result.as_real(), Some(25.0));
    }
}
